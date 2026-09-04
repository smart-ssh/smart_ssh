//! [`ChaCha20Poly1305Cipher`] — die in Spec 0036, Abschnitt 3 vorgegebene
//! `ContentCipher`-Implementierung über die `chacha20poly1305`-Crate.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::ChaCha20Poly1305;

use super::{CipherError, ContentCipher, EncryptedContent};

/// AEAD-Verschlüsselung eines einzelnen Chat-Inhalts. Zustandslos bis auf
/// den Schlüssel selbst — jeder [`ContentCipher::encrypt`]-Aufruf erzeugt
/// einen frischen, zufälligen Nonce (Aufgabenstellung: "Nonce pro
/// Verschlüsselungsvorgang zufällig"), nie einen wiederverwendeten oder
/// vom Inhalt abgeleiteten (das würde bei gleichem Klartext zu gleichem
/// Chiffrat führen — genau das explizit geforderte Gegenteil, s.
/// Testfall "unterschiedliche Nonces bei wiederholter Verschlüsselung
/// desselben Inhalts").
pub struct ChaCha20Poly1305Cipher {
    cipher: ChaCha20Poly1305,
}

impl ChaCha20Poly1305Cipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(&(*key).into()),
        }
    }
}

impl ContentCipher for ChaCha20Poly1305Cipher {
    fn encrypt(&self, plaintext: &str) -> Result<EncryptedContent, CipherError> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| CipherError::EncryptionFailed)?;
        Ok(EncryptedContent {
            ciphertext,
            nonce: nonce.into(),
        })
    }

    fn decrypt(&self, data: &EncryptedContent) -> Result<String, CipherError> {
        let nonce = data.nonce.into();
        let plaintext_bytes = self
            .cipher
            .decrypt(&nonce, data.ciphertext.as_slice())
            .map_err(|_| CipherError::DecryptionFailed)?;
        String::from_utf8(plaintext_bytes).map_err(|_| CipherError::DecryptionFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn test_encrypt_then_decrypt_roundtrips_to_original_plaintext() {
        let cipher = ChaCha20Poly1305Cipher::new(&test_key());
        let plaintext = "geheime Nachricht mit Umlauten äöü";

        let encrypted = cipher.encrypt(plaintext).unwrap();
        let decrypted = cipher.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_empty_plaintext_roundtrips() {
        let cipher = ChaCha20Poly1305Cipher::new(&test_key());
        let encrypted = cipher.encrypt("").unwrap();
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), "");
    }

    /// Aufgabenstellung: "unterschiedliche Nonces bei wiederholter
    /// Verschlüsselung desselben Inhalts (kein deterministisches
    /// Chiffrat)".
    #[test]
    fn test_repeated_encryption_of_same_plaintext_uses_different_nonces_and_ciphertexts() {
        let cipher = ChaCha20Poly1305Cipher::new(&test_key());
        let plaintext = "immer derselbe Inhalt";

        let first = cipher.encrypt(plaintext).unwrap();
        let second = cipher.encrypt(plaintext).unwrap();

        assert_ne!(first.nonce, second.nonce, "Nonce muss pro Aufruf neu sein");
        assert_ne!(
            first.ciphertext, second.ciphertext,
            "gleicher Klartext darf nicht dasselbe Chiffrat ergeben"
        );
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails_cleanly_not_panics() {
        let encrypted = ChaCha20Poly1305Cipher::new(&test_key())
            .encrypt("geheim")
            .unwrap();
        let wrong_key_cipher = ChaCha20Poly1305Cipher::new(&[1u8; 32]);

        assert_eq!(
            wrong_key_cipher.decrypt(&encrypted),
            Err(CipherError::DecryptionFailed)
        );
    }

    /// Ein manipuliertes Chiffrat (z. B. ein Byte geändert, wie es ein
    /// Angreifer mit Dateizugriff könnte) muss die Poly1305-
    /// Authentifizierung erkennbar zum Scheitern bringen, nicht
    /// stillschweigend einen falschen Klartext liefern.
    #[test]
    fn test_decrypt_with_tampered_ciphertext_fails_cleanly() {
        let cipher = ChaCha20Poly1305Cipher::new(&test_key());
        let mut encrypted = cipher.encrypt("geheim").unwrap();
        let last = encrypted.ciphertext.len() - 1;
        encrypted.ciphertext[last] ^= 0xFF;

        assert_eq!(
            cipher.decrypt(&encrypted),
            Err(CipherError::DecryptionFailed)
        );
    }

    #[test]
    fn test_decrypt_with_wrong_nonce_fails_cleanly() {
        let cipher = ChaCha20Poly1305Cipher::new(&test_key());
        let mut encrypted = cipher.encrypt("geheim").unwrap();
        encrypted.nonce[0] ^= 0xFF;

        assert_eq!(
            cipher.decrypt(&encrypted),
            Err(CipherError::DecryptionFailed)
        );
    }
}
