//! `LocalFileSession`: lokale Implementierung von `SftpSession` (Spec
//! 0020, Abschnitt 3) für den lokalen Pseudo-Server (Spec 0032) — direkter
//! Zugriff über `tokio::fs` statt über das SFTP-Protokoll. Dieselbe
//! Trait-Grenze wie `RusshSftpSession`, deshalb ohne jede Änderung an der
//! KI-Zugriffskontrolle (Spec 0020, Abschnitt 4: `sftp-read`/`sftp-write`
//! laufen für beide Implementierungen identisch durch die Filter-Engine).

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

fn io_err(path: &str, err: std::io::Error) -> SshError {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        // Spec 0020, Abschnitt 4.3: dieselbe Variante wie bei einem SFTP-
        // Rechtefehler — löst in der App-Ebene denselben (für den lokalen
        // Pseudo-Server mangels hinterlegtem Sudo-Passwort typischerweise
        // "kein Fallback möglich"-Pfad aus, s. `write_via_sudo_fallback`),
        // kein separates Verhalten nötig.
        SshError::SftpPermissionDenied(format!("{path}: {err}"))
    } else {
        SshError::ChannelError(format!("{path}: {err}"))
    }
}

fn modified_time(metadata: &std::fs::Metadata) -> Option<DateTime<Utc>> {
    metadata.modified().ok().map(DateTime::<Utc>::from)
}

/// Unix: echte `uid`/`gid` des lokalen Dateisystem-Eintrags. Windows kennt
/// dieses Konzept nicht — dort bleiben beide `None`, genau wie die
/// `owner`/`group`-Namen (deren Auflösung bräuchte einen weiteren, hier
/// nicht vorhandenen Abhängigkeits-Aufwand für eine rein informative
/// Anzeige im lokalen Pseudo-Server, s. `crate::sftp`s Pendant für echtes
/// SFTP, das vom Server gelieferte Namen — falls vorhanden — unverändert
/// durchreicht).
fn owner_ids(metadata: &std::fs::Metadata) -> (Option<u32>, Option<u32>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (Some(metadata.uid()), Some(metadata.gid()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        (None, None)
    }
}

/// Unix: tatsächliche `rwx`-Bits. Windows kennt dieses Konzept nicht (nur
/// ein Nur-Lesen-Flag) — dort ein plausibler fester Platzhalter, passend
/// zum tatsächlichen Nur-Lesen-Status, aber ohne den Anspruch, echte
/// ACL-Rechte abzubilden (die SFTP-`permissions`-Anzeige ist ohnehin nur
/// informativ, s. `crate::sftp`s Gegenstück für echtes SFTP, das dieselben
/// vom Server gelieferten Bits unverändert durchreicht).
fn permission_bits(metadata: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        if metadata.permissions().readonly() {
            0o444
        } else {
            0o644
        }
    }
}

/// Home directory of the user running the app, as the start directory of
/// the local pseudo-server (issue #10) — the counterpart of the login home
/// a real SFTP server resolves `"."` to. `None` if it cannot be determined;
/// callers then keep the process cwd as before.
pub(crate) fn user_home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|home| home.is_absolute())
}

/// Resolves `path` the way an SFTP server resolves a path against the login
/// home (issue #10): a relative path (including `"."` and `./…`) is joined
/// onto `home`; an absolute path — anything starting with a root or, on
/// Windows, a drive/UNC prefix — is returned unchanged. Without a known
/// `home` (and for the empty string) the path stays as it is, i.e. relative
/// to the process cwd, exactly as before this change.
pub(crate) fn resolve_against_home(home: Option<&Path>, path: &str) -> PathBuf {
    let raw = Path::new(path);
    let anchored = matches!(
        raw.components().next(),
        Some(Component::Prefix(_) | Component::RootDir)
    );
    match home {
        Some(home) if !anchored && !path.is_empty() => home.join(raw),
        _ => raw.to_path_buf(),
    }
}

pub struct LocalFileSession {
    /// Base for relative paths (issue #10), see [`resolve_against_home`].
    home: Option<PathBuf>,
}

impl LocalFileSession {
    pub fn new() -> Self {
        Self::with_home(user_home_dir())
    }

    /// Explicit home directory — lets tests inject a temporary directory
    /// instead of mutating the process environment, and lets
    /// `LocalTransport` hand over the home it already determined.
    pub(crate) fn with_home(home: Option<PathBuf>) -> Self {
        Self { home }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        resolve_against_home(self.home.as_deref(), path)
    }
}

