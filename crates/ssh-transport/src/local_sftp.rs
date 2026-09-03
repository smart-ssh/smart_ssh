//! `LocalFileSession`: lokale Implementierung von `SftpSession` (Spec
//! 0020, Abschnitt 3) für den lokalen Pseudo-Server (Spec 0032) — direkter
//! Zugriff über `tokio::fs` statt über das SFTP-Protokoll. Dieselbe
//! Trait-Grenze wie `RusshSftpSession`, deshalb ohne jede Änderung an der
//! KI-Zugriffskontrolle (Spec 0020, Abschnitt 4: `sftp-read`/`sftp-write`
//! laufen für beide Implementierungen identisch durch die Filter-Engine).

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

pub struct LocalFileSession;

impl LocalFileSession {
    pub fn new() -> Self {
        Self
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
        let mut read_dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| io_err(path, e))?;
        let mut entries = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|e| io_err(path, e))? {
            let metadata = entry.metadata().await.map_err(|e| io_err(path, e))?;
            entries.push(RemoteEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_string_lossy().into_owned(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                permissions: permission_bits(&metadata),
                modified: modified_time(&metadata),
            });
        }
        Ok(entries)
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        tokio::fs::read(path).await.map_err(|e| io_err(path, e))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        tokio::fs::write(path, content)
            .await
            .map_err(|e| io_err(path, e))
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| io_err(path, e))?;
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        Ok(RemoteEntry {
            name,
            path: path.to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            permissions: permission_bits(&metadata),
            modified: modified_time(&metadata),
        })
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| io_err(path, e))?;
        if metadata.is_dir() {
            tokio::fs::remove_dir(path).await
        } else {
            tokio::fs::remove_file(path).await
        }
        .map_err(|e| io_err(path, e))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        tokio::fs::rename(from, to)
            .await
            .map_err(|e| io_err(from, e))
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        tokio::fs::create_dir(path)
            .await
            .map_err(|e| io_err(path, e))
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

    #[tokio::test]
    async fn test_read_file_missing_yields_channel_error_not_panic() {
        let mut session = LocalFileSession::new();
        let result = session
            .read_file("/this/path/does/not/exist-smart-ssh-test")
            .await;
        assert!(result.is_err());
    }
}
