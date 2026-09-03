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

/// Indiziert nicht mehr nur nach `(host, port)` mit einer anhängenden Liste
/// (die alte Form), sondern zusätzlich nach SSH-Algorithmus
/// ([`key_algorithm`]) — ein Schlüssel pro Host **und** Algorithmus, analog
/// zu einer echten `known_hosts`-Datei. Das ist die Grundlage dafür, dass
/// [`FileHostKeyStore::trust`] eine Rotation (neuer Key **desselben**
/// Algorithmus) korrekt ERSETZEN kann, statt den alten, ggf. kompromittierten
/// Key auf unbestimmte Zeit zusätzlich weiter zu vertrauen — mit einer
/// reinen `(host, port) -> Vec<Key>`-Liste (die vorherige Form) ließen sich
/// "neuer Key desselben Algorithmus" (Rotation, sollte ersetzen) und "Key
/// eines bislang unbekannten Algorithmus" (z. B. zusätzlich zu RSA nun auch
/// ED25519, sollte koexistieren) nicht unterscheiden (unabhängiger
/// Review-Pass, Spec 0005).
type HostPortAlgo = (String, u16, String);
type KnownHostKeys = HashMap<HostPortAlgo, Vec<u8>>;

/// Bewusst fester Sentinel statt eines vom Key-Inhalt abgeleiteten
/// Platzhalters: ein SSH-Public-Key-Blob beginnt laut RFC 4253 Abschnitt
/// 6.6 IMMER mit einem längenpräfigierten Algorithmus-Namen — dieser Zweig
/// greift daher in der Praxis nur bei einem defekten/manipulierten Blob,
/// nie bei einem echten, von `russh` gelieferten Host-Key. Ein fester statt
/// inhaltsabhängiger Sentinel erhält für genau diesen Pathologie-Fall
/// wenigstens das alte "ein Key ersetzt den anderen"-Verhalten, statt jeden
/// unparsbaren Key für immer als potenziell weiteren, nie ersetzbaren
/// Eintrag zu behandeln.
const UNPARSEABLE_ALGORITHM_SENTINEL: &str = "__unparseable__";

/// Extrahiert den SSH-Algorithmus-Bezeichner (z. B. `ssh-ed25519`,
/// `ssh-rsa`, `ecdsa-sha2-nistp256`) aus dem Anfang eines rohen
/// Public-Key-Blobs — das SSH-Drahtformat beginnt jeden Key mit genau
/// diesem längenpräfigierten String (RFC 4253, Abschnitt 6.6).
fn key_algorithm(raw_key: &[u8]) -> Option<String> {
    let len_bytes: [u8; 4] = raw_key.get(0..4)?.try_into().ok()?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let name_bytes = raw_key.get(4..4 + len)?;
    String::from_utf8(name_bytes.to_vec()).ok()
}

fn algorithm_key(raw_key: &[u8]) -> String {
    key_algorithm(raw_key).unwrap_or_else(|| UNPARSEABLE_ALGORITHM_SENTINEL.to_string())
}

pub struct FileHostKeyStore {
    path: PathBuf,
    known: Mutex<KnownHostKeys>,
}

