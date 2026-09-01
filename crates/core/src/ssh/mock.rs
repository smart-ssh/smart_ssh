//! Test-Double für [`SftpSession`] (Spec 0020, Abschnitt 6) — rein
//! In-Memory, kein echtes Dateisystem. Gated hinter `#[cfg(any(test,
//! feature = "test-support"))]` statt nur `#[cfg(test)]`: reine
//! `#[cfg(test)]`-Module sind ausschließlich innerhalb dieser Crate
//! sichtbar, aber die Kontroll-Logik für `ReadRemoteFile`/`WriteRemoteFile`
//! (Spec 0020, Abschnitt 4) lebt in `crates/app-tauri` — das
//! `test-support`-Feature dieser Crate macht `MockSftpSession` für dessen
//! `[dev-dependencies]` nutzbar (dasselbe Muster wie andere Crates es für
//! geteilte Test-Doubles verwenden).

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::error::SshError;
use super::transport::SftpSession;
use super::types::RemoteEntry;

#[derive(Debug, Clone)]
pub struct MockFile {
    pub content: Vec<u8>,
    pub permissions: u32,
    pub modified: Option<DateTime<Utc>>,
}

/// In-Memory-Dateisystem-Fake. Pfade sind flache String-Schlüssel (keine
/// echte Verzeichnisstruktur) — `list_dir` leitet die Kind-Einträge eines
/// Pfads rein aus den vorhandenen Schlüsseln ab (s. dortiger Kommentar).
#[derive(Default)]
pub struct MockSftpSession {
    pub files: HashMap<String, MockFile>,
    /// Jeder Aufruf in Reihenfolge (`"write_file /etc/foo"` etc.) — für
    /// Tests, die prüfen wollen, *ob* (und in welcher Reihenfolge) eine
    /// Methode überhaupt erreicht wurde, z. B. "wurde vor der Bestätigung
    /// nichts geschrieben" (Spec 0020, Abschnitt 4.2, Punkt 2).
    pub calls: Vec<String>,
    /// Pfade, bei denen `write_file` mit
    /// [`SshError::SftpPermissionDenied`] scheitern soll — simuliert Spec
    /// 0020, Abschnitt 4.3, ohne einen echten privilegierten Zielpfad zu
    /// brauchen.
    pub permission_denied_paths: HashSet<String>,
}

impl MockSftpSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(mut self, path: impl Into<String>, content: impl Into<Vec<u8>>) -> Self {
        self.files.insert(
            path.into(),
            MockFile {
                content: content.into(),
                permissions: 0o644,
                modified: Some(Utc::now()),
            },
        );
        self
    }

    pub fn with_permission_denied(mut self, path: impl Into<String>) -> Self {
        self.permission_denied_paths.insert(path.into());
        self
    }

    fn not_found(path: &str) -> SshError {
        SshError::ChannelError(format!("Datei nicht gefunden: {path}"))
    }
}

#[async_trait]
impl SftpSession for MockSftpSession {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.calls.push(format!("list_dir {path}"));
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        Ok(self
            .files
            .iter()
            .filter_map(|(p, f)| {
                let rest = p.strip_prefix(prefix.as_str())?;
                if rest.is_empty() || rest.contains('/') {
                    return None;
                }
                Some(RemoteEntry {
                    name: rest.to_string(),
                    path: p.clone(),
                    is_dir: false,
                    size: f.content.len() as u64,
                    permissions: f.permissions,
                    modified: f.modified,
                })
            })
            .collect())
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.calls.push(format!("read_file {path}"));
        self.files
            .get(path)
            .map(|f| f.content.clone())
            .ok_or_else(|| Self::not_found(path))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        self.calls.push(format!("write_file {path}"));
        if self.permission_denied_paths.contains(path) {
            return Err(SshError::SftpPermissionDenied(format!(
                "keine Schreibrechte für {path}"
            )));
        }
        self.files.insert(
            path.to_string(),
            MockFile {
                content: content.to_vec(),
                permissions: 0o644,
                modified: Some(Utc::now()),
            },
        );
        Ok(())
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.calls.push(format!("stat {path}"));
        self.files
            .get(path)
            .map(|f| RemoteEntry {
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                path: path.to_string(),
                is_dir: false,
                size: f.content.len() as u64,
                permissions: f.permissions,
                modified: f.modified,
            })
            .ok_or_else(|| Self::not_found(path))
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.calls.push(format!("remove {path}"));
        self.files
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| Self::not_found(path))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.calls.push(format!("rename {from} -> {to}"));
        let file = self.files.remove(from).ok_or_else(|| Self::not_found(from))?;
        self.files.insert(to.to_string(), file);
        Ok(())
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.calls.push(format!("create_dir {path}"));
        Ok(())
    }
}
