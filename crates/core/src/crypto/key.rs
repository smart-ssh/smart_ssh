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

/// Was beim Lesen des Wurzelschlüssels K herauskam (Spec 0101, A3: „K-Zustand").
///
/// **Vier Fälle, keine drei:** Die Entscheidungstabelle A3 behandelt
/// „nicht vorhanden" (dann darf ein neuer K entstehen) und „nicht
/// erreichbar" (dann darf er es auf keinen Fall) völlig verschieden — ein
/// gemeinsamer Fehlerfall wäre genau die Verwechslung, die die
/// Angriffsrichtung „Fehlerart verwechselt (`Backend` als `NotFound`)"
/// beschreibt.
#[derive(Debug)]
pub enum RootKeyState {
    /// K liegt vor.
    Present([u8; KEY_LEN]),
    /// Es gibt keinen Eintrag. Ein neuer K **darf** entstehen (A3, Felder
    /// „K erzeugen").
    NotFound,
    /// Der Zugriff ist gescheitert, oder er wurde wegen eines beim Start
    /// als nicht verfügbar erkannten Schlüsselbunds gar nicht versucht. Ein
    /// neuer K darf **nie** entstehen — er würde den feldweise
    /// verschlüsselten Verlauf unlesbar machen und eine verschlüsselte
    /// Datenbank mit einem zweiten Schlüssel verwaisen lassen.
    Unreachable(String),
    /// Der Eintrag ist da, aber kein gültiger 256-Bit-Schlüssel.
    Invalid,
}

/// Liest den 256-Bit-Wurzelschlüssel K aus `store` — und **erzeugt dabei
/// nie einen neuen** (Spec 0101, A3: „nie still ein neuer K").
///
/// Der Schlüssel liegt als Base64-Text im Store (`CredentialStore::set`
/// nimmt `SecretString`, keine rohen Bytes) — reine Kodierung, kein
/// zusätzlicher Schutzmechanismus; die eigentliche Vertraulichkeit kommt
/// weiterhin ausschließlich vom OS-Keychain dahinter (Spec 0003,
/// Abschnitt 4).
///
/// Nie ein Panic bei fehlendem oder korruptem Schlüssel oder einem
/// Backend-Fehler — jeder dieser Fälle ist eine eigene
/// [`RootKeyState`]-Variante, die der Aufrufer entscheiden muss.
///
/// **Ersetzt das frühere `resolve_or_generate_key`** (Spec 0036), das bei
/// `NotFound` still einen neuen Schlüssel erzeugte und speicherte. Seit
/// Spec 0101 entscheidet der Startablauf nach Tabelle A3, ob ein neuer K
/// entstehen darf; eine Funktion, die es von sich aus tut, wäre genau der
/// Weg, den die Spec ausschließt — sie ist deshalb nicht mehr vorhanden,
/// statt bloß nicht mehr aufgerufen zu werden.
pub fn read_root_key(store: &dyn CredentialStore) -> RootKeyState {
    let credential_ref = CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF);
    match store.get(&credential_ref) {
        Ok(secret) => match decode_key(secret.expose_secret()) {
            Ok(key) => RootKeyState::Present(key),
            Err(_) => RootKeyState::Invalid,
        },
        Err(CredentialError::NotFound(_)) => RootKeyState::NotFound,
        Err(CredentialError::Backend(msg)) => RootKeyState::Unreachable(msg),
    }
}

/// Erzeugt einen neuen Wurzelschlüssel K und legt ihn im Store ab.
///
/// **Nur aus einem der Fälle aufrufen, die A3 dafür vorsieht** („K
/// erzeugen" in der Tabelle, oder nach einer ausdrücklichen Nutzerwahl
/// „Neu anfangen"/„Neuen Schlüssel erzeugen"/„Master-Passwort
/// einrichten"). Die Funktion prüft das nicht — sie kann es nicht, ihr
/// fehlt der Dateizustand. Sie heißt deshalb so, dass ein Aufruf an der
/// falschen Stelle im Code auffällt.
pub fn generate_and_store_root_key(
    store: &dyn CredentialStore,
) -> Result<[u8; KEY_LEN], CipherError> {
    let credential_ref = CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF);
    generate_and_store_key(store, &credential_ref)
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

    /// Spec 0101, A3 (Angriffsrichtung „ein Weg, der doch einen neuen K
    /// erzeugt"): Lesen allein legt **nie** einen Schlüssel an. Vor Spec
    /// 0101 tat `resolve_or_generate_key` genau das — dieser Test hält
    /// fest, dass der Weg nicht zurückkommt.
    #[test]
    fn test_reading_an_absent_key_never_generates_one() {
        let store = InMemoryCredentialStore::default();

        assert!(matches!(read_root_key(&store), RootKeyState::NotFound));

        assert!(
            store.entries.lock().unwrap().is_empty(),
            "das Lesen darf keinen Eintrag hinterlassen"
        );
        // Und auch ein zweites Lesen nicht.
        assert!(matches!(read_root_key(&store), RootKeyState::NotFound));
        assert!(store.entries.lock().unwrap().is_empty());
    }

    #[test]
    fn test_generating_stores_a_key_that_can_be_read_back() {
        let store = InMemoryCredentialStore::default();

        let key = generate_and_store_root_key(&store).unwrap();

        assert_eq!(key.len(), KEY_LEN);
        match read_root_key(&store) {
            RootKeyState::Present(read_back) => assert_eq!(read_back, key),
            other => panic!("erwartet: Present, erhalten: {other:?}"),
        }
    }

    #[test]
    fn test_reading_twice_returns_the_same_key() {
        let store = InMemoryCredentialStore::default();
        generate_and_store_root_key(&store).unwrap();

        let (RootKeyState::Present(first), RootKeyState::Present(second)) =
            (read_root_key(&store), read_root_key(&store))
        else {
            panic!("beide Lesevorgänge müssen Present liefern");
        };
        assert_eq!(first, second);
    }

    #[test]
    fn test_corrupt_stored_key_is_invalid_not_a_panic_and_not_notfound() {
        let store = InMemoryCredentialStore::default();
        store
            .set(
                &CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF),
                SecretString::from("nicht-valides-base64!!!".to_string()),
            )
            .unwrap();

        assert!(matches!(read_root_key(&store), RootKeyState::Invalid));
    }

    #[test]
    fn test_wrong_length_key_is_invalid_not_a_panic() {
        let store = InMemoryCredentialStore::default();
        // Valides Base64, aber nur 4 statt 32 Bytes.
        let short_key_b64 = BASE64.encode([1, 2, 3, 4]);
        store
            .set(
                &CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF),
                SecretString::from(short_key_b64),
            )
            .unwrap();

        assert!(matches!(read_root_key(&store), RootKeyState::Invalid));
    }

    /// A3: Ein Backend-Fehler ist `Unreachable`, **nie** `NotFound` — sonst
    /// entstünde bei einem gesperrten Schlüsselbund ein neuer K.
    #[test]
    fn test_a_backend_failure_is_unreachable_never_notfound() {
        let store = AlwaysFailingCredentialStore;

        match read_root_key(&store) {
            RootKeyState::Unreachable(msg) => assert_eq!(msg, "Keychain gesperrt"),
            other => panic!("erwartet: Unreachable, erhalten: {other:?}"),
        }
    }

    #[test]
    fn test_generating_reports_a_backend_failure_instead_of_panicking() {
        let store = AlwaysFailingCredentialStore;

        assert_eq!(
            generate_and_store_root_key(&store),
            Err(CipherError::KeyStoreAccessFailed(
                "Keychain gesperrt".to_string()
            ))
        );
    }
}