impl Default for LocalFileSession {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SftpSession for LocalFileSession {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        let mut read_dir = tokio::fs::read_dir(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))?;
        let mut entries = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|e| io_err(path, e))? {
            let metadata = entry.metadata().await.map_err(|e| io_err(path, e))?;
            let (uid, gid) = owner_ids(&metadata);
            entries.push(RemoteEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                // Built from the caller's `path`, not `entry.path()` (which
                // would carry the resolved home prefix): relative input
                // stays relative output (`"."` → `./name`), as before
                // issue #10, so `remotePath.ts` keeps working unchanged.
                path: Path::new(path)
                    .join(entry.file_name())
                    .to_string_lossy()
                    .into_owned(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                permissions: permission_bits(&metadata),
                modified: modified_time(&metadata),
                uid,
                gid,
                owner: None,
                group: None,
            });
        }
        Ok(entries)
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        tokio::fs::read(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        tokio::fs::write(self.resolve(path), content)
            .await
            .map_err(|e| io_err(path, e))
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let metadata = tokio::fs::metadata(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))?;
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        let (uid, gid) = owner_ids(&metadata);
        Ok(RemoteEntry {
            name,
            path: path.to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            permissions: permission_bits(&metadata),
            modified: modified_time(&metadata),
            uid,
            gid,
            owner: None,
            group: None,
        })
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let metadata = tokio::fs::symlink_metadata(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))?;
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        let (uid, gid) = owner_ids(&metadata);
        Ok(RemoteEntry {
            name,
            path: path.to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            permissions: permission_bits(&metadata),
            modified: modified_time(&metadata),
            uid,
            gid,
            owner: None,
            group: None,
        })
    }

    /// `symlink_metadata` (lstat) statt `metadata` (stat) — SFTP `REMOVE`
    /// (was dieser Trait-Methode entspricht, s. Doc-Kommentar dort) wirkt
    /// nie auf ein Symlink-Ziel, sondern immer auf den Pfad selbst. Mit
    /// `metadata` (folgt Symlinks) würde ein Symlink auf ein Verzeichnis
    /// hier fälschlich als "ist ein Ordner" erkannt und `remove_dir`
    /// aufgerufen — das schlägt für einen Symlink-Pfad mit `ENOTDIR` fehl
    /// (Unix' `rmdir()` verlangt, dass der Pfad selbst ein Verzeichnis
    /// ist, kein Symlink darauf), statt ihn wie erwartet zu entfernen
    /// (gefunden beim Testen von `commands::delete_recursive`s
    /// Symlink-Schutz, Spec 0054, Review des Gesamtpakets).
    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        let metadata = tokio::fs::symlink_metadata(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))?;
        let resolved = self.resolve(path);
        if metadata.is_dir() {
            tokio::fs::remove_dir(resolved).await
        } else {
            tokio::fs::remove_file(resolved).await
        }
        .map_err(|e| io_err(path, e))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        tokio::fs::rename(self.resolve(from), self.resolve(to))
            .await
            .map_err(|e| io_err(from, e))
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        tokio::fs::create_dir(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))
    }

    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        tokio::fs::remove_dir(self.resolve(path))
            .await
            .map_err(|e| io_err(path, e))
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(mode);
            tokio::fs::set_permissions(self.resolve(path), perms)
                .await
                .map_err(|e| io_err(path, e))
        }
        #[cfg(not(unix))]
        {
            // Windows kennt keine Unix-Rechte-Bits — der chmod-Dialog ist
            // dort ohnehin nur gegen den lokalen Pseudo-Server sinnvoll
            // nutzbar, wo er nichts Sinnvolles tun könnte; ein klarer
            // Fehler statt eines stillschweigenden No-ops.
            let _ = mode;
            Err(io_err(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Rechte-Änderung wird auf diesem Betriebssystem nicht unterstützt",
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_then_read_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        let path = path.to_str().unwrap();
        let mut session = LocalFileSession::new();

        session.write_file(path, b"hallo welt").await.unwrap();
        let content = session.read_file(path).await.unwrap();

        assert_eq!(content, b"hallo welt");
    }

    #[tokio::test]
    async fn test_list_dir_finds_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("a.txt");
        tokio::fs::write(&file_path, b"x").await.unwrap();
        let mut session = LocalFileSession::new();

        let entries = session
            .list_dir(dir.path().to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.txt");
        assert!(!entries[0].is_dir);
    }

    #[tokio::test]
    async fn test_stat_reports_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = LocalFileSession::new();

        let entry = session.stat(dir.path().to_str().unwrap()).await.unwrap();

        assert!(entry.is_dir);
    }

    #[tokio::test]
    async fn test_remove_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("gone.txt");
        tokio::fs::write(&file_path, b"x").await.unwrap();
        let mut session = LocalFileSession::new();

        session.remove(file_path.to_str().unwrap()).await.unwrap();

        assert!(!file_path.exists());
    }

    /// Issue #10: the process cwd of `cargo test` is the crate directory,
    /// never a fresh temp directory — so a test that finds the temp
    /// directory's entries under `"."` proves resolution against the
    /// injected home, not against the cwd.
    fn assert_cwd_is_not(home: &Path) {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_ne!(cwd, home.canonicalize().unwrap());
    }

    /// Issue #10: `"."` lists the home directory, not the process cwd, and
    /// the returned paths stay relative (`./name`) as before.
    #[tokio::test]
    async fn test_list_dot_lists_home_not_process_cwd() {
        let home = tempfile::tempdir().unwrap();
        assert_cwd_is_not(home.path());
        tokio::fs::write(home.path().join("in-home.txt"), b"x")
            .await
            .unwrap();
        let mut session = LocalFileSession::with_home(Some(home.path().to_path_buf()));

        let entries = session.list_dir(".").await.unwrap();

        assert_eq!(
            entries.len(),
            1,
            "expected only the home entry: {entries:?}"
        );
        assert_eq!(entries[0].name, "in-home.txt");
        assert_eq!(
            entries[0].path,
            Path::new(".").join("in-home.txt").to_string_lossy()
        );
    }

    /// Issue #10: into a subfolder (via the returned relative path), "up"
    /// (`parentPath` in `remotePath.ts` maps `./sub` back to `"."`) and back
    /// to start all stay under home.
    #[tokio::test]
    async fn test_navigation_into_subfolder_and_back_stays_under_home() {
        let home = tempfile::tempdir().unwrap();
        assert_cwd_is_not(home.path());
        tokio::fs::create_dir(home.path().join("sub"))
            .await
            .unwrap();
        tokio::fs::write(home.path().join("sub").join("inner.txt"), b"x")
            .await
            .unwrap();
        let mut session = LocalFileSession::with_home(Some(home.path().to_path_buf()));

        let start = session.list_dir(".").await.unwrap();
        let sub = start.iter().find(|e| e.name == "sub").unwrap();
        assert!(sub.is_dir);
        let sub_path = sub.path.clone();

        let inside = session.list_dir(&sub_path).await.unwrap();
        assert_eq!(inside.len(), 1);
        assert_eq!(inside[0].name, "inner.txt");
        assert_eq!(
            inside[0].path,
            Path::new(&sub_path).join("inner.txt").to_string_lossy()
        );
        // The frontend joins children with "/" — must work on every OS.
        let joined = format!("{sub_path}/inner.txt");
        assert_eq!(session.read_file(&joined).await.unwrap(), b"x");
        assert!(session.stat(&sub_path).await.unwrap().is_dir);

        let back = session.list_dir(".").await.unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "sub");
    }

    /// Issue #10: write, read, stat, rename, create/remove dir and delete
    /// via relative paths act on files under home.
    #[tokio::test]
    async fn test_relative_file_operations_act_under_home() {
        let home = tempfile::tempdir().unwrap();
        assert_cwd_is_not(home.path());
        let cwd = std::env::current_dir().unwrap();
        let name = "smart-ssh-issue-10-relative-op.txt";
        let mut session = LocalFileSession::with_home(Some(home.path().to_path_buf()));

        session.write_file(name, b"payload").await.unwrap();
        assert!(home.path().join(name).exists());
        assert!(!cwd.join(name).exists(), "must not write into the cwd");
        assert_eq!(session.read_file(name).await.unwrap(), b"payload");
        let dotted = format!("./{name}");
        assert_eq!(session.read_file(&dotted).await.unwrap(), b"payload");
        let entry = session.stat(name).await.unwrap();
        assert_eq!(entry.path, name);
        assert_eq!(entry.size, 7);
        assert!(!session.lstat(name).await.unwrap().is_dir);

        session.rename(name, "renamed.txt").await.unwrap();
        assert!(!home.path().join(name).exists());
        assert!(home.path().join("renamed.txt").exists());

        session.remove("renamed.txt").await.unwrap();
        assert!(!home.path().join("renamed.txt").exists());

        session.create_dir("made-dir").await.unwrap();
        assert!(home.path().join("made-dir").is_dir());
        session.remove_dir("made-dir").await.unwrap();
        assert!(!home.path().join("made-dir").exists());

        session.create_dir("made-dir-2").await.unwrap();
        session.remove("./made-dir-2").await.unwrap();
        assert!(!home.path().join("made-dir-2").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_relative_set_permissions_acts_under_home() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        tokio::fs::write(home.path().join("perm.txt"), b"x")
            .await
            .unwrap();
        let mut session = LocalFileSession::with_home(Some(home.path().to_path_buf()));

        session.set_permissions("perm.txt", 0o600).await.unwrap();

        let mode = std::fs::metadata(home.path().join("perm.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// Issue #10: absolute paths ignore the home directory entirely and
    /// come back exactly as before (`list_dir` paths = `entry.path()`).
    #[tokio::test]
    async fn test_absolute_paths_are_unaffected_by_home() {
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("abs.txt");
        let file_str = file.to_str().unwrap();
        let mut session = LocalFileSession::with_home(Some(home.path().to_path_buf()));

        session.write_file(file_str, b"abs").await.unwrap();
        assert!(file.exists());
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
        assert_eq!(session.read_file(file_str).await.unwrap(), b"abs");
        assert_eq!(session.stat(file_str).await.unwrap().path, file_str);

        let entries = session
            .list_dir(other.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, file.to_string_lossy());

        let renamed = other.path().join("abs2.txt");
        session
            .rename(file_str, renamed.to_str().unwrap())
            .await
            .unwrap();
        assert!(renamed.exists());
        session.remove(renamed.to_str().unwrap()).await.unwrap();
        assert!(!renamed.exists());
    }

    /// Issue #10: without a determinable home, relative paths keep
    /// resolving against the process cwd (the crate directory under
    /// `cargo test`, which contains `Cargo.toml`) instead of failing.
    #[tokio::test]
    async fn test_without_home_relative_paths_fall_back_to_cwd() {
        let mut session = LocalFileSession::with_home(None);

        let entry = session.stat("Cargo.toml").await.unwrap();
        assert!(!entry.is_dir);
        let entries = session.list_dir(".").await.unwrap();
        assert!(entries.iter().any(|e| e.name == "Cargo.toml"));
    }

    #[test]
    fn test_resolve_against_home_rules() {
        let home = std::env::temp_dir().join("smart-ssh-home");
        assert_eq!(resolve_against_home(Some(&home), "."), home.join("."));
        assert_eq!(
            resolve_against_home(Some(&home), "a/b"),
            home.join("a").join("b")
        );
        assert_eq!(resolve_against_home(Some(&home), "./a"), home.join("a"));
        let abs = std::env::temp_dir().join("elsewhere");
        assert_eq!(
            resolve_against_home(Some(&home), abs.to_str().unwrap()),
            abs
        );
        assert_eq!(resolve_against_home(None, "."), PathBuf::from("."));
        assert_eq!(resolve_against_home(Some(&home), ""), PathBuf::from(""));
    }

    /// Issue #10, Windows: both separators count as relative, and
    /// root-relative (`\x`) or drive-prefixed paths are never re-based.
    #[cfg(windows)]
    #[test]
    fn test_resolve_against_home_windows_separators_and_prefixes() {
        let home = PathBuf::from(r"C:\Users\someone");
        assert_eq!(
            resolve_against_home(Some(&home), r".\sub\f.txt"),
            home.join("sub").join("f.txt")
        );
        assert_eq!(
            resolve_against_home(Some(&home), "./sub/f.txt"),
            home.join("sub").join("f.txt")
        );
        assert_eq!(
            resolve_against_home(Some(&home), r"\top"),
            PathBuf::from(r"\top")
        );
        assert_eq!(
            resolve_against_home(Some(&home), r"D:\data"),
            PathBuf::from(r"D:\data")
        );
        assert_eq!(
            resolve_against_home(Some(&home), "D:rel"),
            PathBuf::from("D:rel")
        );
    }

    #[tokio::test]
    async fn test_read_file_missing_yields_channel_error_not_panic() {
        let mut session = LocalFileSession::new();
        let result = session
            .read_file("/this/path/does/not/exist-smart-ssh-test")
            .await;
        assert!(result.is_err());
    }
}
