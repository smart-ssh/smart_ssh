//! Schritt-für-Schritt-Protokoll eines Verbindungsversuchs (Issue #51).
//!
//! Der Transport (`ssh-transport`) trägt je Hop ein, was er gerade tut —
//! DNS, TCP, Handshake, Host-Key-Prüfung, Anmeldung — mit Dauer und
//! Ergebnis. Die Oberfläche zeigt das zugeklappt unter dem Testergebnis bzw.
//! unter dem Verbindungsfehler.
//!
//! **Nur für die Oberfläche, flüchtig.** Nichts hiervon geht in die
//! Log-Datei oder in den Diagnose-Export: Kein Typ hier ruft `tracing`, und
//! kein Aufrufer darf ein [`ConnectStepRecord`] loggen. Hostnamen,
//! Benutzernamen und Fingerprints sind Daten des Nutzers und in seiner
//! eigenen Oberfläche in Ordnung, in einer weitergegebenen Datei nicht
//! (Spec 0094, Spec 0063).
//!
//! **Strukturiert, nie freier Bibliothekstext.** Jeder Schritt ist eine
//! Variante mit festen, ungefährlichen Parametern; ein Fehlschlag trägt nur
//! einen stabilen Code ([`super::SshError::code`] oder einen der
//! `HOST_KEY_*`-Codes unten), nie die `Display`-Meldung einer Bibliothek.
//! Geheimnisse (Passwort, Passphrase, Schlüssel, Zertifikat) haben hier
//! strukturell keinen Platz: [`AuthMethodKind`] nennt nur die Art.
//!
//! **Beobachten, nie entscheiden.** Das Protokoll ist reine Aufzeichnung;
//! keine Vertrauens- oder Anmeldeentscheidung hängt an ihm.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;

use crate::profiles::AuthMethod;

/// Fehlercode eines Host-Key-Schritts mit unbekanntem Schlüssel. Kein
/// `SshError`: Ein unbekannter Schlüssel ist kein Fehler des Transports,
/// sondern wartet auf die Entscheidung des Nutzers.
pub const HOST_KEY_UNKNOWN: &str = "HOST_KEY_UNKNOWN";
/// Fehlercode eines Host-Key-Schritts mit geändertem Schlüssel.
pub const HOST_KEY_CHANGED: &str = "HOST_KEY_CHANGED";

/// Obergrenze für die angezeigte Versionskennung des Servers. RFC 4253
/// erlaubt 255 Zeichen einschließlich CR LF.
const MAX_SERVER_VERSION_CHARS: usize = 255;

/// Ein Schritt eines Verbindungsversuchs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConnectStep {
    /// Namensauflösung des ersten Hops. `addresses` ist leer, solange sie
    /// läuft oder wenn sie gescheitert ist.
    DnsResolution {
        host: String,
        port: u16,
        addresses: Vec<String>,
    },
    /// TCP-Verbindung des ersten Hops. `address` ist die tatsächlich
    /// erreichte Gegenstelle, sobald die Verbindung steht.
    TcpConnect { address: Option<String>, port: u16 },
    /// Hops ab dem zweiten: Kanal `direct-tcpip` über den vorherigen Hop.
    /// Namensauflösung und TCP-Verbindung macht dort der Jump-Host, nicht
    /// dieser Rechner — deshalb kein DNS-/TCP-Schritt für diese Hops.
    TunnelOpen { host: String, port: u16 },
    /// SSH-Handshake: Versionskennung des Servers und ausgehandelte
    /// Algorithmen.
    Handshake {
        server_version: Option<String>,
        kex: Option<String>,
        host_key_algorithm: Option<String>,
        cipher: Option<String>,
        mac: Option<String>,
    },
    /// Prüfung des Host-Keys gegen die bekannten Schlüssel.
    HostKeyCheck {
        key_type: Option<String>,
        fingerprint: Option<String>,
        result: Option<HostKeyCheckResult>,
    },
    /// Eine Anmeldung mit der konfigurierten Methode. `remaining_methods`
    /// nennt, was der Server nach einem Fehlschlag noch anbietet — **nur zur
    /// Anzeige**, es wird keine weitere Methode versucht.
    Authentication {
        method: AuthMethodKind,
        remaining_methods: Vec<String>,
        partial_success: bool,
    },
    /// Die SSH-Sitzung zum Ziel steht (letzter Hop angemeldet).
    SessionReady,
}

/// Ergebnis der Host-Key-Prüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostKeyCheckResult {
    Known,
    Unknown,
    Changed,
}

