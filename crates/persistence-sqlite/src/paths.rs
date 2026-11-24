use ssh_manager_core::sync::lock_tolerating_poison;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "android"))]
use directories::BaseDirs;

/// Überschreibt sowohl den Standard-Datenpfad als auch den Debug-Build-
/// Suffix (s. [`default_db_path`]) — wirkt in Debug- UND Release-Builds
/// gleichermaßen (bewusst, damit sich auch ein Release-Artefakt gezielt
/// gegen ein sauberes/bestimmtes Verzeichnis testen lässt, nicht nur
/// `cargo tauri dev`). Leerer Wert wird wie "nicht gesetzt" behandelt,
/// nicht als "aktuelles Verzeichnis" — ein versehentlich leer
/// exportiertes `SMART_SSH_DATA_DIR=` soll nicht stillschweigend auf
/// einen kaum sinnvollen relativen Pfad zeigen.
pub const DATA_DIR_OVERRIDE_ENV: &str = "SMART_SSH_DATA_DIR";

/// Der wirksame Override aus einem Rohwert von `SMART_SSH_DATA_DIR`:
/// `None`, wenn die Variable fehlt **oder leer ist** (s.
/// [`DATA_DIR_OVERRIDE_ENV`]). Rein, ohne Umgebungszugriff, damit die
/// Regel „leer gilt als nicht gesetzt" an einer Stelle steht und sich
/// auch außerhalb dieses Crates prüfen lässt (Issue #44).
pub fn data_dir_override_from(raw: Option<&str>) -> Option<PathBuf> {
    raw.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Der wirksame Override dieses Prozesses, dieselbe Quelle wie
/// [`resolve_data_dir`]: nach [`init_data_dir`] der dort aufgelöste Wert
/// (`--data-dir`, Env oder Zeigerdatei), sonst nur `SMART_SSH_DATA_DIR`.
/// Ein nicht als Unicode lesbarer Wert gilt — wie seit jeher — als nicht
/// gesetzt.
pub fn data_dir_override() -> Option<PathBuf> {
    if let Some(resolved) = RESOLVED.get() {
        return resolved.is_override().then(|| resolved.dir.clone());
    }
    data_dir_override_from(std::env::var(DATA_DIR_OVERRIDE_ENV).ok().as_deref())
}

/// Name of the command-line argument (issue #1313).
pub const DATA_DIR_FLAG: &str = "--data-dir";

/// File extension of the pointer file next to the app (issue #1313).
pub const POINTER_FILE_EXTENSION: &str = "data-dir";

/// Where this process's data directory comes from (issue #1313), highest
/// priority first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDirSource {
    /// Command-line argument `--data-dir`.
    Flag,
    /// Environment variable `SMART_SSH_DATA_DIR`.
    Env,
    /// Pointer file `<name>.data-dir` next to the app.
    PointerFile,
    /// Platform default (no override).
    Default,
}

impl DataDirSource {
    /// Human-readable name for log lines and error texts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "command-line flag --data-dir",
            Self::Env => "environment variable SMART_SSH_DATA_DIR",
            Self::PointerFile => "pointer file next to the app",
            Self::Default => "platform default",
        }
    }
}

/// The data directory, decided once at start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDataDir {
    pub dir: PathBuf,
    pub source: DataDirSource,
    /// For [`DataDirSource::PointerFile`]: the pointer file.
    pub pointer_file: Option<PathBuf>,
}

impl ResolvedDataDir {
    /// Whether an override is active (anything but the platform default).
    pub fn is_override(&self) -> bool {
        self.source != DataDirSource::Default
    }
}

/// The data directory (or its source) is unusable. The caller shows the
/// startup error window; there is never a fallback to the default path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDirError {
    pub path: PathBuf,
    pub source: DataDirSource,
    pub pointer_file: Option<PathBuf>,
    pub reason: String,
}

impl std::fmt::Display for DataDirError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "data directory {} (from {}) is not usable: {}",
            self.path.display(),
            self.source.as_str(),
            self.reason
        )
    }
}

impl std::error::Error for DataDirError {}

/// The value of `--data-dir <path>` / `--data-dir=<path>`. Empty or missing
/// values count as not set; unknown arguments are ignored; on repetition
/// the last non-empty occurrence wins.
pub fn data_dir_flag_from<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut found = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let arg = arg.as_ref();
        let value = if arg == DATA_DIR_FLAG {
            it.next().map(|v| v.as_ref().to_owned())
        } else {
            arg.strip_prefix(DATA_DIR_FLAG)
                .and_then(|rest| rest.strip_prefix('='))
                .map(str::to_owned)
        };
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            found = Some(v);
        }
    }
    found
}

