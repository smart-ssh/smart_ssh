//! Test-Double für [`SftpSession`] (Spec 0020, Abschnitt 6) — rein
//! In-Memory, kein echtes Dateisystem. Gated hinter `#[cfg(any(test,
//! feature = "test-support"))]` statt nur `#[cfg(test)]`: reine
//! `#[cfg(test)]`-Module sind ausschließlich innerhalb dieser Crate
//! sichtbar, aber die Kontroll-Logik für `ReadRemoteFile`/`WriteRemoteFile`
//! (Spec 0020, Abschnitt 4) lebt in `crates/app-tauri` — das
//! `test-support`-Feature dieser Crate macht `MockSftpSession` für dessen
//! `[dev-dependencies]` nutzbar (dasselbe Muster wie andere Crates es für
//! geteilte Test-Doubles verwenden).
//!
//! Zustand liegt hinter `Arc<StdMutex<..>>` statt als einfaches Feld:
//! `MockSftpSession` wird typischerweise als `Box<dyn SftpSession>` in eine
//! `Session` verschoben (s. `app_tauri::session::Session::sftp`), ein Test
//! kann danach also nicht mehr direkt auf das ursprüngliche Objekt
//! zugreifen. Ein vor dem Verschieben gezogener `.clone()` (billig — teilt
//! sich denselben `Arc`) bleibt als Prüf-Handle nutzbar, um z. B. zu
//! verifizieren, *ob* (und mit welchem Inhalt) eine Methode aufgerufen
//! wurde, nachdem der eigentliche Aufruf längst gelaufen ist.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};

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

#[derive(Default)]
struct Inner {
    files: HashMap<String, MockFile>,
    /// Jeder Aufruf in Reihenfolge (`"write_file /etc/foo"` etc.) — für
    /// Tests, die prüfen wollen, *ob* (und in welcher Reihenfolge) eine
    /// Methode überhaupt erreicht wurde, z. B. "wurde vor der Bestätigung
    /// nichts geschrieben" (Spec 0020, Abschnitt 4.2, Punkt 2).
    calls: Vec<String>,
    /// Pfade, bei denen `write_file` mit
    /// [`SshError::SftpPermissionDenied`] scheitern soll — simuliert Spec
    /// 0020, Abschnitt 4.3, ohne einen echten privilegierten Zielpfad zu
    /// brauchen.
    permission_denied_paths: HashSet<String>,
}

#[derive(Default, Clone)]
pub struct MockSftpSession {
    inner: Arc<StdMutex<Inner>>,
}

impl MockSftpSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(self, path: impl Into<String>, content: impl Into<Vec<u8>>) -> Self {
        self.inner.lock().unwrap().files.insert(
            path.into(),
            MockFile {
                content: content.into(),
                permissions: 0o644,
                modified: Some(Utc::now()),
            },
        );
        self
    }

    pub fn with_permission_denied(self, path: impl Into<String>) -> Self {
        self.inner
            .lock()
            .unwrap()
            .permission_denied_paths
            .insert(path.into());
        self
    }

    /// Aufrufe in Reihenfolge, seit Erzeugung dieses (ggf. geklonten)
    /// Handles — s. Modul-Doc-Kommentar zum `Arc`-Zustand.
    pub fn calls(&self) -> Vec<String> {
        self.inner.lock().unwrap().calls.clone()
    }

    /// Aktueller Inhalt eines Pfads, falls (noch) vorhanden — für Tests, die
    /// nach einem `write_file`/`remove`/`rename` den resultierenden Zustand
    /// direkt prüfen wollen, ohne selbst wieder über den Trait zu lesen.
    pub fn file_content(&self, path: &str) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .files
            .get(path)
            .map(|f| f.content.clone())
    }

    fn not_found(path: &str) -> SshError {
        SshError::ChannelError(format!("Datei nicht gefunden: {path}"))
    }
}

#[async_trait]
impl SftpSession for MockSftpSession {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("list_dir {path}"));
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        Ok(inner
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
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("read_file {path}"));
        inner
            .files
            .get(path)
            .map(|f| f.content.clone())
            .ok_or_else(|| Self::not_found(path))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("write_file {path}"));
        if inner.permission_denied_paths.contains(path) {
            return Err(SshError::SftpPermissionDenied(format!(
                "keine Schreibrechte für {path}"
            )));
        }
        inner.files.insert(
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
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("stat {path}"));
        inner
            .files
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
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("remove {path}"));
        inner
            .files
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| Self::not_found(path))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("rename {from} -> {to}"));
        let file = inner.files.remove(from).ok_or_else(|| Self::not_found(from))?;
        inner.files.insert(to.to_string(), file);
        Ok(())
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .lock()
            .unwrap()
            .calls
            .push(format!("create_dir {path}"));
        Ok(())
    }
}