/// Art der Anmeldung — ohne jedes Material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethodKind {
    Password,
    PrivateKey,
    Agent,
    Certificate,
    IdentityFile,
}

impl From<&AuthMethod> for AuthMethodKind {
    fn from(auth: &AuthMethod) -> Self {
        match auth {
            AuthMethod::Password { .. } => AuthMethodKind::Password,
            AuthMethod::PrivateKey { .. } => AuthMethodKind::PrivateKey,
            AuthMethod::Agent => AuthMethodKind::Agent,
            AuthMethod::Certificate { .. } => AuthMethodKind::Certificate,
            AuthMethod::IdentityFile { .. } => AuthMethodKind::IdentityFile,
        }
    }
}

/// Stand eines Schritts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StepStatus {
    /// Läuft noch — nur in einer Momentaufnahme mitten im Versuch zu sehen.
    Running,
    Ok,
    /// Hier ist der Versuch gescheitert. `code` ist ein stabiler Code, nie
    /// freier Text.
    Failed {
        code: String,
    },
}

/// Ein aufgezeichneter Schritt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectStepRecord {
    /// 0 = erster Hop der Kette.
    pub hop_index: usize,
    /// `user@host:port` des Hops.
    pub hop: String,
    pub step: ConnectStep,
    pub status: StepStatus,
    /// Dauer in Millisekunden, sobald der Schritt beendet ist.
    pub duration_ms: Option<u64>,
}

/// Verweis auf einen begonnenen Schritt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepId(usize);

struct Entry {
    record: ConnectStepRecord,
    started: Instant,
}

/// Geteilte Aufzeichnung eines Verbindungsversuchs.
///
/// `Clone` teilt denselben Speicher: Der Transport schreibt hinein (auch aus
/// dem `russh`-Handler, der in einen eigenen Task wandert), der Aufrufer
/// liest nach dem Versuch — auch dann, wenn ein Timeout den Versuch mitten
/// im Schritt abgebrochen hat.
#[derive(Clone, Default)]
pub struct ConnectLog {
    entries: Arc<Mutex<Vec<Entry>>>,
}

impl std::fmt::Debug for ConnectLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectLog")
            .field("steps", &self.lock().len())
            .finish()
    }
}

impl ConnectLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Eine vergiftete Sperre ist hier kein Schutzproblem — das Protokoll
    /// ist reine Anzeige. Weiterarbeiten mit dem Inhalt statt eines Panics
    /// mitten im Verbindungsaufbau.
    fn lock(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Beginnt einen Schritt.
    pub fn start(&self, hop_index: usize, hop: &str, step: ConnectStep) -> StepId {
        let mut entries = self.lock();
        entries.push(Entry {
            record: ConnectStepRecord {
                hop_index,
                hop: hop.to_string(),
                step,
                status: StepStatus::Running,
                duration_ms: None,
            },
            started: Instant::now(),
        });
        StepId(entries.len() - 1)
    }

    /// Ergänzt die Parameter eines Schritts (z. B. aufgelöste Adressen).
    pub fn update(&self, id: StepId, f: impl FnOnce(&mut ConnectStep)) {
        if let Some(entry) = self.lock().get_mut(id.0) {
            f(&mut entry.record.step);
        }
    }

    /// Beendet einen Schritt erfolgreich.
    pub fn succeed(&self, id: StepId) {
        self.finish(id, StepStatus::Ok);
    }

    /// Beendet einen Schritt als gescheitert.
    pub fn fail(&self, id: StepId, code: &str) {
        self.finish(
            id,
            StepStatus::Failed {
                code: code.to_string(),
            },
        );
    }

    fn finish(&self, id: StepId, status: StepStatus) {
        if let Some(entry) = self.lock().get_mut(id.0) {
            if entry.record.status == StepStatus::Running {
                entry.record.status = status;
                entry.record.duration_ms = Some(millis(entry.started));
            }
        }
    }

    /// Markiert jeden noch laufenden Schritt als gescheitert — für Fehler,
    /// die mitten in einem Schritt auftreten (`?` im Transport) und für den
    /// Timeout, der den Versuch von außen abbricht.
    pub fn fail_running(&self, code: &str) {
        for entry in self.lock().iter_mut() {
            if entry.record.status == StepStatus::Running {
                entry.record.status = StepStatus::Failed {
                    code: code.to_string(),
                };
                entry.record.duration_ms = Some(millis(entry.started));
            }
        }
    }

    /// Momentaufnahme aller Schritte in Reihenfolge.
    pub fn snapshot(&self) -> Vec<ConnectStepRecord> {
        self.lock().iter().map(|e| e.record.clone()).collect()
    }
}

