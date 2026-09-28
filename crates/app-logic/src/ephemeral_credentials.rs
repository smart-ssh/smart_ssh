//! Kurzlebiger, rein In-Memory-gestützter [`CredentialStore`] für
//! `test_connection` (Spec 0008, Abschnitt 7): "nichts wird dabei
//! persistiert, weder in der DB noch im `CredentialStore`". Die
//! `ssh-transport`/`core::ssh`-Verbindungslogik ist aber komplett um
//! `AuthMethod { credential_ref }` + `&dyn CredentialStore` herum gebaut —
//! ein frisches, noch nie gespeichertes Secret aus dem Formular lässt sich
//! ohne diese Maschinerie zu duplizieren nur einspeisen, indem es kurz in
//! einen Store gelegt wird, der nie etwas außerhalb des Prozessspeichers
//! berührt und mit dem Ende des `test_connection`-Aufrufs automatisch
//! verschwindet (kein `drop`/Aufräum-Schritt nötig).

use std::collections::HashMap;
use std::sync::Mutex;

use secrecy::SecretString;

use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore,
};

#[derive(Default)]
pub struct EphemeralCredentialStore {
    secrets: Mutex<HashMap<String, SecretString>>,
}

impl EphemeralCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, r: &CredentialRef, value: SecretString) {
        self.secrets
            .lock()
            .unwrap()
            .insert(r.as_str().to_string(), value);
    }
}

impl CredentialStore for EphemeralCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        self.secrets
            .lock()
            .unwrap()
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        self.insert(r, value);
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        self.secrets.lock().unwrap().remove(r.as_str());
        Ok(())
    }
}
