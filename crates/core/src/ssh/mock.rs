//! Test-Double für [`SftpSession`] (Spec 0020, Abschnitt 6) — rein
//! In-Memory, kein echtes Dateisystem. Gated hinter `#[cfg(any(test,
//! feature = "test-support"))]` statt nur `#[cfg(test)]`: reine
//! `#[cfg(test)]`-Module sind ausschließlich innerhalb dieser Crate
//! sichtbar, aber die Kontroll-Logik für `ReadRemoteFile`/`WriteRemoteFile`
//! (Spec 0020, Abschnitt 4) lebt in `crates/app-shell` — das
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
use secrecy::SecretString;

use super::auth::{KeyFileContent, KeyFileError, KeyFileFacts, KeyFileReader};
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
                    uid: None,
                    gid: None,
                    owner: None,
                    group: None,
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
                uid: None,
                gid: None,
                owner: None,
                group: None,
            })
            .ok_or_else(|| Self::not_found(path))
    }

    /// Dieser Mock kennt keine Symlinks (nur flache Dateien, s. Moduldoc-
    /// Kommentar) — identisch zu `stat`, nur mit eigener Aufruf-Verfolgung,
    /// damit ein Test verifizieren kann, DASS `lstat` (statt `stat`)
    /// aufgerufen wurde, falls das für Spec 0054, Teil 3 relevant wird.
    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("lstat {path}"));
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
                uid: None,
                gid: None,
                owner: None,
                group: None,
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
        let file = inner
            .files
            .remove(from)
            .ok_or_else(|| Self::not_found(from))?;
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

    /// Wie `create_dir`: reine Aufruf-Verfolgung, kein echtes
    /// Verzeichnis-Modell (dieser Mock kennt nur flache Dateien, s.
    /// Moduldoc-Kommentar) — für die orchestration-seitigen
    /// `ReadRemoteFile`/`WriteRemoteFile`-Tests, die diesen Mock nutzen,
    /// reicht das; die tatsächliche Rekursions-/Verzeichnis-Logik von Spec
    /// 0054, Teil 3 wird gegen den echten lokalen Pseudo-Server getestet
    /// (`ssh-transport`s Integrationstests).
    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .lock()
            .unwrap()
            .calls
            .push(format!("remove_dir {path}"));
        Ok(())
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(format!("set_permissions {path} {mode:o}"));
        if let Some(file) = inner.files.get_mut(path) {
            file.permissions = mode;
        }
        Ok(())
    }
}

// --- Spec 0076: Attrappe für den Dateizugriff ---------------------------

/// Eine Schlüsseldatei, wie die Attrappe sie kennt — **ohne** Dateisystem
/// (Spec 0076, §4.2: „in Tests steht eine Attrappe, mit der sich jeder
/// Fehlerfall aus A-6 ohne echte Datei herstellen lässt").
#[derive(Debug, Clone)]
pub struct MockKeyFile {
    /// Der Dateiinhalt, wie ihn [`KeyFileReader::read`] als
    /// [`SecretString`] herausgibt.
    pub content: String,
    /// Ob der Schlüssel verschlüsselt ist — eine **Feststellung**, kein
    /// Fehler (A-5): ob daraus ein Fehler folgt, entscheidet
    /// `resolve_auth`.
    pub encrypted: bool,
    /// Rechte zu weit im Sinne von A-4. Wirkt in [`KeyFileReader::read`]
    /// nur bei `enforce_permissions == true` — genau die Unterscheidung,
    /// die Test §6.2.10 prüft.
    pub permissions_too_open: bool,
}

/// Test-Double für [`KeyFileReader`] (Spec 0076, §4.2). Gated wie
/// [`MockSftpSession`] hinter `test-support`, damit auch `app-shell`s
/// Tests ihn nutzen können.
///
/// Zählt seine Aufrufe: Test §6.3.2 verlangt, dass der Vorab-Befund
/// `inspect` benutzt und **nicht** `read` (der Befund darf kein
/// Schlüsselmaterial herausgeben), und E-5 verlangt, dass bei **jedem**
/// Verbindungsaufbau neu gelesen wird.
#[derive(Default, Clone)]
pub struct MockKeyFileReader {
    inner: Arc<StdMutex<KeyFileInner>>,
}

