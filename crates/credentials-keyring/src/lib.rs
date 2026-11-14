//! OS-Keychain-gestützte [`CredentialStore`]-Implementierung (Spec 0003,
//! Abschnitt 4; s. auch `crate::credentials`-Modul-Kommentar in
//! `ssh-manager-core`, der genau diese Crate ankündigt) über die
//! `keyring`-Crate (`v1`-Kompatibilitätsmodus: `Entry::new`/
//! `set_password`/`get_password`/`delete_credential` — plattformunabhängig
//! macOS Keychain Services, Windows Credential Manager, *nix Secret
//! Service).
//!
//! **Eigene Crate statt Teil von `app-shell`**: `app-shell` "enthält keine
//! fachliche Logik" (Spec 0007, Abschnitt 3) — ein OS-Keychain-Wrapper ist
//! zwar keine *fachliche* Logik, aber eine konkrete, austauschbare I/O-
//! Implementierung eines `core`-Traits, also genau die Sorte Baustein, die
//! im gesamten Projekt bislang immer eine eigene Crate bekommen hat
//! (`persistence-sqlite` für `ProfileStore`, `ssh-transport` für
//! `SshTransport`, `ai-providers` für `AiProvider`). Dieselbe Trennung hier
//! zu brechen, nur weil `app-shell` diese eine Implementierung als Erstes
//! braucht, würde das Muster inkonsistent machen, ohne echten Vorteil.
//!
//! **Kein automatisierter Test gegen den echten Keychain**: anders als
//! die übrigen konkreten Implementierungen in diesem Workspace lässt sich
//! diese hier nicht sinnvoll in `cargo test` verifizieren — ein Zugriff auf
//! den echten macOS-/Windows-/Secret-Service-Keychain setzt eine
//! interaktive, entsperrte Sitzung voraus (bei einer unsignierten
//! Dev-Build-Binary ggf. sogar einen einmaligen GUI-Bestätigungsdialog pro
//! Rebuild), was in einer headless/CI-artigen Testumgebung hängen bleiben
//! oder fehlschlagen kann. Analog zum bereits mit `#[ignore]` markierten
//! Zwei-Hop-Jump-Host-Test in `ssh-transport` (Spec 0005, ADR 0008) bleibt
//! diese Implementierung deshalb ungetestet durch `cargo test`; ihre
//! Korrektheit ist stattdessen durch den manuellen Smoke-Test beim Start
//! der Tauri-App (`cargo tauri dev`, Anlegen eines echten AI-Providers) zu
//! verifizieren.

pub mod availability;
mod backend;

#[cfg(target_os = "android")]
pub use backend::set_forced_unavailable;

pub use availability::{
    classify_store_failure, probe_keychain_availability, session_bus_present, store_failure_of,
    store_status_cause_chain_for_log, KeychainAvailability, KeychainUnavailableReason,
    StoreFailure,
};

use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use std::path::{Component, Path, PathBuf};

/// Keychain service of an instance without a data-directory override. All
/// entries of the instance share it; the `CredentialRef` is the account name.
///
/// **User-visible**: this exact string is the "Where" column in macOS
/// Keychain Access. Never change it (no migration exists).
pub const DEFAULT_SERVICE_NAME: &str = "Smart SSH";

/// Issue #1314: the keychain service of this process for a data-directory
/// override. `None` (no override) is exactly [`DEFAULT_SERVICE_NAME`]; with an
/// override it is `Smart SSH (<id>)`, `<id>` being the first 12 hex characters
/// of the SHA-256 of the canonical absolute path, so an instance with its own
/// data directory can never read, replace or delete the secrets of another.
pub fn service_name_for(data_dir_override: Option<&Path>) -> String {
    match data_dir_override {
        None => DEFAULT_SERVICE_NAME.to_string(),
        Some(dir) => {
            let canonical = canonical_dir(dir);
            let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
            let id: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
            format!("{DEFAULT_SERVICE_NAME} ({id})")
        }
    }
}

