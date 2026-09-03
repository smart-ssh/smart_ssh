//! `russh-sftp`-gestützte Implementierung von `SftpSession` (Spec 0020,
//! Abschnitt 3) — läuft über einen `sftp`-Subsystem-Channel derselben
//! `SshTransport`-Verbindung, kein zweiter Verbindungsaufbau.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use russh_sftp::client::error::Error as RusshSftpError;
use russh_sftp::client::SftpSession as RusshSftpClient;
use russh_sftp::protocol::{FileAttributes, StatusCode};
use tokio::io::AsyncWriteExt;

use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

pub struct RusshSftpSession {
    inner: RusshSftpClient,
}

impl RusshSftpSession {
    pub(crate) fn new(inner: RusshSftpClient) -> Self {
        Self { inner }
    }
}

/// Wandelt einen `russh_sftp::client::error::Error` in ein [`SshError`].
/// `PermissionDenied` bekommt bewusst eine eigene [`SshError`]-Variante
/// (Spec 0020, Abschnitt 4.3: App-Ebene muss diesen einen Fall zuverlässig
/// vom allgemeinen Fehlerfall unterscheiden können, für den Sudo-Rechte-
/// Fallback) — alles andere landet in `ChannelError` mit der
/// Original-Fehlermeldung, analog zu `crate::error::map_russh_error`.
fn map_sftp_error(path: &str, e: RusshSftpError) -> SshError {
    match &e {
        RusshSftpError::Status(status) if status.status_code == StatusCode::PermissionDenied => {
            SshError::SftpPermissionDenied(format!("{path}: {e}"))
        }
        _ => SshError::ChannelError(format!("SFTP-Fehler bei '{path}': {e}")),
    }
}

/// `permissions` liefert `russh-sftp` als rohen `st_mode`-Wert inkl.
/// Dateityp-Bits — `RemoteEntry::permissions` soll nur die reinen
/// Rechte-Bits tragen (s. dortiger Doc-Kommentar), daher hier maskiert.
fn remote_entry(name: String, path: String, metadata: &FileAttributes) -> RemoteEntry {
    RemoteEntry {
        name,
        path,
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        permissions: metadata.permissions.unwrap_or(0) & 0o7777,
        modified: metadata.modified().ok().map(DateTime::<Utc>::from),
    }
}

#[async_trait]
impl SftpSession for RusshSftpSession {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        let entries = self
            .inner
            .read_dir(path)
            .await
            .map_err(|e| map_sftp_error(path, e))?;
        Ok(entries
            .map(|entry| {
                let metadata = entry.metadata();
                remote_entry(entry.file_name(), entry.path(), &metadata)
            })
            .collect())
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.inner
            .read(path)
            .await
            .map_err(|e| map_sftp_error(path, e))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        // Bewusst nicht `SftpSession::write()`: dessen Implementierung öffnet
        // nur mit `OpenFlags::WRITE` (kein `CREATE`) und scheitert deshalb
        // mit "No such file", sobald die Zieldatei noch nicht existiert —
        // für `write_file` (erstellt oder überschreibt, Spec 0020, Abschnitt
        // 3/4.2) das falsche Verhalten. `create()` (CREATE|TRUNCATE|WRITE)
        // deckt beide Fälle korrekt ab.
        let mut file = self
            .inner
            .create(path)
            .await
            .map_err(|e| map_sftp_error(path, e))?;
        file.write_all(content)
            .await
            .map_err(|e| SshError::ChannelError(format!("SFTP-Schreibfehler bei '{path}': {e}")))?;
        // `File::poll_write` reicht Schreibanfragen nur "fire-and-forget"
        // weiter (`write_nowait`) — erst `shutdown()`/`flush()` wartet auf
        // die ausstehenden Bestätigungen. Ohne diesen Aufruf lief der
        // Handle in die `Drop`-Implementierung (`close_nowait`, Antwort
        // nicht abgewartet) — in der Praxis unauffällig, aber kein
        // garantiert abgeschlossener Schreibvorgang, bevor `write_file`
        // zurückkehrt.
        file.shutdown()
            .await
            .map_err(|e| SshError::ChannelError(format!("SFTP-Schreibfehler bei '{path}': {e}")))?;
        Ok(())
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let metadata = self
            .inner
            .metadata(path)
            .await
            .map_err(|e| map_sftp_error(path, e))?;
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        Ok(remote_entry(name, path.to_string(), &metadata))
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .remove_file(path)
            .await
            .map_err(|e| map_sftp_error(path, e))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.inner
            .rename(from, to)
            .await
            .map_err(|e| map_sftp_error(from, e))
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .create_dir(path)
            .await
            .map_err(|e| map_sftp_error(path, e))
    }
}