/// Operating system for the pointer-file path rule (a parameter so all three
/// rules are testable on any host).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerOs {
    MacOs,
    Windows,
    Linux,
}

impl PointerOs {
    /// `None` on Android (no pointer file there).
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "android") {
            None
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else {
            Some(Self::Linux)
        }
    }
}

/// Pointer-file path from the executable path:
///
/// - macOS: next to the `.app` folder, `<bundle name without .app>.data-dir`;
/// - Windows: next to the `.exe`, `<name without extension>.data-dir`;
/// - Linux: next to the AppImage (`appimage`) or the executable,
///   `<name without .AppImage>.data-dir`.
///
/// `None` when no name can be derived (e.g. a macOS binary outside a `.app`).
pub fn pointer_file_path(os: PointerOs, exe: &Path, appimage: Option<&Path>) -> Option<PathBuf> {
    let with_ext = |dir: &Path, name: &str| {
        (!name.is_empty()).then(|| dir.join(format!("{name}.{POINTER_FILE_EXTENSION}")))
    };
    match os {
        PointerOs::MacOs => {
            let bundle = exe
                .ancestors()
                .find(|a| a.extension().is_some_and(|e| e == "app"))?;
            let stem = bundle.file_stem()?.to_str()?;
            with_ext(bundle.parent()?, stem)
        }
        PointerOs::Windows => {
            let stem = exe.file_stem()?.to_str()?;
            with_ext(exe.parent()?, stem)
        }
        PointerOs::Linux => {
            let base = appimage
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(exe);
            let name = base.file_name()?.to_str()?;
            let name = match name.len().checked_sub(".appimage".len()) {
                Some(cut)
                    if name.is_char_boundary(cut)
                        && name[cut..].eq_ignore_ascii_case(".appimage") =>
                {
                    &name[..cut]
                }
                _ => name,
            };
            with_ext(base.parent()?, name)
        }
    }
}

/// The path from a pointer file's content: first non-empty line (trimmed),
/// `~` / `~/…` expanded with `home`, a relative path taken relative to the
/// pointer file's folder. `Ok(None)` without a non-empty line (default
/// directory); `Err(HomeUnknown)` for `~` / `~/…` without a known home (an
/// override that cannot be resolved must never fall back to the default).
pub fn pointer_file_target(
    pointer_file: &Path,
    content: &str,
    home: Option<&Path>,
) -> Result<Option<PathBuf>, HomeUnknown> {
    let Some(line) = content.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return Ok(None);
    };
    let path = if line == "~" {
        home.ok_or(HomeUnknown)?.to_path_buf()
    } else if let Some(rest) = line.strip_prefix("~/").or_else(|| line.strip_prefix("~\\")) {
        home.ok_or(HomeUnknown)?.join(rest)
    } else {
        PathBuf::from(line)
    };
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Ok(Some(
            pointer_file.parent().unwrap_or(Path::new("")).join(path),
        ))
    }
}

/// A pointer file names `~` / `~/…` but the home directory is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeUnknown;

/// Precedence flag > env > pointer file as a pure function. Empty values
/// count as not set. `pointer` is `(pointer file, content)` if the file exists.
pub fn select_override(
    flag: Option<&str>,
    env: Option<&str>,
    pointer: Option<(&Path, &str)>,
    home: Option<&Path>,
) -> Result<Option<(PathBuf, DataDirSource)>, DataDirError> {
    if let Some(p) = data_dir_override_from(flag) {
        return Ok(Some((p, DataDirSource::Flag)));
    }
    if let Some(p) = data_dir_override_from(env) {
        return Ok(Some((p, DataDirSource::Env)));
    }
    let Some((file, content)) = pointer else {
        return Ok(None);
    };
    match pointer_file_target(file, content, home) {
        Ok(target) => Ok(target.map(|p| (p, DataDirSource::PointerFile))),
        Err(HomeUnknown) => Err(DataDirError {
            path: file.to_path_buf(),
            source: DataDirSource::PointerFile,
            pointer_file: Some(file.to_path_buf()),
            reason: "the pointer file starts with \"~\" but the home directory is unknown"
                .to_owned(),
        }),
    }
}

static RESOLVED: std::sync::OnceLock<ResolvedDataDir> = std::sync::OnceLock::new();

/// The resolved data directory, if [`init_data_dir`] has run.
pub fn resolved_data_dir() -> Option<&'static ResolvedDataDir> {
    RESOLVED.get()
}