impl FileHostKeyStore {
    /// Lädt (falls vorhanden) `path` synchron beim Start — bewusst nicht
    /// lazy, damit ein defektes/nicht lesbares File sofort beim App-Start
    /// auffällt statt erst beim ersten `connect()`. Das Dateiformat selbst
    /// bleibt unverändert (weiterhin eine flache `Vec<StoredEntry>`, kein
    /// serialisiertes Algorithmus-Feld) — der Algorithmus wird beim Laden
    /// aus `raw_key` neu abgeleitet ([`algorithm_key`]), damit bestehende
    /// `host_keys.json`-Dateien ohne Migration weiter lesbar bleiben.
    /// Enthält eine Datei aus der Zeit vor diesem Fix für denselben
    /// (Host, Port, Algorithmus) mehrere Einträge (genau das war der Bug —
    /// ein "Trust anyway" auf einen `Mismatch` hängte den neuen Key nur an,
    /// statt den alten zu ersetzen), gewinnt beim Laden einer davon
    /// (Reihenfolge nicht garantiert) — ein Selbstheilungseffekt, strikt
    /// nicht schlechter als der vorherige Zustand, in dem alle dauerhaft
    /// vertraut blieben.
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let known = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("Host-Key-Datei {path:?} konnte nicht gelesen werden: {e}"))?;
            let entries: Vec<StoredEntry> = serde_json::from_str(&raw)
                .map_err(|e| format!("Host-Key-Datei {path:?} ist kein gültiges JSON: {e}"))?;
            let mut map: KnownHostKeys = HashMap::new();
            for e in entries {
                let algo = algorithm_key(&e.raw_key);
                map.insert((e.host, e.port, algo), e.raw_key);
            }
            map
        } else {
            HashMap::new()
        };

        Ok(Self {
            path,
            known: Mutex::new(known),
        })
    }

    fn persist(&self, known: &KnownHostKeys) -> Result<(), SshError> {
        let mut entries = Vec::new();
        for ((host, port, _algorithm), raw_key) in known {
            entries.push(StoredEntry {
                host: host.clone(),
                port: *port,
                raw_key: raw_key.clone(),
            });
        }
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
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp_path, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn fingerprint(raw_key: &[u8]) -> String {
    let digest = Sha256::digest(raw_key);
    format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
    )
}

