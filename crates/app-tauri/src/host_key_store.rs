//! Persistente [`HostKeyStore`]-Implementierung (Spec 0005, Abschnitt 6).
//!
//! **Datei statt SQLite-Tabelle.** `HostKeyStore`s Trait-Dokumentation
//! nennt beide Optionen ausdrücklich als gleichwertig ("eigene Tabelle in
//! `persistence-sqlite` oder `known_hosts`-Datei"). Der Trait ist bewusst
//! **synchron** (`fn check`/`fn trust`, kein `async_trait`) — `check()`
//! wird aus `russh`s `check_server_key`-Callback heraus aufgerufen (s.
//! `ssh-transport::host_key::evaluate_host_key`), der selbst innerhalb
//! eines von `russh` gespawnten Tokio-Tasks läuft. Ein `sqlx`-Zugriff
//! (async-only) hätte dort entweder `tokio::task::block_in_place` +
//! `Handle::block_on` gebraucht — funktioniert nur auf einer
//! Multi-Thread-Runtime und bricht bei jeder künftigen Änderung der
//! Runtime-Konfiguration lautlos wieder ab — oder einen fire-and-forget
//! Hintergrund-Kanal für Schreibzugriffe. Eine einfache JSON-Datei mit
//! synchronem `std::fs`-I/O umgeht dieses Problem vollständig: kein
//! Async-Runtime-Zugriff nötig, keine Blocking-in-Runtime-Fallstricke,
//! bei der hier zu erwartenden Schreibfrequenz (ein `trust()`-Aufruf pro
//! neu gesehenem Host) auch performant genug.
//!
//! Für Fingerprints wird echtes SHA-256 (statt eines rohen Hex-Dumps des
//! kompletten Public Keys) im OpenSSH-üblichen Format `SHA256:<base64>`
//! verwendet — das ist die Darstellung, die Nutzer typischerweise von
//! `ssh-keygen -lf`/dem OpenSSH-Client kennen und mit einer anderen Quelle
//! (z. B. dem Server-Betreiber) abgleichen können.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use ssh_manager_core::ssh::{HostKeyDecision, HostKeyStore, SshError};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    host: String,
    port: u16,
    #[serde(with = "hex_bytes")]
    raw_key: Vec<u8>,
}

/// Minimalistische Hex-(De-)Serialisierung für `raw_key` — JSON kennt keine
/// Byte-Strings, ein `Vec<u8>` würde sonst als JSON-Zahlenarray landen
/// (unnötig groß, unlesbar).
mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        hex.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let hex = String::deserialize(d)?;
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect()
    }
}

pub struct FileHostKeyStore {
    path: PathBuf,
    known: Mutex<HashMap<(String, u16), Vec<u8>>>,
}

impl FileHostKeyStore {
    /// Lädt (falls vorhanden) `path` synchron beim Start — bewusst nicht
    /// lazy, damit ein defektes/nicht lesbares File sofort beim App-Start
    /// auffällt statt erst beim ersten `connect()`.
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let known = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("Host-Key-Datei {path:?} konnte nicht gelesen werden: {e}"))?;
            let entries: Vec<StoredEntry> = serde_json::from_str(&raw)
                .map_err(|e| format!("Host-Key-Datei {path:?} ist kein gültiges JSON: {e}"))?;
            entries
                .into_iter()
                .map(|e| ((e.host, e.port), e.raw_key))
                .collect()
        } else {
            HashMap::new()
        };

        Ok(Self {
            path,
            known: Mutex::new(known),
        })
    }

    fn persist(&self, known: &HashMap<(String, u16), Vec<u8>>) -> Result<(), SshError> {
        let entries: Vec<StoredEntry> = known
            .iter()
            .map(|((host, port), raw_key)| StoredEntry {
                host: host.clone(),
                port: *port,
                raw_key: raw_key.clone(),
            })
            .collect();
        let json = serde_json::to_string_pretty(&entries)
            .map_err(|e| SshError::ChannelError(format!("Host-Keys nicht serialisierbar: {e}")))?;
        write_atomically(&self.path, &json)
            .map_err(|e| SshError::ChannelError(format!("Host-Keys nicht speicherbar: {e}")))
    }
}

/// Schreibt über eine temporäre Datei + `rename` statt direkt — ein Absturz
/// mitten im Schreiben darf die bestehende, gültige Datei nicht durch eine
/// halb geschriebene ersetzen (Trust-on-First-Use-Daten sind sicherheits-
/// relevant, ein kaputtes File darf nicht stillschweigend als "kein
/// bekannter Host" interpretiert werden).
fn write_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, contents)?;
    std::fs::rename(&tmp_path, path)
}

fn fingerprint(raw_key: &[u8]) -> String {
    let digest = Sha256::digest(raw_key);
    format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
    )
}

impl HostKeyStore for FileHostKeyStore {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        let known = self.known.lock().unwrap();
        match known.get(&(host.to_string(), port)) {
            None => HostKeyDecision::Unknown {
                fingerprint: fingerprint(key),
            },
            Some(stored) if stored.as_slice() == key => HostKeyDecision::Trusted,
            Some(stored) => HostKeyDecision::Mismatch {
                expected_fingerprint: fingerprint(stored),
                actual_fingerprint: fingerprint(key),
            },
        }
    }

    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        let mut known = self.known.lock().unwrap();
        known.insert((host.to_string(), port), key.to_vec());
        self.persist(&known)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_host_yields_unknown_decision() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();

        let decision = store.check("example.invalid", 22, b"raw-key-bytes");

        assert!(matches!(decision, HostKeyDecision::Unknown { .. }));
    }

    #[test]
    fn test_trusted_host_yields_trusted_decision_for_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();

        store
            .trust("example.invalid", 22, b"raw-key-bytes")
            .unwrap();
        let decision = store.check("example.invalid", 22, b"raw-key-bytes");

        assert_eq!(decision, HostKeyDecision::Trusted);
    }

    #[test]
    fn test_changed_key_yields_mismatch_decision() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();

        store.trust("example.invalid", 22, b"old-key").unwrap();
        let decision = store.check("example.invalid", 22, b"new-key");

        assert!(matches!(decision, HostKeyDecision::Mismatch { .. }));
    }

    #[test]
    fn test_trust_persists_across_store_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host_keys.json");

        let store = FileHostKeyStore::load(path.clone()).unwrap();
        store
            .trust("example.invalid", 22, b"raw-key-bytes")
            .unwrap();
        drop(store);

        let reloaded = FileHostKeyStore::load(path).unwrap();
        let decision = reloaded.check("example.invalid", 22, b"raw-key-bytes");

        assert_eq!(decision, HostKeyDecision::Trusted);
    }
}
