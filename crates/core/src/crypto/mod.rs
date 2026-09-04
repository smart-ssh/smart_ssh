//! Feld-Verschlüsselung für persistierte Chat-Inhalte (Spec 0036).
//!
//! Reine Logik (kein I/O außer dem `CredentialStore`-Trait-Aufruf für die
//! Schlüsselverwaltung, s. [`resolve_or_generate_key`]) — passt deshalb in
//! `core`, kein eigenes Crate nötig (derselbe Präzedenzfall wie
//! `ai::redactor`: Trait UND konkrete Implementierung leben zusammen hier,
//! da `chacha20poly1305` eine reine Rust-Implementierung ohne C-Linking/
//! OS-Abhängigkeit ist — Spec 0036, Abschnitt 3 nennt genau das als Grund
//! für die Wahl dieser Crate).

mod chacha;
mod key;

pub use chacha::ChaCha20Poly1305Cipher;
pub use key::{resolve_or_generate_key, CHAT_CONTENT_ENCRYPTION_KEY_REF};

/// Fehler bei einem [`ContentCipher`]-Zugriff oder der Schlüsselverwaltung
/// (s. [`resolve_or_generate_key`]). Nie ein Panic — ein fehlender/
/// korrupter Schlüssel oder eine fehlgeschlagene Ver-/Entschlüsselung ist
/// ein regulärer, vom Aufrufer zu behandelnder Fehlerfall (Aufgabenstellung:
/// "fehlender/korrupter Schlüssel führt zu einem klaren Fehler, nicht zu
/// einem Panic").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CipherError {
    /// Der Blob war zu kurz, um auch nur den Nonce zu enthalten, oder
    /// sonst strukturell ungültig (s. [`EncryptedContent::from_blob`]).
    InvalidBlob,
    /// AEAD-Verschlüsselung ist fehlgeschlagen — bei `chacha20poly1305`
    /// praktisch nur bei einem fehlerhaften Schlüssel möglich.
    EncryptionFailed,
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
    /// behoben, s. [`resolve_or_generate_key`]).
    KeyStoreAccessFailed(String),
}

impl std::fmt::Display for CipherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CipherError::InvalidBlob => write!(f, "ungültiger verschlüsselter Blob"),
            CipherError::EncryptionFailed => write!(f, "Verschlüsselung fehlgeschlagen"),
            CipherError::DecryptionFailed => write!(f, "Entschlüsselung fehlgeschlagen"),
            CipherError::InvalidKey => write!(f, "ungültiger Verschlüsselungsschlüssel"),
            CipherError::KeyStoreAccessFailed(msg) => {
                write!(f, "Zugriff auf Schlüssel-Speicher fehlgeschlagen: {msg}")
            }
        }
    }
}

impl std::error::Error for CipherError {}

/// Länge des Nonce für ChaCha20-Poly1305 (96 Bit, Standard laut RFC 8439 —
/// `chacha20poly1305`s eigener `Nonce`-Typ hat exakt diese Größe).
const NONCE_LEN: usize = 12;

/// Ergebnis einer [`ContentCipher::encrypt`]-Operation (Spec 0036,
/// Abschnitt 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedContent {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; NONCE_LEN],
}

impl EncryptedContent {
    /// "Gespeichert wird `nonce || ciphertext` als ein zusammenhängender
    /// Blob pro Zeile" (Spec 0036, Abschnitt 3) — die Form, die tatsächlich
    /// in `chat_messages.content` (jetzt `BLOB`) landet.
    pub fn to_blob(&self) -> Vec<u8> {
        let mut blob = Vec::with_capacity(NONCE_LEN + self.ciphertext.len());
        blob.extend_from_slice(&self.nonce);
        blob.extend_from_slice(&self.ciphertext);
        blob
    }

    /// Kehrt [`Self::to_blob`] um. `CipherError::InvalidBlob`, falls
    /// `blob` kürzer als ein Nonce ist — nie ein Index-Panic auf
    /// unerwartet kurzen/korrupten Daten.
    pub fn from_blob(blob: &[u8]) -> Result<Self, CipherError> {
        if blob.len() < NONCE_LEN {
            return Err(CipherError::InvalidBlob);
        }
        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(nonce_bytes);
        Ok(Self {
            nonce,
            ciphertext: ciphertext.to_vec(),
        })
    }
}

/// Verschlüsselt/entschlüsselt den Inhalt einer einzelnen Chat-Nachricht
/// (Spec 0036, Abschnitt 3) — als Trait, damit Tests eine In-Memory-/
/// Mock-Implementierung nutzen können, ohne echte Kryptografie zu brauchen
/// (dieselbe Testbarkeits-Begründung wie bei jedem anderen `core`-Trait
/// für eine austauschbare Implementierung, s. `docs/specs/0001-
/// architecture-overview.md`, Abschnitt 4).
pub trait ContentCipher: Send + Sync {
    fn encrypt(&self, plaintext: &str) -> Result<EncryptedContent, CipherError>;
    fn decrypt(&self, data: &EncryptedContent) -> Result<String, CipherError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypted_content_blob_round_trip() {
        let content = EncryptedContent {
            ciphertext: vec![1, 2, 3, 4, 5],
            nonce: [9; NONCE_LEN],
        };
        let blob = content.to_blob();
        assert_eq!(blob.len(), NONCE_LEN + 5);
        let parsed = EncryptedContent::from_blob(&blob).unwrap();
        assert_eq!(parsed, content);
    }

    #[test]
    fn test_from_blob_rejects_too_short_input() {
        let too_short = vec![1, 2, 3];
        assert_eq!(
            EncryptedContent::from_blob(&too_short),
            Err(CipherError::InvalidBlob)
        );
    }

    #[test]
    fn test_from_blob_accepts_nonce_only_empty_ciphertext() {
        let blob = vec![0u8; NONCE_LEN];
        let parsed = EncryptedContent::from_blob(&blob).unwrap();
        assert_eq!(parsed.ciphertext, Vec::<u8>::new());
    }
}
