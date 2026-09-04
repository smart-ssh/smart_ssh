//! Schlüsselverwaltung für die Chat-Inhalts-Verschlüsselung (Spec 0036,
//! Abschnitt 4).

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use secrecy::{ExposeSecret, SecretString};

use crate::profiles::{CredentialError, CredentialRef, CredentialStore};

use super::CipherError;

/// Fester Slot im `CredentialStore` (Spec 0036, Abschnitt 4) — "kein neuer
/// Speichermechanismus", derselbe `CredentialStore` wie für Server-
/// Credentials/API-Keys, nur mit einem App-weiten statt einem
/// Server-/Provider-spezifischen Schlüssel (daher das `app:`-Präfix statt
/// einer `ServerId`/`ProviderId`).
pub const CHAT_CONTENT_ENCRYPTION_KEY_REF: &str = "app:chat_content_encryption_key";

const KEY_LEN: usize = 32; // 256 Bit

/// Liest den 256-Bit-Verschlüsselungsschlüssel aus `store`; generiert und
/// speichert bei Bedarf einen neuen, falls noch keiner hinterlegt ist
/// (Spec 0036, Abschnitt 4: "automatische Generierung beim ersten
/// Schreibzugriff, falls noch nicht vorhanden").
///
/// Der Schlüssel wird als Base64-Text abgelegt (`CredentialStore::set`
/// nimmt `SecretString`, keine rohen Bytes) — reine Kodierung, kein
/// zusätzlicher Schutzmechanismus; die eigentliche Vertraulichkeit kommt
/// weiterhin ausschließlich vom OS-Keychain dahinter (Spec 0003,
/// Abschnitt 4).
///
/// Nie ein Panic bei fehlendem/korruptem Schlüssel oder einem Backend-
/// Fehler — liefert stattdessen den passenden [`CipherError`] (s. dortige
/// Varianten-Dokumentation).
pub fn resolve_or_generate_key(store: &dyn CredentialStore) -> Result<[u8; KEY_LEN], CipherError> {
    let credential_ref = CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF);
    match store.get(&credential_ref) {
        Ok(secret) => decode_key(secret.expose_secret()),
        Err(CredentialError::NotFound(_)) => generate_and_store_key(store, &credential_ref),
        Err(CredentialError::Backend(msg)) => Err(CipherError::KeyStoreAccessFailed(msg)),
    }
}

fn decode_key(encoded: &str) -> Result<[u8; KEY_LEN], CipherError> {
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| CipherError::InvalidKey)?;
    bytes.try_into().map_err(|_| CipherError::InvalidKey)
}

fn generate_and_store_key(
    store: &dyn CredentialStore,
    credential_ref: &CredentialRef,
) -> Result<[u8; KEY_LEN], CipherError> {
    let key = generate_key();
    let encoded = BASE64.encode(key);
    store
        .set(credential_ref, SecretString::from(encoded))
        .map_err(|err| match err {
            CredentialError::Backend(msg) => CipherError::KeyStoreAccessFailed(msg),
            CredentialError::NotFound(_) => {
                unreachable!("CredentialStore::set liefert nie NotFound")
            }
        })?;
    Ok(key)
}

fn generate_key() -> [u8; KEY_LEN] {
    use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
    let mut key = [0u8; KEY_LEN];
    OsRng.fill_bytes(&mut key);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct InMemoryCredentialStore {
        entries: Mutex<HashMap<String, SecretString>>,
    }

    impl CredentialStore for InMemoryCredentialStore {
        fn get(&self, r: &CredentialRef) -> Result<SecretString, CredentialError> {
            self.entries
                .lock()
                .unwrap()
                .get(r.as_str())
                .cloned()
                .ok_or_else(|| CredentialError::NotFound(r.clone()))
        }

        fn set(&self, r: &CredentialRef, value: SecretString) -> Result<(), CredentialError> {
            self.entries
                .lock()
                .unwrap()
                .insert(r.as_str().to_string(), value);
            Ok(())
        }

        fn delete(&self, r: &CredentialRef) -> Result<(), CredentialError> {
            self.entries.lock().unwrap().remove(r.as_str());
            Ok(())
        }
    }

    struct AlwaysFailingCredentialStore;
    impl CredentialStore for AlwaysFailingCredentialStore {
        fn get(&self, _r: &CredentialRef) -> Result<SecretString, CredentialError> {
            Err(CredentialError::Backend("Keychain gesperrt".to_string()))
        }
        fn set(&self, _r: &CredentialRef, _value: SecretString) -> Result<(), CredentialError> {
            Err(CredentialError::Backend("Keychain gesperrt".to_string()))
        }
        fn delete(&self, _r: &CredentialRef) -> Result<(), CredentialError> {
            Err(CredentialError::Backend("Keychain gesperrt".to_string()))
        }
    }

    #[test]
    fn test_first_access_generates_and_persists_a_key() {
        let store = InMemoryCredentialStore::default();

        let key = resolve_or_generate_key(&store).unwrap();

        assert_eq!(key.len(), KEY_LEN);
        let stored = store
            .get(&CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF))
            .unwrap();
        assert_eq!(decode_key(stored.expose_secret()).unwrap(), key);
    }

    #[test]
    fn test_second_access_returns_the_same_key_not_a_new_one() {
        let store = InMemoryCredentialStore::default();

        let first = resolve_or_generate_key(&store).unwrap();
        let second = resolve_or_generate_key(&store).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn test_corrupt_stored_key_yields_clear_error_not_panic() {
        let store = InMemoryCredentialStore::default();
        store
            .set(
                &CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF),
                SecretString::from("nicht-valides-base64!!!".to_string()),
            )
            .unwrap();

        assert_eq!(
            resolve_or_generate_key(&store),
            Err(CipherError::InvalidKey)
        );
    }

    #[test]
    fn test_wrong_length_key_yields_clear_error_not_panic() {
        let store = InMemoryCredentialStore::default();
        // Valides Base64, aber nur 4 statt 32 Bytes.
        let short_key_b64 = BASE64.encode([1, 2, 3, 4]);
        store
            .set(
                &CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF),
                SecretString::from(short_key_b64),
            )
            .unwrap();

        assert_eq!(
            resolve_or_generate_key(&store),
            Err(CipherError::InvalidKey)
        );
    }

    #[test]
    fn test_missing_credential_store_backend_yields_clear_error_not_panic() {
        let store = AlwaysFailingCredentialStore;

        assert_eq!(
            resolve_or_generate_key(&store),
            Err(CipherError::KeyStoreAccessFailed(
                "Keychain gesperrt".to_string()
            ))
        );
    }
}
