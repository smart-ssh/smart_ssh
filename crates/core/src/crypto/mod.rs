//! Schlüssel und Kryptografie rund um die verschlüsselte Datenbank (Spec
//! 0101): der Wurzelschlüssel K, die Ableitung des Datenbankschlüssels, die
//! Verpackung unter einem Master-Passwort — und, nur für die einmalige
//! Umstellung, das Lesen der früheren feldweisen Verschlüsselung (Spec
//! 0036, Issue #113, s. [`legacy_field_content`]).
//!
//! Reine Logik (kein I/O außer dem `CredentialStore`-Trait-Aufruf für K) —
//! passt deshalb in `core`, kein eigenes Crate nötig.

/// Spec 0101, A2: Ableitung des SQLCipher-Schlüssels aus dem Wurzelschlüssel
/// K — hier und nicht in `persistence-sqlite`, weil es reine Logik ohne I/O
/// ist und `app-shell` denselben Typ braucht, um die Entscheidungstabelle
/// A3 zu fahren.
mod db_key;
mod key;
/// Spec 0101, A14/A19 (E9): der Wurzelschlüssel K, verpackt unter einem
/// Master-Passwort — Format, Argon2id-Ableitung und AEAD. Reine Logik; das
/// Schreiben und Lesen der Datei liegt in `app-logic`.
mod key_wrapping;
/// Issue #113: nur die Umstellung beim Start liest hiermit Zeilen, die vor
/// dem Rückbau der feldweisen Verschlüsselung geschrieben wurden.
pub mod legacy_field_content;

pub use db_key::{
    root_key_fingerprint, DatabaseKey, DATABASE_KEY_HKDF_INFO, DATABASE_KEY_LEN,
    ROOT_KEY_FINGERPRINT_HKDF_INFO,
};
pub use key::{
    generate_and_store_root_key, generate_root_key, read_root_key, RootKeyState,
    CHAT_CONTENT_ENCRYPTION_KEY_REF,
};
pub use key_wrapping::{
    check_password_length, inspect_wrapped_key, unwrap_root_key, wrap_root_key, KeyWrapError,
    RootKey, MIN_PASSWORD_CHARS, WRAPPING_FORMAT_VERSION, WRAPPING_MAGIC, WRITE_M_COST_KIB,
    WRITE_P_COST, WRITE_T_COST,
};

/// Fehler beim Lesen einer feldweise verschlüsselten Altzeile (s.
/// [`legacy_field_content`]) oder bei der Schlüsselverwaltung für K. Nie ein
/// Panic — ein fehlender/
/// korrupter Schlüssel oder eine fehlgeschlagene Ver-/Entschlüsselung ist
/// ein regulärer, vom Aufrufer zu behandelnder Fehlerfall (Aufgabenstellung:
/// "fehlender/korrupter Schlüssel führt zu einem klaren Fehler, nicht zu
/// einem Panic").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CipherError {
    /// Der Blob war zu kurz, um auch nur den Nonce zu enthalten.
    InvalidBlob,
    /// AEAD-Entschlüsselung ist fehlgeschlagen — bei ChaCha20-Poly1305 der
    /// gemeinsame Fall für "falscher Schlüssel" UND "Chiffrat wurde
    /// manipuliert" (Poly1305 ist ein Authentifizierungs-Tag, kein reiner
    /// Integritäts-Nebeneffekt — beide Fälle sind ununterscheidbar, per
    /// Absicht des AEAD-Designs: kein Orakel für Angreifer, welcher der
    /// beiden Fälle vorliegt).
    DecryptionFailed,
    /// Der im `CredentialStore` hinterlegte Schlüssel-String ließ sich
    /// nicht als gültiger 256-Bit-Schlüssel dekodieren (falsche Länge,
    /// kein valides Base64) — "korrupter Schlüssel" aus der
    /// Aufgabenstellung.
    InvalidKey,
    /// Der `CredentialStore`-Zugriff selbst ist fehlgeschlagen (Backend-
    /// Fehler beim Lesen ODER Schreiben, z. B. OS-Keychain verweigert
    /// Zugriff) — "fehlender Schlüssel" im Sinne von "nicht beschaffbar",
    /// nicht nur "nicht vorhanden" (das wird bei Bedarf automatisch
    /// behoben).
    KeyStoreAccessFailed(String),
}

impl std::fmt::Display for CipherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CipherError::InvalidBlob => write!(f, "ungültiger verschlüsselter Blob"),
            CipherError::DecryptionFailed => write!(f, "Entschlüsselung fehlgeschlagen"),
            CipherError::InvalidKey => write!(f, "ungültiger Verschlüsselungsschlüssel"),
            CipherError::KeyStoreAccessFailed(msg) => {
                write!(f, "Zugriff auf Schlüssel-Speicher fehlgeschlagen: {msg}")
            }
        }
    }
}

impl std::error::Error for CipherError {}
