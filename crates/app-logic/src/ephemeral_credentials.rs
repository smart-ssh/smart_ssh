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
use std::sync::{Mutex, MutexGuard};

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

    /// Recovers from a poisoned mutex: the map only holds plain values, so a
    /// panic in another thread cannot leave it in an inconsistent state.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, SecretString>> {
        self.secrets.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, r: &CredentialRef, value: SecretString) {
        self.lock().insert(r.as_str().to_string(), value);
    }
}

impl CredentialStore for EphemeralCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        self.lock()
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        self.insert(r, value);
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        self.lock().remove(r.as_str());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::sync::Arc;

    fn poisoned() -> Arc<EphemeralCredentialStore> {
        let store = Arc::new(EphemeralCredentialStore::new());
        let s = Arc::clone(&store);
        let _ = std::thread::spawn(move || {
            let _guard = s.secrets.lock().expect("first lock is clean");
            panic!("poison the mutex");
        })
        .join();
        assert!(store.secrets.is_poisoned());
        store
    }

    #[test]
    fn works_after_mutex_poisoning() {
        let store = poisoned();
        let r = CredentialRef::new("ref-a");
        store.insert(&r, SecretString::from("one".to_string()));
        assert_eq!(store.get(&r).unwrap().expose_secret(), "one");
        store
            .set(&r, SecretString::from("two".to_string()))
            .unwrap();
        assert_eq!(store.get(&r).unwrap().expose_secret(), "two");
        store.delete(&r).unwrap();
        assert!(matches!(store.get(&r), Err(CredentialError::NotFound(_))));
    }
}
