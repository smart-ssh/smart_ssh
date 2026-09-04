//! Strukturiertes JSON-Lines-Logging (Spec 0016, Abschnitt 2/3) — Aufbau des
//! globalen `tracing`-Subscribers beim App-Start sowie die
//! altersbasierte Aufbewahrung ("14 Tage") der Log-Dateien.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use directories::BaseDirs;
use tracing_appender::non_blocking::WorkerGuard;

/// Spec 0016, Abschnitt 3: "Aufbewahrung der letzten 14 Tage".
const MAX_LOG_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

const LOG_FILE_PREFIX: &str = "smart-ssh.log";

/// Plattformspezifischer Log-Ordner (Spec 0016, Abschnitt 3):
///
/// - macOS: `~/Library/Logs/Smart SSH/`
/// - Windows: `%APPDATA%\Smart SSH\logs\`
/// - Linux: `~/.local/state/smart-ssh/logs/`
///
/// **Kein `directories::BaseDirs::data_dir()`-Wiederverwendungsmuster wie
/// bei `persistence_sqlite::default_db_path`**: `~/Library/Logs` ist auf
/// macOS ein eigenständiges Standardverzeichnis, das `BaseDirs` nicht als
/// eigenen Accessor anbietet (nur `data_dir` → `Application Support`) —
/// hier deshalb direkt über `home_dir()` zusammengesetzt. Windows nutzt
/// bewusst denselben Basisordner wie die DB (`data_dir` → `%APPDATA%`), nur
/// mit eigenem `logs`-Unterordner, weil das exakt dem in der Spec
/// vorgegebenen `%APPDATA%\Smart SSH\logs\` entspricht. Linux nutzt
/// `state_dir()` (XDG `$XDG_STATE_HOME`, Default `~/.local/state`) statt
/// `data_dir()` — Logs sind laut XDG-Basisverzeichnis-Spezifikation "state
/// data", nicht "data", und die Spec verlangt exakt diesen Pfad.
pub fn default_log_dir() -> PathBuf {
    let base =
        BaseDirs::new().expect("kein Home-Verzeichnis gefunden – kann Log-Ordner nicht ermitteln");

    #[cfg(target_os = "macos")]
    {
        base.home_dir()
            .join("Library")
            .join("Logs")
            .join("Smart SSH")
    }
    #[cfg(target_os = "windows")]
    {
        base.data_dir().join("Smart SSH").join("logs")
    }
    #[cfg(target_os = "linux")]
    {
        base.state_dir()
            .expect("kein XDG-State-Verzeichnis gefunden – kann Log-Ordner nicht ermitteln")
            .join("smart-ssh")
            .join("logs")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        base.data_dir().join("smart-ssh").join("logs")
    }
}