fn millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Bereitet die Versionskennung des Servers zur Anzeige auf: nur druckbares
/// ASCII (RFC 4253, 4.2), gekürzt. Der Text kommt vom Server und ist damit
/// nicht vertrauenswürdig — Steuerzeichen haben in der Anzeige nichts zu
/// suchen.
pub fn sanitize_server_version(raw: &[u8]) -> String {
    raw.iter()
        .map(|&b| b as char)
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(MAX_SERVER_VERSION_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::CredentialRef;

    #[test]
    fn steps_finish_once_and_keep_their_order() {
        let log = ConnectLog::new();
        let dns = log.start(
            0,
            "u@h:22",
            ConnectStep::DnsResolution {
                host: "h".into(),
                port: 22,
                addresses: vec![],
            },
        );
        log.update(dns, |step| {
            if let ConnectStep::DnsResolution { addresses, .. } = step {
                addresses.push("192.0.2.1".into());
            }
        });
        log.succeed(dns);
        // Ein zweites Beenden ändert nichts mehr.
        log.fail(dns, "SSH_TIMEOUT");
        let tcp = log.start(
            0,
            "u@h:22",
            ConnectStep::TcpConnect {
                address: None,
                port: 22,
            },
        );
        log.fail(tcp, "SSH_CONNECTION_REFUSED");

        let steps = log.snapshot();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].status, StepStatus::Ok);
        assert!(steps[0].duration_ms.is_some());
        assert_eq!(
            steps[0].step,
            ConnectStep::DnsResolution {
                host: "h".into(),
                port: 22,
                addresses: vec!["192.0.2.1".into()],
            }
        );
        assert_eq!(
            steps[1].status,
            StepStatus::Failed {
                code: "SSH_CONNECTION_REFUSED".into()
            }
        );
    }

    #[test]
    fn fail_running_marks_only_unfinished_steps() {
        let log = ConnectLog::new();
        let a = log.start(0, "u@h:22", ConnectStep::SessionReady);
        log.succeed(a);
        log.start(
            0,
            "u@h:22",
            ConnectStep::Handshake {
                server_version: None,
                kex: None,
                host_key_algorithm: None,
                cipher: None,
                mac: None,
            },
        );
        log.fail_running("SSH_TIMEOUT");
        let steps = log.snapshot();
        assert_eq!(steps[0].status, StepStatus::Ok);
        assert_eq!(
            steps[1].status,
            StepStatus::Failed {
                code: "SSH_TIMEOUT".into()
            }
        );
        assert!(steps[1].duration_ms.is_some());
    }

    #[test]
    fn clones_share_the_same_record() {
        let log = ConnectLog::new();
        let writer = log.clone();
        writer.start(1, "u@jump:22", ConnectStep::SessionReady);
        assert_eq!(log.snapshot().len(), 1);
        assert_eq!(log.snapshot()[0].hop_index, 1);
    }

    #[test]
    fn auth_method_kind_names_only_the_kind() {
        let r = CredentialRef::new("secret-ref");
        let cases = [
            (
                AuthMethod::Password {
                    credential_ref: r.clone(),
                },
                AuthMethodKind::Password,
            ),
            (
                AuthMethod::PrivateKey {
                    credential_ref: r.clone(),
                    passphrase_ref: Some(r.clone()),
                },
                AuthMethodKind::PrivateKey,
            ),
            (AuthMethod::Agent, AuthMethodKind::Agent),
            (
                AuthMethod::Certificate {
                    cert_ref: r.clone(),
                    key_ref: r.clone(),
                },
                AuthMethodKind::Certificate,
            ),
            (
                AuthMethod::IdentityFile {
                    path: "~/.ssh/id".into(),
                    passphrase_ref: Some(r.clone()),
                },
                AuthMethodKind::IdentityFile,
            ),
        ];
        for (auth, expected) in cases {
            assert_eq!(AuthMethodKind::from(&auth), expected);
        }
    }

    #[test]
    fn server_version_is_reduced_to_printable_ascii_and_capped() {
        assert_eq!(
            sanitize_server_version(b"SSH-2.0-OpenSSH_9.6\r\n"),
            "SSH-2.0-OpenSSH_9.6"
        );
        assert_eq!(
            sanitize_server_version(b"SSH-2.0-x\x1b[31mred\x07"),
            "SSH-2.0-x[31mred"
        );
        assert_eq!(sanitize_server_version(&[b'a'; 1000]).len(), 255);
    }
}