/// Like [`init_data_dir`] with explicit inputs and no process global; the
/// basis for tests. Creates the directory when an override is active.
pub fn resolve_with(
    flag: Option<&str>,
    env: Option<&str>,
    pointer_file: Option<&Path>,
    home: Option<&Path>,
) -> Result<ResolvedDataDir, DataDirError> {
    // The pointer file is only read when neither flag nor env is set.
    let flag_or_env =
        data_dir_override_from(flag).is_some() || data_dir_override_from(env).is_some();
    let mut content = None;
    if let (false, Some(file)) = (flag_or_env, pointer_file) {
        match std::fs::read_to_string(file) {
            Ok(c) => content = Some(c),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(DataDirError {
                    path: file.to_path_buf(),
                    source: DataDirSource::PointerFile,
                    pointer_file: Some(file.to_path_buf()),
                    reason: format!("cannot read the pointer file: {e}"),
                })
            }
        }
    }
    let pointer = pointer_file.zip(content.as_deref());
    let Some((dir, source)) = select_override(flag, env, pointer, home)? else {
        return Ok(ResolvedDataDir {
            dir: platform_default_data_dir(),
            source: DataDirSource::Default,
            pointer_file: None,
        });
    };
    let pointer_file = (source == DataDirSource::PointerFile)
        .then(|| pointer_file.map(Path::to_path_buf))
        .flatten();
    let fail = |reason: String| DataDirError {
        path: dir.clone(),
        source,
        pointer_file: pointer_file.clone(),
        reason,
    };
    std::fs::create_dir_all(&dir).map_err(|e| fail(format!("cannot create it: {e}")))?;
    let meta = std::fs::metadata(&dir).map_err(|e| fail(format!("cannot read it: {e}")))?;
    if !meta.is_dir() {
        return Err(fail("it is not a directory".to_owned()));
    }
    if meta.permissions().readonly() {
        return Err(fail("it is read-only".to_owned()));
    }
    Ok(ResolvedDataDir {
        dir,
        source,
        pointer_file,
    })
}

/// Decides the data directory **once** at start-up (before the logger, no
/// logging) from arguments, environment and pointer file, and remembers it
/// for the whole process; every consumer (DB, logs, stores, speech models,
/// path list, single-instance rule) reads the same value. A second call
/// returns the first value. An unusable directory is an error — never a
/// fallback to the default path.
pub fn init_data_dir<I, S>(args: I) -> Result<&'static ResolvedDataDir, DataDirError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if let Some(done) = RESOLVED.get() {
        return Ok(done);
    }
    let flag = data_dir_flag_from(args);
    let env = std::env::var(DATA_DIR_OVERRIDE_ENV).ok();
    let pointer = PointerOs::current().and_then(|os| {
        let exe = std::env::current_exe().ok()?;
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        pointer_file_path(os, &exe, appimage.as_deref())
    });
    #[cfg(not(target_os = "android"))]
    let home = BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    #[cfg(target_os = "android")]
    let home: Option<PathBuf> = None;
    let flag = if cfg!(target_os = "android") {
        None
    } else {
        flag
    };
    let resolved = resolve_with(
        flag.as_deref(),
        env.as_deref(),
        pointer.as_deref(),
        home.as_deref(),
    )?;
    Ok(RESOLVED.get_or_init(|| resolved))
}

/// Determines the app data directory — basis of [`default_db_path`] and (via
/// its `.parent()`) the host-key store. After [`init_data_dir`] its result
/// applies. Otherwise (tests, tools):
///
/// 1. `SMART_SSH_DATA_DIR`, if set and non-empty (see above).
/// 2. Else the platform data path — unchanged in release builds ("Smart
///    SSH"/`smart-ssh`, identical for Community and Official, see
///    architecture brief D2), with an additional "(dev)"/`-dev` suffix in
///    **debug** builds (`cargo tauri dev`, `#[cfg(debug_assertions)]`).
///
/// Background: before this separation `cargo tauri dev` and a locally built
/// release binary shared one SQLite DB; a dev build with a newer migration
/// left a DB that an older release build failed to open with
/// `Migrate(VersionMissing(n))`. The debug suffix is deliberately a
/// debug-build trait only, not an edition split (see
/// `docs/adr/0032-dev-data-dir-separation-vs-d2.md`).
fn resolve_data_dir() -> PathBuf {
    if let Some(resolved) = RESOLVED.get() {
        return resolved.dir.clone();
    }
    if let Some(override_dir) = data_dir_override() {
        return override_dir;
    }
    platform_default_data_dir()
}

fn platform_default_data_dir() -> PathBuf {
    // Android (Spec 0114, ADR 0148): `BaseDirs::new()` is `None` once `HOME`
    // is missing — the normal case. The app-private folder is fixed.
    #[cfg(target_os = "android")]
    {
        android_files_dir()
    }
    #[cfg(not(target_os = "android"))]
    {
        resolve_desktop_data_dir()
    }
}

/// Bezeichner der App (identisch mit `identifier` in `tauri.conf.json`,
/// per Test abgesichert) — bestimmt den app-privaten Ordner auf Android.
pub const ANDROID_APP_ID: &str = "com.smartssh.desktop";