/// Löscht alle Dateien in `dir`, deren letzte Änderung (`mtime`) mehr als
/// `max_age` vor `now` liegt — Spec 0016, Abschnitt 3: "ältere Dateien
/// werden beim Start automatisch gelöscht". Bewusst rein dateibasiert
/// (Änderungszeit statt Parsen des `tracing-appender`-Dateinamensformats):
/// funktioniert unabhängig davon, wie genau `tracing_appender::rolling`
/// seine Dateien benennt, und räumt auch verwaiste Dateien aus einem
/// früheren Namensschema auf.
///
/// `now` als Parameter (statt intern `SystemTime::now()`) macht die
/// Funktion ohne echte 14-Tage-Wartezeit testbar — ein Test kann eine
/// frisch erstellte Datei durch ein weit in der Zukunft liegendes `now`
/// simulieren, ganz ohne die Datei-`mtime` selbst manipulieren zu müssen
/// (s. `tests::test_cleanup_old_logs_*`, die das trotzdem zusätzlich über
/// `File::set_modified` tun, um "neuere Dateien bleiben unangetastet"
/// direkt neben einer wirklich alten Datei zu prüfen).
pub fn cleanup_old_logs(dir: &Path, max_age: Duration, now: SystemTime) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        let age = now.duration_since(modified).unwrap_or_default();
        if age > max_age {
            // Best-effort: ein einzelner nicht löschbarer Log-Rest (z. B.
            // durch eine gleichzeitig laufende zweite Instanz gesperrt)
            // soll den App-Start nicht verhindern.
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// Richtet den globalen `tracing`-Subscriber ein: JSON-Lines in eine
/// täglich rotierende Datei im plattformspezifischen Log-Ordner (Spec
/// 0016, Abschnitt 2/3). Räumt vor dem Öffnen der aktuellen Datei alte
/// Log-Dateien auf (s. [`cleanup_old_logs`]).
///
/// Gibt den [`WorkerGuard`] des nicht-blockierenden Writers zurück — dieser
/// muss so lange am Leben bleiben, wie geloggt werden soll (er flusht den
/// internen Puffer beim `Drop`). Der Aufrufer (`crate::run`) hält ihn
/// deshalb als lokale Variable über die gesamte App-Laufzeit
/// (`tauri::Builder::run` blockiert bis zum Beenden der App, danach ist ein
/// finaler Flush ohnehin nicht mehr relevant).
///
/// Log-Level per `RUST_LOG`-Umgebungsvariable konfigurierbar
/// (`tracing_subscriber::EnvFilter`), Default `info` — reicht für Spec
/// 0016 Abschnitt 4 (Kontext/Chunks/Parsing/Filter-Entscheidung/SSH/
/// Lifecycle sind alle `info`, nicht `debug`), ohne die Log-Dateien mit
/// `trace`-Rauschen aus Bibliotheks-Crates aufzublähen.
pub fn init_logging() -> WorkerGuard {
    let dir = default_log_dir();
    if let Err(err) = fs::create_dir_all(&dir) {
        eprintln!(
            "Log-Ordner {} konnte nicht angelegt werden: {err}",
            dir.display()
        );
    }
    if let Err(err) = cleanup_old_logs(&dir, MAX_LOG_AGE, SystemTime::now()) {
        eprintln!("Alte Log-Dateien konnten nicht aufgeräumt werden: {err}");
    }

    let file_appender = tracing_appender::rolling::daily(&dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Unabhängiger Review-Pass (Spec 0016): Log-Dateien tragen Kommando-
    // Text/System-Kontext (nur best-effort redigiert, s. `log_outgoing_
    // context`/`log_command_execution`) und verdienen dieselbe
    // Zugriffsbeschränkung wie die SQLite-DB und `host_keys.json` (0700/
    // 0600) statt der OS-Standardrechte (typ. 0755/0644, weltlesbar).
    harden_log_permissions(&dir);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .json()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(filter)
        .init();

    guard
}

/// Setzt den Log-Ordner auf `0700` und jede darin liegende Datei auf
/// `0600` — dasselbe Muster wie
/// `persistence_sqlite::store::SqliteProfileStore::connect` (DB-Datei) und
/// `host_key_store::write_atomically` (`host_keys.json`). Best-effort (wie
/// dort): ein fehlgeschlagenes `chmod` verhindert nicht den App-Start.
#[cfg(unix)]
fn harden_log_permissions(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn harden_log_permissions(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use tempfile::tempdir;

    use super::*;

    /// Simuliert Alter über ein weit in der Zukunft liegendes `now` statt
    /// über manipuliertes `mtime` — deckt denselben Pfad wie
    /// `test_cleanup_old_logs_keeps_recent_and_removes_old_file_side_by_side`
    /// mit einer unabhängigen Technik ab (kein `File::set_modified`
    /// beteiligt, das auf manchen Dateisystemen/Plattformen Einschränkungen
    /// haben könnte).
    #[test]
    fn test_cleanup_old_logs_removes_file_when_now_is_far_in_the_future() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("smart-ssh.log.2020-01-01");
        File::create(&path).unwrap();

        let far_future = SystemTime::now() + Duration::from_secs(20 * 24 * 60 * 60);
        cleanup_old_logs(
            dir.path(),
            Duration::from_secs(14 * 24 * 60 * 60),
            far_future,
        )
        .unwrap();

        assert!(!path.exists());
    }

    /// Unabhängiger Review-Pass (Spec 0016): Log-Ordner/-Dateien müssen
    /// dieselbe 0700/0600-Beschränkung bekommen wie die SQLite-DB und
    /// `host_keys.json` (s. `store::tests`/`host_key_store::tests::
    /// test_t10_posix_permissions_enforced` für dasselbe Testmuster).
    #[cfg(unix)]
    #[test]
    fn test_harden_log_permissions_sets_0700_dir_and_0600_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("smart-ssh.log.today");
        File::create(&file_path).unwrap();

        harden_log_permissions(dir.path());

        let dir_mode = fs::metadata(dir.path()).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
        let file_mode = fs::metadata(&file_path).unwrap().permissions().mode();
        assert_eq!(file_mode & 0o777, 0o600);
    }

    #[test]
    fn test_cleanup_old_logs_keeps_file_when_within_max_age() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("smart-ssh.log.today");
        File::create(&path).unwrap();

        let now = SystemTime::now();
        cleanup_old_logs(dir.path(), Duration::from_secs(14 * 24 * 60 * 60), now).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_cleanup_old_logs_keeps_recent_and_removes_old_file_side_by_side() {
        let dir = tempdir().unwrap();
        let old_path = dir.path().join("smart-ssh.log.old");
        let recent_path = dir.path().join("smart-ssh.log.recent");
        File::create(&old_path).unwrap();
        File::create(&recent_path).unwrap();

        let now = SystemTime::now();
        // Alte Datei: mtime auf vor 20 Tagen zurückgesetzt.
        File::options()
            .write(true)
            .open(&old_path)
            .unwrap()
            .set_modified(now - Duration::from_secs(20 * 24 * 60 * 60))
            .unwrap();
        // Neuere Datei: mtime auf vor 1 Tag.
        File::options()
            .write(true)
            .open(&recent_path)
            .unwrap()
            .set_modified(now - Duration::from_secs(24 * 60 * 60))
            .unwrap();

        cleanup_old_logs(dir.path(), Duration::from_secs(14 * 24 * 60 * 60), now).unwrap();

        assert!(
            !old_path.exists(),
            "Datei älter als 14 Tage muss gelöscht werden"
        );
        assert!(
            recent_path.exists(),
            "Datei jünger als 14 Tage muss erhalten bleiben"
        );
    }

    #[test]
    fn test_cleanup_old_logs_on_missing_directory_is_a_no_op() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        let result = cleanup_old_logs(&missing, MAX_LOG_AGE, SystemTime::now());

        assert!(result.is_ok());
    }
}