impl HostKeyStore for FileHostKeyStore {
    /// Schlägt gezielt für `(host, port, Algorithmus von `key`)` nach — ein
    /// bislang für diesen Host nur in einem ANDEREN Algorithmus bekannter
    /// Key (z. B. RSA bekannt, Server bietet zusätzlich ED25519 an) ist
    /// dadurch korrekt `Unknown` (neuer, bislang ungesehener Algorithmus),
    /// nicht fälschlich `Mismatch` — vorher wurde jeder für diesen Host
    /// gespeicherte Key gemeinsam betrachtet, unabhängig vom Algorithmus.
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        let known = self.known.lock().unwrap();
        let algo = algorithm_key(key);
        match known.get(&(host.to_string(), port, algo)) {
            None => HostKeyDecision::Unknown {
                fingerprint: fingerprint(key),
            },
            Some(stored_key) if stored_key.as_slice() == key => HostKeyDecision::Trusted,
            Some(stored_key) => HostKeyDecision::Mismatch {
                expected_fingerprint: fingerprint(stored_key),
                actual_fingerprint: fingerprint(key),
            },
        }
    }

    /// ERSETZT (statt anzuhängen) den gespeicherten Key für `(host, port,
    /// Algorithmus von `key`)`. Das ist der eigentliche Fix des
    /// unabhängigen Review-Passes: bestätigt der Nutzer einen `Mismatch`
    /// ("trotzdem vertrauen"), muss der alte — ggf. kompromittierte oder
    /// schlicht rotierte — Key für genau diesen Algorithmus verschwinden,
    /// nicht als zusätzlicher, für immer weiter gültiger Eintrag neben dem
    /// neuen stehen bleiben (das machte einen legitim rotierten Key
    /// dauerhaft MITM-fähig, ohne dass je wieder ein Dialog erscheint). Ein
    /// Key eines ANDEREN, bislang unbekannten Algorithmus wird weiterhin
    /// einfach zusätzlich gespeichert (Koexistenz mehrerer Algorithmen
    /// bleibt erhalten, s. `key_algorithm`-Doc-Kommentar).
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        let mut known = self.known.lock().unwrap();
        let algo = algorithm_key(key);
        known.insert((host.to_string(), port, algo), key.to_vec());
        self.persist(&known)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut einen SSH-Wire-Format-artigen Key (RFC 4253, Abschnitt 6.6:
    /// längenpräfigierter Algorithmus-Name, gefolgt vom Rest des Keys) —
    /// die vorherigen Tests nutzten frei erfundene Byte-Strings ohne dieses
    /// Präfix, was `key_algorithm` zuverlässig scheitern ließ und dadurch
    /// jeden Test-Key in denselben `UNPARSEABLE_ALGORITHM_SENTINEL`-Eimer
    /// fallen ließ — für Tests, die gezielt UNTERSCHIEDLICHE Algorithmen
    /// desselben Hosts prüfen wollen, reicht das nicht mehr.
    fn fake_key(algorithm: &str, distinguishing_suffix: &str) -> Vec<u8> {
        let algo_bytes = algorithm.as_bytes();
        let mut key = (algo_bytes.len() as u32).to_be_bytes().to_vec();
        key.extend_from_slice(algo_bytes);
        key.extend_from_slice(distinguishing_suffix.as_bytes());
        key
    }

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
    fn test_multiple_algorithms_per_host_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();
        let ed25519_key = fake_key("ssh-ed25519", "aaa");
        let rsa_key = fake_key("ssh-rsa", "bbb");

        store.trust("example.invalid", 22, &ed25519_key).unwrap();
        store.trust("example.invalid", 22, &rsa_key).unwrap();

        assert_eq!(
            store.check("example.invalid", 22, &ed25519_key),
            HostKeyDecision::Trusted
        );
        assert_eq!(
            store.check("example.invalid", 22, &rsa_key),
            HostKeyDecision::Trusted
        );
    }

    /// Regressionstest für den unabhängigen Review-Pass (Spec 0005): ein
    /// bislang für diesen Host unbekannter ALGORITHMUS (der Server bietet
    /// z. B. zusätzlich zu bereits vertrautem RSA nun auch ED25519 an) ist
    /// `Unknown` (ein neuer, noch nicht gesehener Eintrag), nicht fälschlich
    /// `Mismatch` — vorher wurden alle für einen Host gespeicherten Keys
    /// gemeinsam betrachtet, unabhängig vom Algorithmus, was echte Trust-
    /// Erweiterungen wie eine echte MITM-Warnung aussehen ließ.
    #[test]
    fn test_new_algorithm_for_known_host_is_unknown_not_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();
        let rsa_key = fake_key("ssh-rsa", "bbb");
        store.trust("example.invalid", 22, &rsa_key).unwrap();

        let ed25519_key = fake_key("ssh-ed25519", "aaa");
        let decision = store.check("example.invalid", 22, &ed25519_key);

        assert!(
            matches!(decision, HostKeyDecision::Unknown { .. }),
            "erwartet Unknown (neuer Algorithmus), bekam {decision:?}"
        );
    }

    /// **Der eigentliche Fix des unabhängigen Review-Passes** (Spec 0005):
    /// bestätigt der Nutzer einen `Mismatch` ("trotzdem vertrauen"), muss
    /// der ALTE Key danach nicht mehr vertraut sein. Vorher blieb er als
    /// zusätzlicher Eintrag neben dem neuen bestehen — ein Angreifer mit
    /// dem alten (z. B. geleakten, gerade deshalb rotierten) Key konnte
    /// dadurch nach der Rotation weiterhin unbemerkt MITMen, ganz ohne
    /// erneuten Dialog.
    #[test]
    fn test_trusting_a_mismatched_key_replaces_the_old_one_not_appends() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();
        let old_key = fake_key("ssh-ed25519", "old");
        let new_key = fake_key("ssh-ed25519", "new");

        store.trust("example.invalid", 22, &old_key).unwrap();
        assert!(matches!(
            store.check("example.invalid", 22, &new_key),
            HostKeyDecision::Mismatch { .. }
        ));

        // Nutzer bestätigt trotz Mismatch-Warnung ("Trust anyway").
        store.trust("example.invalid", 22, &new_key).unwrap();

        assert_eq!(
            store.check("example.invalid", 22, &new_key),
            HostKeyDecision::Trusted
        );
        // Der ALTE Key darf danach nicht mehr vertraut sein.
        assert!(
            matches!(
                store.check("example.invalid", 22, &old_key),
                HostKeyDecision::Mismatch { .. }
            ),
            "der alte Key darf nach dem Vertrauen des neuen nicht mehr als Trusted gelten"
        );
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

    #[cfg(unix)]
    #[test]
    fn test_t10_posix_permissions_enforced() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("host_keys.json");
        let store = FileHostKeyStore::load(path.clone()).unwrap();
        store.trust("example.invalid", 22, b"key1").unwrap();

        let file_meta = std::fs::metadata(&path).unwrap();
        assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);

        let parent_meta = std::fs::metadata(path.parent().unwrap()).unwrap();
        assert_eq!(parent_meta.permissions().mode() & 0o777, 0o700);
    }
}
