//! Frisch erzeugte OpenSSH-Schlüssel für Tests (Spec 0076, §6.2/§6.4.2).
//!
//! **Erzeugt, nicht eingebettet.** Dieses Repo ist öffentlich; ein
//! eingebetteter privater Schlüssel — und sei er ein Wegwerfschlüssel —
//! wäre Schlüsselmaterial im Klartext in der Versionsgeschichte. Jeder
//! Aufruf liefert stattdessen einen neuen Schlüssel, der nur so lange
//! lebt wie der Testlauf.
//!
//! Liegt in dieser Crate, weil hier die Schlüssel-Bibliothek schon wohnt
//! (`russh::keys`, s. `crate::auth`). `app-shell` aktiviert das Feature
//! `test-support` unter `[dev-dependencies]`, es landet also nie in einem
//! Release-Build.

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};

/// Ein gültiger, **unverschlüsselter** OpenSSH-Privatschlüssel als Text —
/// genau die Form, die in einer Schlüsseldatei steht.
pub fn unencrypted_private_key() -> String {
    generate().to_openssh(LineEnding::LF).unwrap().to_string()
}

/// Derselbe Schlüssel, aber **passphrase-geschützt**. Für A-5: Ein
/// verschlüsselter Schlüssel ist eine Feststellung, kein Fehler.
pub fn encrypted_private_key(passphrase: &str) -> String {
    generate()
        .encrypt(&mut rand::rng(), passphrase.as_bytes())
        .expect("Testschlüssel lässt sich verschlüsseln")
        .to_openssh(LineEnding::LF)
        .unwrap()
        .to_string()
}

/// Der **öffentliche** Teil eines frischen Schlüssels — für §6.4.2: Eine
/// Datei mit einem öffentlichen statt einem privaten Schlüssel muss „kein
/// gültiger Schlüssel" ergeben, nicht etwa durchgehen.
pub fn public_key() -> String {
    generate()
        .public_key()
        .to_openssh()
        .expect("öffentlicher Testschlüssel lässt sich schreiben")
}

fn generate() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .expect("Ed25519-Testschlüssel lässt sich erzeugen")
}