#[derive(Default)]
struct KeyFileInner {
    files: HashMap<String, Result<MockKeyFile, KeyFileError>>,
    calls: Vec<String>,
}

impl MockKeyFileReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Eine lesbare Datei mit gültigem, unverschlüsseltem Schlüssel.
    pub fn with_key(self, path: &str, content: &str) -> Self {
        self.with_file(
            path,
            MockKeyFile {
                content: content.to_string(),
                encrypted: false,
                permissions_too_open: false,
            },
        )
    }

    /// Eine lesbare Datei mit gültigem, **verschlüsseltem** Schlüssel.
    pub fn with_encrypted_key(self, path: &str, content: &str) -> Self {
        self.with_file(
            path,
            MockKeyFile {
                content: content.to_string(),
                encrypted: true,
                permissions_too_open: false,
            },
        )
    }

    pub fn with_file(self, path: &str, file: MockKeyFile) -> Self {
        self.inner
            .lock()
            .unwrap()
            .files
            .insert(path.to_string(), Ok(file));
        self
    }

    /// Stellt genau einen der Fehlerfälle aus A-6 her, ohne eine echte
    /// Datei zu brauchen.
    pub fn with_error(self, path: &str, error: KeyFileError) -> Self {
        self.inner
            .lock()
            .unwrap()
            .files
            .insert(path.to_string(), Err(error));
        self
    }

    /// Alle Aufrufe in Reihenfolge, z. B. `"read /x true"`, `"inspect /x"`.
    pub fn calls(&self) -> Vec<String> {
        self.inner.lock().unwrap().calls.clone()
    }

    pub fn read_calls(&self) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.starts_with("read "))
            .count()
    }

    fn lookup(&self, path: &str) -> Result<MockKeyFile, KeyFileError> {
        match self.inner.lock().unwrap().files.get(path) {
            Some(Ok(file)) => Ok(file.clone()),
            Some(Err(err)) => Err(err.clone()),
            None => Err(KeyFileError::NotFound {
                path: path.to_string(),
            }),
        }
    }
}

impl KeyFileReader for MockKeyFileReader {
    fn read(&self, path: &str, enforce_permissions: bool) -> Result<KeyFileContent, KeyFileError> {
        self.inner
            .lock()
            .unwrap()
            .calls
            .push(format!("read {path} {enforce_permissions}"));
        let file = self.lookup(path)?;
        // A-4/§4.2: zu weite Rechte sperren **nur** den Anmeldepfad. Mit
        // `enforce_permissions == false` (Überführung C-3) wird dieselbe
        // Datei gelesen — sonst wäre der Rettungsknopf aus C-1 genau dann
        // gesperrt, wenn er gebraucht wird.
        if enforce_permissions && file.permissions_too_open {
            return Err(KeyFileError::PermissionsTooOpen {
                path: path.to_string(),
                mode: 0o644,
            });
        }
        Ok(KeyFileContent {
            key: SecretString::from(file.content.clone()),
            encrypted: file.encrypted,
        })
    }

    fn inspect(&self, path: &str) -> KeyFileFacts {
        self.inner
            .lock()
            .unwrap()
            .calls
            .push(format!("inspect {path}"));
        match self.lookup(path) {
            Ok(file) => KeyFileFacts {
                exists: true,
                permissions_too_open: file.permissions_too_open,
                valid_key: true,
                encrypted: file.encrypted,
                problem: None,
            },
            Err(err) => KeyFileFacts {
                // Ein Pfad, der gar keiner ist, und eine fehlende Datei
                // sind die beiden Fälle, in denen nichts existiert; bei
                // allen übrigen gibt es etwas, es taugt nur nicht.
                exists: !matches!(
                    err,
                    KeyFileError::NotFound { .. } | KeyFileError::PathNotAbsolute { .. }
                ),
                permissions_too_open: matches!(err, KeyFileError::PermissionsTooOpen { .. }),
                valid_key: false,
                encrypted: false,
                problem: Some(err),
            },
        }
    }
}