/// Android assigns each user profile a uid range of this size
/// (`AID_USER_OFFSET`); the user id is `uid / 100000`.
const ANDROID_AID_USER_OFFSET: u32 = 100_000;

/// Android user id (0 = primary user) from a process uid. Pure, so it is
/// testable on any host.
pub fn android_user_id_from_uid(uid: u32) -> u32 {
    uid / ANDROID_AID_USER_OFFSET
}

/// App-private base directory (`/data/data/<id>` for user 0, the historic
/// location; `/data/user/<N>/<id>` for secondary users).
fn android_app_dir_for(app_id: &str, user_id: u32) -> PathBuf {
    if user_id == 0 {
        PathBuf::from("/data/data").join(app_id)
    } else {
        PathBuf::from("/data/user")
            .join(user_id.to_string())
            .join(app_id)
    }
}

/// App-privates `files`-Verzeichnis unter Android (entspricht
/// `Context.getFilesDir()` / Tauris `app_data_dir`) für den Android-Nutzer
/// `user_id`. Rein berechnet, damit es auf jedem Host testbar ist; kein
/// Aufruf von `directories`.
pub fn android_files_dir_for(app_id: &str, user_id: u32) -> PathBuf {
    android_app_dir_for(app_id, user_id).join("files")
}

/// App-privates Cache-Verzeichnis unter Android (`Context.getCacheDir()`).
pub fn android_cache_dir_for(app_id: &str, user_id: u32) -> PathBuf {
    android_app_dir_for(app_id, user_id).join("cache")
}