/// Absolute path with `.`/`..` and trailing separators resolved. Uses the
/// file system (symlinks) when the directory exists, otherwise a lexical
/// normalisation, so the result is deterministic either way.
fn canonical_dir(dir: &Path) -> PathBuf {
    if let Ok(real) = dir.canonicalize() {
        return real;
    }
    let absolute = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| dir.to_path_buf(), |cwd| cwd.join(dir))
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// OS-keychain credential store bound to one service name. There is
/// deliberately no constructor without a service: callers pass the name of
/// their data-directory namespace (see [`service_name_for`]).
pub struct KeyringCredentialStore {
    service: String,
}

impl KeyringCredentialStore {
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Store for the given data-directory override (`None` = default service).
    pub fn for_data_dir(data_dir_override: Option<&Path>) -> Self {
        Self::with_service(service_name_for(data_dir_override))
    }

    /// The service name all entries of this store live under.
    pub fn service(&self) -> &str {
        &self.service
    }

    fn entry(&self, r: &CredentialRef) -> CredentialResult<backend::Entry> {
        backend::Entry::new(&self.service, r.as_str())
            .map_err(|e| CredentialError::Backend(e.to_string()))
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        let entry = self.entry(r)?;
        match entry.get_password() {
            Ok(password) => Ok(SecretString::from(password)),
            Err(keyring::Error::NoEntry) => Err(CredentialError::NotFound(r.clone())),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        let entry = self.entry(r)?;
        entry
            .set_password(value.expose_secret())
            .map_err(|e| CredentialError::Backend(e.to_string()))
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        let entry = self.entry(r)?;
        match entry.delete_credential() {
            // Löschen eines bereits fehlenden Eintrags ist idempotent kein
            // Fehler — passt zum Aufrufer-Verhalten in Spec 0007 Abschnitt
            // 8.2 (`delete_ai_provider` löscht zuerst das Credential, dann
            // die DB-Zeile; ein doppelter Löschversuch nach einem
            // vorherigen Teilfehler darf nicht an einem bereits gelöschten
            // Credential scheitern).
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_override_keeps_the_default_service() {
        assert_eq!(service_name_for(None), "Smart SSH");
        assert_eq!(
            KeyringCredentialStore::for_data_dir(None).service(),
            "Smart SSH"
        );
    }

    #[test]
    fn test_override_service_is_namespaced_and_canonicalised() {
        let a = service_name_for(Some(Path::new("/nonexistent-a/b")));
        assert_eq!(a, service_name_for(Some(Path::new("/nonexistent-a/b/"))));
        assert_eq!(a, service_name_for(Some(Path::new("/nonexistent-a/./b"))));
        assert_eq!(
            a,
            service_name_for(Some(Path::new("/nonexistent-a/x/../b")))
        );
        let id = a
            .strip_prefix("Smart SSH (")
            .and_then(|r| r.strip_suffix(')'))
            .unwrap();
        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        assert_ne!(a, service_name_for(Some(Path::new("/nonexistent-a/c"))));
        assert_ne!(a, "Smart SSH");
    }

    #[test]
    fn test_override_id_is_sha256_prefix_of_the_path() {
        // sha256("/nonexistent-a/b") first 6 bytes, computed independently.
        let digest = Sha256::digest(b"/nonexistent-a/b");
        let expected: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            service_name_for(Some(Path::new("/nonexistent-a/b"))),
            format!("Smart SSH ({expected})")
        );
    }

    /// Issue #1314: no production code constructs a store with a fixed
    /// service; every store comes from the process's namespace.
    #[test]
    fn test_no_hard_coded_service_in_production_call_sites() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for crate_dir in ["app-shell", "app-logic"] {
            let mut stack = vec![root.join(crate_dir).join("src")];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs")
                        && path.file_name().is_some_and(|n| n != "keychain.rs")
                    {
                        let text = std::fs::read_to_string(&path).unwrap();
                        assert!(
                            !text.contains("KeyringCredentialStore::with_service")
                                && !text.contains("DEFAULT_SERVICE_NAME")
                                && !text.contains("KeyringCredentialStore::for_data_dir"),
                            "{} builds a keychain store itself; use app_shell::keychain::process_keyring_store",
                            path.display()
                        );
                    }
                }
            }
        }
    }
}
