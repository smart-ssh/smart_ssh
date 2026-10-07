//! Lesen der früheren feldweisen Verschlüsselung (Spec 0036) — **nur für die
//! einmalige Umstellung** (Issue #113, ADR 0112).
//!
//! Bis Issue #113 lagen Chat-Nachrichten, Ledger-Einträge, Prompt-Historie
//! und Zusammenfassungen zusätzlich zur verschlüsselten Datenbankdatei (Spec
//! 0101) feldweise unter ChaCha20-Poly1305 mit dem Wurzelschlüssel K in der
//! Datenbank, je Zeile als `nonce (12 Byte) || ciphertext`. Seitdem schreibt
//! kein Store mehr so; die Umstellung beim Start entschlüsselt die
//! vorhandenen Zeilen einmal und schreibt Klartext zurück — innerhalb der
//! verschlüsselten Datei.
//!
//! **Bewusst nur Lesen.** Es gibt keinen Produktivweg mehr, der feldweise
//! verschlüsselt. Den Schreibweg gibt es ausschließlich für Tests
//! ([`encrypt_for_tests`]), die Altzeilen erzeugen müssen — hinter
//! `cfg(any(test, feature = "test-support"))`, damit der Schritt
//! `cargo build --workspace` des Gates einen Produktivaufruf abfängt.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;

use super::CipherError;

/// Länge des Nonce-Präfixes jeder Altzeile (96 Bit, RFC 8439).
pub const LEGACY_NONCE_LEN: usize = 12;

/// Entschlüsselt eine Altzeile `nonce || ciphertext` mit K.
///
/// - [`CipherError::InvalidBlob`], wenn der Blob nicht einmal den Nonce
///   enthält — nie ein Index-Panic auf zu kurzen Daten.
/// - [`CipherError::DecryptionFailed`] bei falschem Schlüssel,
///   manipuliertem Chiffrat oder einem Klartext, der kein UTF-8 ist. Die
///   ersten beiden sind beim AEAD-Verfahren absichtlich nicht
///   unterscheidbar.
pub fn decrypt_legacy_field_content(
    root_key: &[u8; 32],
    blob: &[u8],
) -> Result<String, CipherError> {
    if blob.len() < LEGACY_NONCE_LEN {
        return Err(CipherError::InvalidBlob);
    }
    let (nonce_bytes, ciphertext) = blob.split_at(LEGACY_NONCE_LEN);
    let mut nonce = [0u8; LEGACY_NONCE_LEN];
    nonce.copy_from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(&(*root_key).into());
    let plaintext = cipher
        .decrypt(&nonce.into(), ciphertext)
        .map_err(|_| CipherError::DecryptionFailed)?;
    String::from_utf8(plaintext).map_err(|_| CipherError::DecryptionFailed)
}

/// Erzeugt eine Altzeile, wie die Stores sie bis Issue #113 geschrieben
/// haben — **nur für Tests** (Fixture-Zeilen für die Umstellung).
#[cfg(any(test, feature = "test-support"))]
pub fn encrypt_for_tests(root_key: &[u8; 32], plaintext: &str) -> Vec<u8> {
    use chacha20poly1305::aead::{AeadCore, OsRng};
    let cipher = ChaCha20Poly1305::new(&(*root_key).into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("ChaCha20-Poly1305 encryption with a 32-byte key cannot fail");
    let mut blob = Vec::with_capacity(LEGACY_NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    blob
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];

    #[test]
    fn test_a_legacy_row_decrypts_to_its_plaintext() {
        let blob = encrypt_for_tests(&KEY, "geheime Nachricht mit Umlauten äöü");
        assert_eq!(
            decrypt_legacy_field_content(&KEY, &blob).unwrap(),
            "geheime Nachricht mit Umlauten äöü"
        );
    }

    #[test]
    fn test_an_empty_legacy_row_decrypts() {
        let blob = encrypt_for_tests(&KEY, "");
        assert_eq!(decrypt_legacy_field_content(&KEY, &blob).unwrap(), "");
    }

    #[test]
    fn test_a_row_under_another_key_fails_cleanly() {
        let blob = encrypt_for_tests(&KEY, "geheim");
        assert_eq!(
            decrypt_legacy_field_content(&[1u8; 32], &blob),
            Err(CipherError::DecryptionFailed)
        );
    }

    #[test]
    fn test_a_tampered_row_fails_cleanly() {
        let mut blob = encrypt_for_tests(&KEY, "geheim");
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert_eq!(
            decrypt_legacy_field_content(&KEY, &blob),
            Err(CipherError::DecryptionFailed)
        );
    }

    #[test]
    fn test_a_too_short_row_is_rejected_without_panicking() {
        assert_eq!(
            decrypt_legacy_field_content(&KEY, &[1, 2, 3]),
            Err(CipherError::InvalidBlob)
        );
    }

    #[test]
    fn test_plaintext_bytes_are_not_mistaken_for_a_legacy_row() {
        assert_eq!(
            decrypt_legacy_field_content(&KEY, b"just some plaintext prompt"),
            Err(CipherError::DecryptionFailed)
        );
    }
}