/// Android user id of this process (`getuid() / 100000`); no `AppHandle` or
/// JNI needed. Off Android there are no user profiles: 0.
fn current_android_user_id() -> u32 {
    #[cfg(target_os = "android")]
    {
        // SAFETY: `getuid` has no preconditions and cannot fail.
        #[expect(unsafe_code, reason = "getuid(2) is infallible")]
        let uid = unsafe { libc::getuid() };
        android_user_id_from_uid(uid)
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// Fehler von [`set_android_app_id`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AndroidAppIdError {
    /// Leer, mit Pfadtrenner oder `..` — würde aus `/data/data` ausbrechen.
    #[error("invalid Android application id: {0:?}")]
    Invalid(String),
    /// Es wurde schon ein Pfad aufgelöst; die Ordner dürfen sich im
    /// Prozess nicht mehr ändern.
    #[error("Android application id can no longer be changed: a path was already resolved")]
    AlreadyResolved,
    /// Ein anderer Wert wurde schon gesetzt.
    #[error("Android application id already set to {0:?}")]
    Conflict(String),
}

/// Set-once-Zustand der Application-ID (Prozess-global in [`APP_ID_STATE`]).
#[derive(Default)]
struct AppIdState {
    inner: std::sync::Mutex<AppIdInner>,
}

#[derive(Default)]
struct AppIdInner {
    id: Option<String>,
    resolved: bool,
}

impl AppIdState {
    fn set(&self, app_id: &str) -> Result<(), AndroidAppIdError> {
        if app_id.is_empty()
            || app_id == "."
            || app_id.contains(['/', '\\', '\0'])
            || app_id.contains("..")
        {
            return Err(AndroidAppIdError::Invalid(app_id.to_owned()));
        }
        let mut g = lock_tolerating_poison(&self.inner);
        if g.resolved {
            return Err(AndroidAppIdError::AlreadyResolved);
        }
        match &g.id {
            Some(existing) if existing != app_id => {
                Err(AndroidAppIdError::Conflict(existing.clone()))
            }
            _ => {
                g.id = Some(app_id.to_owned());
                Ok(())
            }
        }
    }

    /// Liefert die wirksame ID und sperrt weitere Änderungen.
    fn resolve(&self) -> String {
        let mut g = lock_tolerating_poison(&self.inner);
        g.resolved = true;
        g.id.clone().unwrap_or_else(|| ANDROID_APP_ID.to_owned())
    }
}

static APP_ID_STATE: std::sync::LazyLock<AppIdState> =
    std::sync::LazyLock::new(AppIdState::default);

/// Überschreibt die Application-ID für die app-privaten Ordner (einmalig,
/// vor dem ersten Pfad-Lookup). Ohne Aufruf gilt [`ANDROID_APP_ID`].
/// Abgelehnt wird: ungültige ID, Aufruf nach dem ersten Lookup, abweichender
/// zweiter Wert. Kein Effekt auf Desktop-Plattformen.
pub fn set_android_app_id(app_id: &str) -> Result<(), AndroidAppIdError> {
    APP_ID_STATE.set(app_id)
}

/// [`android_files_dir_for`] mit der wirksamen Application-ID.
pub fn android_files_dir() -> PathBuf {
    android_files_dir_for(&APP_ID_STATE.resolve(), current_android_user_id())
}

/// [`android_cache_dir_for`] mit der wirksamen Application-ID.
pub fn android_cache_dir() -> PathBuf {
    android_cache_dir_for(&APP_ID_STATE.resolve(), current_android_user_id())
}

#[cfg(not(target_os = "android"))]
fn resolve_desktop_data_dir() -> PathBuf {
    let base = BaseDirs::new()
        .expect("kein Home-Verzeichnis gefunden – kann App-Datenordner nicht ermitteln");

    #[cfg(all(target_os = "linux", debug_assertions))]
    let app_dir_name = "smart-ssh-dev";
    #[cfg(all(target_os = "linux", not(debug_assertions)))]
    let app_dir_name = "smart-ssh";
    #[cfg(all(not(target_os = "linux"), debug_assertions))]
    let app_dir_name = "Smart SSH (dev)";
    #[cfg(all(not(target_os = "linux"), not(debug_assertions)))]
    let app_dir_name = "Smart SSH";

    base.data_dir().join(app_dir_name)
}

/// Plattformspezifischer Standard-Pfad für die App-Datenbankdatei
/// (Spec 0004, Abschnitt 3, nach der Umbenennung zu "Smart SSH"; Debug-
/// Suffix und `SMART_SSH_DATA_DIR`-Override s. [`resolve_data_dir`], Spec
/// 0041):
///
/// - macOS (Release): `~/Library/Application Support/Smart SSH/smart-ssh.db`
/// - macOS (Debug/`cargo tauri dev`): `~/Library/Application Support/Smart SSH (dev)/smart-ssh.db`
/// - Windows (Release): `%APPDATA%\Smart SSH\smart-ssh.db`
/// - Windows (Debug): `%APPDATA%\Smart SSH (dev)\smart-ssh.db`
/// - Linux (Release): `~/.local/share/smart-ssh/smart-ssh.db`
/// - Linux (Debug): `~/.local/share/smart-ssh-dev/smart-ssh.db`
///
/// Öffentlich (nicht nur intern von [`crate::SqliteProfileStore::connect`]
/// genutzt), damit `app-shell` später denselben Pfad ermitteln kann, ohne
/// die Pfad-Logik zu duplizieren (Aufgabenstellung Teil 2, Punkt 5).
///
/// **`BaseDirs` statt `ProjectDirs`**: `ProjectDirs::from("", "", "Smart SSH")`
/// würde den App-Namen für den Ordnernamen selbst normalisieren (empirisch
/// geprüft: macOS ergäbe `Application Support/Smart-SSH`, Leerzeichen durch
/// Bindestrich ersetzt) — das weicht von den in der Spec vorgegebenen
/// Pfaden ab, die auf macOS/Windows den Ordnernamen mit Leerzeichen
/// verlangen ("Smart SSH"), auf Linux dagegen klein/mit Bindestrich
/// ("smart-ssh"). `BaseDirs::data_dir()` liefert stattdessen nur das
/// unqualifizierte System-Datenverzeichnis (macOS:
/// `~/Library/Application Support`, Windows: `%APPDATA%`, Linux:
/// `~/.local/share`, ohne App-spezifischen Unterordner) — den
/// Ordner-/Dateinamen hängt diese Funktion selbst an, mit exakter Kontrolle
/// über Groß-/Kleinschreibung pro Plattform.
///
/// Panic-Signatur bewusst wie in der Spec/Aufgabenstellung vorgegeben
/// (`-> PathBuf`, kein `Option`/`Result`): `BaseDirs::new` liefert nur dann
/// `None`, wenn das Betriebssystem kein Home-Verzeichnis für den aktuellen
/// Nutzer ermitteln kann — für eine Desktop-App ein praktisch nicht
/// behebbarer Umgebungsfehler, kein regulärer, vom Aufrufer behandelbarer
/// Fall.
pub fn default_db_path() -> PathBuf {
    resolve_data_dir().join("smart-ssh.db")
}

#[cfg(test)]
#[expect(unsafe_code, reason = "tests mutate process env under ENV_LOCK")]
mod tests {
    use super::*;

    #[test]
    fn test_android_files_dir_is_app_private_and_needs_no_home() {
        assert_eq!(
            android_files_dir_for("dev.example.app", 0),
            PathBuf::from("/data/data/dev.example.app/files")
        );
        assert_eq!(
            android_files_dir_for("dev.example.app", 10),
            PathBuf::from("/data/user/10/dev.example.app/files")
        );
        assert_eq!(
            android_cache_dir_for("dev.example.app", 10),
            PathBuf::from("/data/user/10/dev.example.app/cache")
        );
    }

    #[test]
    fn test_android_user_id_from_uid() {
        assert_eq!(android_user_id_from_uid(10_123), 0);
        assert_eq!(android_user_id_from_uid(1_010_123), 10);
        assert_eq!(android_user_id_from_uid(100_000), 1);
    }

    #[test]
    fn test_current_user_resolves_to_primary_location_off_android() {
        assert_eq!(current_android_user_id(), 0);
    }

    #[test]
    fn test_app_id_override_applies_and_rejects_changes() {
        let st = AppIdState::default();
        assert_eq!(st.resolve(), ANDROID_APP_ID);
        let st = AppIdState::default();
        st.set("dev.example.app").unwrap();
        assert_eq!(
            st.set("dev.other.app"),
            Err(AndroidAppIdError::Conflict("dev.example.app".into()))
        );
        let id = st.resolve();
        assert_eq!(
            android_files_dir_for(&id, 0),
            PathBuf::from("/data/data/dev.example.app/files")
        );
        assert_eq!(
            android_cache_dir_for(&id, 0),
            PathBuf::from("/data/data/dev.example.app/cache")
        );
        assert_eq!(
            st.set("dev.example.app"),
            Err(AndroidAppIdError::AlreadyResolved)
        );
        assert_eq!(st.resolve(), "dev.example.app");
    }

    #[test]
    fn test_app_id_rejects_invalid() {
        let st = AppIdState::default();
        for bad in ["", ".", "..", "a/b", "a\\b", "../x", "a..b", "/abs"] {
            assert!(
                matches!(st.set(bad), Err(AndroidAppIdError::Invalid(_))),
                "{bad}"
            );
        }
        assert_eq!(st.resolve(), ANDROID_APP_ID);
    }

    #[test]
    fn test_global_default_and_set_after_lookup_rejected() {
        assert_eq!(
            android_files_dir(),
            PathBuf::from(format!("/data/data/{ANDROID_APP_ID}/files"))
        );
        assert_eq!(
            set_android_app_id("dev.example.app"),
            Err(AndroidAppIdError::AlreadyResolved)
        );
        assert_eq!(
            android_cache_dir(),
            PathBuf::from(format!("/data/data/{ANDROID_APP_ID}/cache"))
        );
    }

    #[test]
    fn test_android_app_id_matches_tauri_identifier() {
        let conf: serde_json::Value = serde_json::from_str(include_str!(
            "../../../apps/smart-ssh-community/tauri.conf.json"
        ))
        .unwrap();
        assert_eq!(conf["identifier"], ANDROID_APP_ID);
    }

    /// Serialisiert alle Tests dieses Moduls — sie mutieren einen
    /// Prozess-globalen Zustand (Umgebungsvariablen), parallele
    /// `cargo test`-Threads würden sich sonst gegenseitig überschreiben.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Räumt `SMART_SSH_DATA_DIR` beim Drop wieder auf, selbst wenn eine
    /// Assertion im Testkörper fehlschlägt/panict — sonst könnte ein
    /// fehlschlagender Test die Variable für nachfolgende Tests gesetzt
    /// lassen (`cargo test` führt alle Tests eines Binaries im selben
    /// Prozess aus).
    struct EnvVarGuard;
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: durch `ENV_LOCK` serialisiert — kein anderer Thread
            // dieses Prozesses liest/schreibt `SMART_SSH_DATA_DIR`
            // gleichzeitig (s. `std::env::set_var`-Doku zur fehlenden
            // Thread-Sicherheit ohne externe Synchronisierung).
            unsafe {
                std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
            }
        }
    }

    #[test]
    fn test_data_dir_override_env_wins_over_default_and_debug_suffix() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _cleanup = EnvVarGuard;
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::set_var(DATA_DIR_OVERRIDE_ENV, "/tmp/smart-ssh-test-override");
        }

        assert_eq!(
            resolve_data_dir(),
            PathBuf::from("/tmp/smart-ssh-test-override")
        );
        assert_eq!(
            default_db_path(),
            PathBuf::from("/tmp/smart-ssh-test-override/smart-ssh.db")
        );
    }

    #[test]
    fn test_empty_data_dir_override_env_is_treated_as_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _cleanup = EnvVarGuard;
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::set_var(DATA_DIR_OVERRIDE_ENV, "");
        }

        // Ein leerer Wert darf nicht als "aktuelles Verzeichnis" (`PathBuf::from("")`)
        // durchschlagen — muss auf den regulären Default zurückfallen.
        let resolved = resolve_data_dir();
        assert_ne!(resolved, PathBuf::from(""));
        assert!(
            resolved.to_string_lossy().contains("Smart SSH")
                || resolved.to_string_lossy().contains("smart-ssh"),
            "erwartete den regulären Default-Pfad, war: {resolved:?}"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_build_default_path_carries_dev_suffix() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Sicherstellen, dass kein Override aus einem vorherigen Test
        // (oder der Umgebung des Testläufers) hier hineinwirkt.
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
        }

        let path = default_db_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("(dev)") || path_str.contains("-dev"),
            "Debug-Build muss einen erkennbaren Dev-Suffix tragen, war: {path_str}"
        );
        assert!(
            !path_str.contains("smart-ssh-dev-dev"),
            "kein doppelter Suffix erwartet: {path_str}"
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_release_build_default_path_has_no_dev_suffix() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
        }

        let path = default_db_path();
        let path_str = path.to_string_lossy();
        assert!(
            !path_str.contains("(dev)") && !path_str.contains("-dev"),
            "Release-Build darf keinen Dev-Suffix tragen — Community/Official müssen \
             identisch bleiben (D2): {path_str}"
        );
    }

    /// Issue #44: die reine Regel hinter dem Override, ohne Umgebung.
    #[test]
    fn test_data_dir_override_from_treats_missing_and_empty_as_unset() {
        assert_eq!(data_dir_override_from(None), None);
        assert_eq!(data_dir_override_from(Some("")), None);
        assert_eq!(
            data_dir_override_from(Some("/tmp/smart-ssh-b")),
            Some(PathBuf::from("/tmp/smart-ssh-b"))
        );
    }

    // ---- Issue #1313: flag, pointer file, precedence ----

    #[test]
    fn test_flag_parses_both_forms_and_ignores_empty_and_unknown() {
        assert_eq!(
            data_dir_flag_from(["app", "--data-dir", "/x"]),
            Some("/x".into())
        );
        assert_eq!(data_dir_flag_from(["--data-dir=/y"]), Some("/y".into()));
        assert_eq!(data_dir_flag_from(["--data-dir="]), None);
        assert_eq!(data_dir_flag_from(["--data-dir", ""]), None);
        assert_eq!(data_dir_flag_from(["--data-dir"]), None);
        assert_eq!(data_dir_flag_from(["--data-directory=/z", "--other"]), None);
        assert_eq!(data_dir_flag_from(Vec::<String>::new()), None);
    }

    #[test]
    fn test_precedence_flag_over_env_over_pointer_over_default_with_empty_values() {
        let file = Path::new("/app/Test.data-dir");
        let ptr = Some((file, "/p\n"));
        let sel = |f, e, p| select_override(f, e, p, None).unwrap();
        assert_eq!(
            sel(Some("/f"), Some("/e"), ptr),
            Some((PathBuf::from("/f"), DataDirSource::Flag))
        );
        assert_eq!(
            sel(None, Some("/e"), ptr),
            Some((PathBuf::from("/e"), DataDirSource::Env))
        );
        assert_eq!(
            sel(Some(""), Some(""), ptr),
            Some((PathBuf::from("/p"), DataDirSource::PointerFile))
        );
        assert_eq!(sel(None, None, Some((file, "\n  \n"))), None);
        assert_eq!(sel(None, None, None), None);
    }

    #[test]
    fn test_pointer_file_target_first_nonempty_line_tilde_and_relative() {
        let file = Path::new("/apps/Test.data-dir");
        let home = Path::new("/home/u");
        let t = |c| pointer_file_target(file, c, Some(home)).unwrap();
        assert_eq!(t("\n  /abs/dir  \nsecond"), Some(PathBuf::from("/abs/dir")));
        assert_eq!(
            t("~/SmartSSH-Test"),
            Some(PathBuf::from("/home/u/SmartSSH-Test"))
        );
        assert_eq!(t("~"), Some(PathBuf::from("/home/u")));
        assert_eq!(t("data"), Some(PathBuf::from("/apps/data")));
        assert_eq!(t("../data"), Some(PathBuf::from("/apps/../data")));
        assert_eq!(t(" \n"), None);
        assert_eq!(pointer_file_target(file, "~/x", None), Err(HomeUnknown));
        assert_eq!(pointer_file_target(file, " \n", None), Ok(None));
    }

    #[test]
    fn test_resolve_with_pointer_tilde_without_home_is_error_not_default() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Test.data-dir");
        for content in ["~", "~/Foo", "\n ~\\Foo\n"] {
            std::fs::write(&file, content).unwrap();
            let err = resolve_with(None, None, Some(&file), None).unwrap_err();
            assert_eq!(err.source, DataDirSource::PointerFile);
            assert_eq!(err.pointer_file.as_deref(), Some(file.as_path()));
            assert_eq!(err.path, file);
            assert!(err.reason.contains("home directory is unknown"));
            // With a known home it resolves as before.
            let home = tempfile::tempdir().unwrap();
            let r = resolve_with(None, None, Some(&file), Some(home.path())).unwrap();
            assert_eq!(r.source, DataDirSource::PointerFile);
        }
        std::fs::write(&file, " \n").unwrap();
        let r = resolve_with(None, None, Some(&file), None).unwrap();
        assert_eq!(r.source, DataDirSource::Default);
    }

    #[test]
    fn test_pointer_file_path_per_os() {
        let p = |os, exe: &str, ai: Option<&str>| {
            pointer_file_path(os, Path::new(exe), ai.map(Path::new))
        };
        assert_eq!(
            p(
                PointerOs::MacOs,
                "/Users/u/Apps/Smart SSH Test.app/Contents/MacOS/smart-ssh",
                None
            ),
            Some(PathBuf::from("/Users/u/Apps/Smart SSH Test.data-dir"))
        );
        assert_eq!(p(PointerOs::MacOs, "/usr/local/bin/smart-ssh", None), None);
        assert_eq!(
            p(PointerOs::Windows, "/c/Apps/Smart SSH Test.exe", None),
            Some(PathBuf::from("/c/Apps/Smart SSH Test.data-dir"))
        );
        assert_eq!(
            p(
                PointerOs::Linux,
                "/tmp/.mount_x/usr/bin/smart-ssh",
                Some("/home/u/Smart_SSH.AppImage")
            ),
            Some(PathBuf::from("/home/u/Smart_SSH.data-dir"))
        );
        assert_eq!(
            p(PointerOs::Linux, "/opt/smart-ssh/smart-ssh", None),
            Some(PathBuf::from("/opt/smart-ssh/smart-ssh.data-dir"))
        );
        assert_eq!(
            p(PointerOs::Linux, "/opt/smart-ssh/smart-ssh", Some("")),
            Some(PathBuf::from("/opt/smart-ssh/smart-ssh.data-dir"))
        );
    }

    #[test]
    fn test_resolve_with_creates_override_dir_and_reports_source() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a").join("b");
        let r = resolve_with(Some(target.to_str().unwrap()), None, None, None).unwrap();
        assert_eq!(r.dir, target);
        assert_eq!(r.source, DataDirSource::Flag);
        assert!(r.is_override());
        assert!(target.is_dir());
    }

    #[test]
    fn test_resolve_with_pointer_file_relative_and_missing_file_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("App.data-dir");
        // No pointer file: platform default, nothing created.
        let r = resolve_with(None, None, Some(&file), None).unwrap();
        assert_eq!(r.source, DataDirSource::Default);
        assert!(!r.is_override());
        std::fs::write(&file, "rel-data\n").unwrap();
        let r = resolve_with(None, None, Some(&file), None).unwrap();
        assert_eq!(r.source, DataDirSource::PointerFile);
        assert_eq!(r.dir, tmp.path().join("rel-data"));
        assert_eq!(r.pointer_file.as_deref(), Some(file.as_path()));
        assert!(r.dir.is_dir());
    }

    #[test]
    fn test_resolve_with_unusable_dir_is_an_error_naming_path_and_source() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        let bad = blocker.join("sub");
        let err = resolve_with(None, Some(bad.to_str().unwrap()), None, None).unwrap_err();
        assert_eq!(err.path, bad);
        assert_eq!(err.source, DataDirSource::Env);
        assert!(err.to_string().contains("SMART_SSH_DATA_DIR"));
        // Pointer file naming an unusable dir keeps the pointer file for the message.
        let ptr = tmp.path().join("P.data-dir");
        std::fs::write(&ptr, bad.to_str().unwrap()).unwrap();
        let err = resolve_with(None, None, Some(&ptr), None).unwrap_err();
        assert_eq!(err.source, DataDirSource::PointerFile);
        assert_eq!(err.pointer_file.as_deref(), Some(ptr.as_path()));
        // A path that is really a file is unusable too.
        let err = resolve_with(Some(blocker.to_str().unwrap()), None, None, None).unwrap_err();
        assert!(err.reason.contains("cannot create"), "{}", err.reason);
    }

    /// Without any override the paths are exactly the documented per-OS
    /// values (regression guard for issue #1313).
    #[cfg(not(target_os = "android"))]
    #[test]
    fn test_default_data_dir_unchanged_per_os() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
        }
        let base = BaseDirs::new().unwrap();
        let name = match (cfg!(target_os = "linux"), cfg!(debug_assertions)) {
            (true, true) => "smart-ssh-dev",
            (true, false) => "smart-ssh",
            (false, true) => "Smart SSH (dev)",
            (false, false) => "Smart SSH",
        };
        assert_eq!(resolve_data_dir(), base.data_dir().join(name));
        assert_eq!(
            default_db_path(),
            base.data_dir().join(name).join("smart-ssh.db")
        );
    }
}
