use std::fmt;

use secrecy::SecretString;

use super::types::CredentialRef;

/// Fehler eines [`CredentialStore`]-Zugriffs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    NotFound(CredentialRef),
    /// Backend-spezifischer Fehler (z. B. OS-Keychain verweigert Zugriff).
    /// Nur die Fehlermeldung, nie ein Secret-Wert.
    Backend(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::NotFound(r) => {
                write!(f, "kein Credential für Referenz '{}' gefunden", r.as_str())
            }
            CredentialError::Backend(msg) => write!(f, "Credential-Backend-Fehler: {msg}"),
        }
    }
}

impl std::error::Error for CredentialError {}

pub type CredentialResult<T> = Result<T, CredentialError>;

/// Zugriff auf die eigentlichen Secret-Werte hinter einer [`CredentialRef`]
/// (Spec 0003, Abschnitt 4). Die lokale DB kennt nur die opaken
/// `CredentialRef`-Strings; das eigentliche Secret kommt ausschließlich über
/// diesen Trait aus dem OS-Keychain (`keyring`-Crate, s. Spec 0001).
///
/// Als Trait modelliert (analog zu `PolicyStore` in Spec 0002), damit Tests
/// eine In-Memory-Implementierung nutzen können, ohne einen echten
/// OS-Keychain zu brauchen.
pub trait CredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString>;
    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()>;
    fn delete(&self, r: &CredentialRef) -> CredentialResult<()>;
}
