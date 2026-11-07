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

use crate::sync::lock_tolerating_poison;
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

/// Schrittcode für eine Zeitüberschreitung während der TCP-Verbindung.
pub const TCP_TIMEOUT_CODE: &str = "SSH_TCP_TIMEOUT";

/// Ein Schritt eines Verbindungsversuchs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
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
    /// Ein Anmeldeversuch (bzw. bei `keyboard-interactive` eine Runde).
    /// `remaining_methods` nennt, was der Server nach einem Fehlschlag noch
    /// anbietet; weitere Schritte folgen nur nach Teilerfolg (Issue #1184).
    Authentication {
        method: AuthMethodKind,
        remaining_methods: Vec<String>,
        partial_success: bool,
        /// Issue #1184: `keyboard-interactive` only — number of the info
        /// request round (1-based).
        round: Option<u32>,
        /// Issue #1184: the prompt texts of that round (server-supplied,
        /// capped). The answers are never recorded.
        prompts: Vec<String>,
    },
    /// Die SSH-Sitzung zum Ziel steht (letzter Hop angemeldet).
    SessionReady,
    /// Issue #1173: this hop negotiated outdated algorithms (or no strict
    /// key exchange). Recorded with status [`StepStatus::Warning`]; the
    /// connection is not affected.
    OutdatedAlgorithms { algorithms: Vec<String> },
}

/// Issue #1173: one hop of an established connection that uses outdated
/// SSH algorithms. Algorithm names are not secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ConnectWarning {
    /// `user@host:port` of the affected hop.
    pub hop: String,
    /// Names of the outdated algorithms (the Terrapin warning is
    /// [`STRICT_KEX_MISSING`]).
    pub algorithms: Vec<String>,
}

/// Pseudo algorithm name for a server that does not support strict key
/// exchange (`kex-strict-s-v00@openssh.com`, Terrapin, CVE-2023-48795).
pub const STRICT_KEX_MISSING: &str = "kex-strict-s-v00@openssh.com (missing)";

/// Stable code of the warning status of an [`ConnectStep::OutdatedAlgorithms`]
/// step.
pub const OUTDATED_ALGORITHMS_CODE: &str = "SSH_OUTDATED_ALGORITHMS";

/// Ergebnis der Host-Key-Prüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum HostKeyCheckResult {
    Known,
    Unknown,
    Changed,
}

/// Art der Anmeldung — ohne jedes Material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "ts-export",
    derive(ts_rs::TS),
    ts(rename = "ConnectAuthMethodKind")
)]
pub enum AuthMethodKind {
    /// SSH-Methode `none` (Issue #632) — nur als erster Versuch einer
    /// Anmeldung ohne Passwort.
    None,
    Password,
    PrivateKey,
    Agent,
    Certificate,
    IdentityFile,
    /// Issue #1184: SSH method `keyboard-interactive`.
    KeyboardInteractive,
}

impl From<&AuthMethod> for AuthMethodKind {
    fn from(auth: &AuthMethod) -> Self {
        match auth {
            AuthMethod::Password { .. } | AuthMethod::NoPassword => AuthMethodKind::Password,
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
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum StepStatus {
    /// Läuft noch — nur in einer Momentaufnahme mitten im Versuch zu sehen.
    Running,
    Ok,
    /// Der Schritt war nicht nötig (z. B. keine Namensauflösung für eine
    /// IP-Adresse).
    Skipped,
    /// Issue #1173: the step succeeded but deserves the user's attention. It
    /// does not count as failed. `code` is a stable code, never free text.
    Warning {
        code: String,
    },
    /// Hier ist der Versuch gescheitert. `code` ist ein stabiler Code, nie
    /// freier Text.
    Failed {
        code: String,
    },
}

/// Ein aufgezeichneter Schritt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
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
    /// Issue #546: optional live sink, called synchronously with the full
    /// snapshot after every change. Shared by clones like `entries`.
    sink: Option<ProgressSink>,
    /// Issue #1187: shared by clones; the transport holds it while a login
    /// prompt is open, so every time limit around the attempt stands still.
    prompt_pause: Arc<PromptPause>,
}

/// Issue #1184/#1187: a flag that is held while the user is asked for a
/// login answer. Time limits watch it and stop counting while it is held.
pub struct PromptPause {
    tx: tokio::sync::watch::Sender<bool>,
}

impl Default for PromptPause {
    fn default() -> Self {
        Self {
            tx: tokio::sync::watch::channel(false).0,
        }
    }
}

impl PromptPause {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pauses until the returned guard is dropped (also when the waiting
    /// future is cancelled).
    pub fn pause(&self) -> PauseGuard<'_> {
        self.tx.send_replace(true);
        PauseGuard(self)
    }

    /// Watches the paused state.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<bool> {
        self.tx.subscribe()
    }
}

pub struct PauseGuard<'a>(&'a PromptPause);

impl Drop for PauseGuard<'_> {
    fn drop(&mut self) {
        self.0.tx.send_replace(false);
    }
}

/// Receives the full progress snapshot of an attempt whenever a step starts,
/// changes its detail, succeeds or fails (Issue #546).
pub type ProgressSink = Arc<dyn Fn(Vec<ConnectStepSnapshot>) + Send + Sync>;

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

    /// A log that reports every change to `sink` (Issue #546). The sink runs
    /// synchronously on the thread that changed the log and must not block
    /// or call back into this log.
    pub fn with_sink(sink: ProgressSink) -> Self {
        Self {
            entries: Arc::default(),
            sink: Some(sink),
            prompt_pause: Arc::default(),
        }
    }

    /// The pause shared with the transport (Issue #1187).
    pub fn prompt_pause(&self) -> Arc<PromptPause> {
        self.prompt_pause.clone()
    }

    /// Reports the current state to the sink, if any. Called after the lock
    /// is released.
    fn notify(&self) {
        if let Some(sink) = &self.sink {
            sink(self.progress_snapshot());
        }
    }

    /// Snapshot in the shape of the `connect-progress` event.
    pub fn progress_snapshot(&self) -> Vec<ConnectStepSnapshot> {
        ConnectStepSnapshot::from_records(&self.snapshot(), false)
    }

    /// Eine vergiftete Sperre ist hier kein Schutzproblem — das Protokoll
    /// ist reine Anzeige. Weiterarbeiten mit dem Inhalt statt eines Panics
    /// mitten im Verbindungsaufbau.
    fn lock(&self) -> MutexGuard<'_, Vec<Entry>> {
        lock_tolerating_poison(&self.entries)
    }

    /// Beginnt einen Schritt.
    pub fn start(&self, hop_index: usize, hop: &str, step: ConnectStep) -> StepId {
        let id = self.start_silent(hop_index, hop, step);
        self.notify();
        id
    }

    fn start_silent(&self, hop_index: usize, hop: &str, step: ConnectStep) -> StepId {
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
        self.notify();
    }

    /// Beendet einen Schritt erfolgreich.
    pub fn succeed(&self, id: StepId) {
        self.finish(id, StepStatus::Ok);
    }

    /// Beendet einen Schritt als übersprungen (nicht nötig).
    pub fn skip(&self, id: StepId) {
        self.finish(id, StepStatus::Skipped);
    }

    /// Issue #1173: records that `hop` negotiated outdated `algorithms`. The
    /// step is born finished with status warning; it never fails the attempt.
    pub fn warn_outdated(&self, hop_index: usize, hop: &str, algorithms: Vec<String>) {
        // One warning per hop: a later finding for the same hop (for
        // example the user key signature) extends the existing entry.
        let existing = {
            let mut entries = self.lock();
            entries
                .iter_mut()
                .find(|e| {
                    e.record.hop_index == hop_index
                        && matches!(e.record.step, ConnectStep::OutdatedAlgorithms { .. })
                })
                .map(|e| {
                    if let ConnectStep::OutdatedAlgorithms { algorithms: have } = &mut e.record.step
                    {
                        for a in &algorithms {
                            if !have.contains(a) {
                                have.push(a.clone());
                            }
                        }
                    }
                })
                .is_some()
        };
        if existing {
            self.notify();
            return;
        }
        let id = self.start_silent(
            hop_index,
            hop,
            ConnectStep::OutdatedAlgorithms { algorithms },
        );
        self.finish(
            id,
            StepStatus::Warning {
                code: OUTDATED_ALGORITHMS_CODE.to_string(),
            },
        );
    }

    /// Issue #1173: the outdated-algorithm warnings recorded so far, one per
    /// affected hop, in hop order of recording.
    pub fn warnings(&self) -> Vec<ConnectWarning> {
        self.lock()
            .iter()
            .filter_map(|e| match &e.record.step {
                ConnectStep::OutdatedAlgorithms { algorithms } => Some(ConnectWarning {
                    hop: e.record.hop.clone(),
                    algorithms: algorithms.clone(),
                }),
                _ => None,
            })
            .collect()
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

    /// Issue #1220: like [`Self::fail`], but attributes the step to another
    /// hop. Used when a bastion refuses to open the tunnel: the step stays
    /// the tunnel to the target, yet the hop that failed is the bastion.
    pub fn fail_at_hop(&self, id: StepId, hop_index: usize, hop: &str, code: &str) {
        if let Some(entry) = self.lock().get_mut(id.0) {
            if entry.record.status == StepStatus::Running {
                entry.record.hop_index = hop_index;
                entry.record.hop = hop.to_string();
            }
        }
        self.fail(id, code);
    }

    fn finish(&self, id: StepId, status: StepStatus) {
        if let Some(entry) = self.lock().get_mut(id.0) {
            if entry.record.status == StepStatus::Running {
                entry.record.status = status;
                entry.record.duration_ms = Some(millis(entry.started));
            }
        }
        self.notify();
    }

    /// Wie [`Self::fail_running`] für eine abgelaufene Zeitgrenze: läuft
    /// gerade die TCP-Verbindung, lautet der Code `SSH_TCP_TIMEOUT`, sonst
    /// `SSH_TIMEOUT`.
    pub fn fail_running_timeout(&self) -> &'static str {
        let tcp = self.lock().iter().any(|e| {
            e.record.status == StepStatus::Running
                && matches!(e.record.step, ConnectStep::TcpConnect { .. })
        });
        let code = if tcp { TCP_TIMEOUT_CODE } else { "SSH_TIMEOUT" };
        self.fail_running(code);
        code
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
        self.notify();
    }

    /// Momentaufnahme aller Schritte in Reihenfolge.
    pub fn snapshot(&self) -> Vec<ConnectStepRecord> {
        self.lock().iter().map(|e| e.record.clone()).collect()
    }
}

/// Short, untranslated detail of a step for the live view (Issue #546): a
/// stable `code` plus parameters, so the frontend can translate it. Never
/// carries a key path, passphrase, password or key material — the
/// authentication detail is the method name only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum StepDetail {
    /// DNS / TCP: `host:port` being contacted.
    Contacting { target: String },
    /// Jump hop tunnel: the hop name.
    Tunnel { hop: String },
    /// Handshake while running.
    WaitingForResponse,
    /// Host key check while the host key dialog is open.
    WaitingForDecision,
    /// Authentication: the method currently tried
    /// (`publickey (agent)`, `publickey (key file)`, `password`).
    AuthMethod { method: String },
}

/// One step as sent in the `connect-progress` event (Issue #546).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ConnectStepSnapshot {
    /// Step kind, camelCase (`dnsResolution`, `tcpConnect`, `tunnelOpen`,
    /// `handshake`, `hostKeyCheck`, `authentication`, `sessionReady`).
    #[cfg_attr(
        feature = "ts-export",
        ts(
            type = "\"dnsResolution\" | \"tcpConnect\" | \"tunnelOpen\" | \"handshake\" | \"hostKeyCheck\" | \"authentication\" | \"sessionReady\" | \"outdatedAlgorithms\""
        )
    )]
    pub kind: String,
    pub hop_index: usize,
    /// Hop label (`user@host:port`, Spec 0076 naming).
    pub hop: String,
    /// `running` | `ok` | `skipped` | `warning` | `failed`.
    #[cfg_attr(
        feature = "ts-export",
        ts(type = "\"running\" | \"ok\" | \"skipped\" | \"warning\" | \"failed\"")
    )]
    pub status: String,
    /// Error code when `failed`.
    pub error_code: Option<String>,
    pub detail: Option<StepDetail>,
}

impl ConnectStepSnapshot {
    /// `awaiting_host_key`: the host key dialog is open — the host key check
    /// of the last hop (which the transport closed as `HOST_KEY_UNKNOWN` /
    /// `HOST_KEY_CHANGED`) is shown as running and waiting for the decision.
    pub fn from_records(records: &[ConnectStepRecord], awaiting_host_key: bool) -> Vec<Self> {
        let waiting_idx = if awaiting_host_key {
            records.iter().rposition(|r| {
                matches!(
                    (&r.step, &r.status),
                    (ConnectStep::HostKeyCheck { .. }, StepStatus::Failed { code })
                        if code == HOST_KEY_UNKNOWN || code == HOST_KEY_CHANGED
                )
            })
        } else {
            None
        };
        records
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let waiting = waiting_idx == Some(i);
                let (status, error_code) = match (&r.status, waiting) {
                    (_, true) | (StepStatus::Running, _) => ("running", None),
                    (StepStatus::Ok, _) => ("ok", None),
                    (StepStatus::Skipped, _) => ("skipped", None),
                    (StepStatus::Warning { .. }, _) => ("warning", None),
                    (StepStatus::Failed { code }, _) => ("failed", Some(code.clone())),
                };
                let running = status == "running";
                let detail = match &r.step {
                    ConnectStep::DnsResolution { host, port, .. } => Some(StepDetail::Contacting {
                        target: format!("{host}:{port}"),
                    }),
                    ConnectStep::TcpConnect { address, port } => Some(StepDetail::Contacting {
                        target: match address {
                            Some(a) => a.clone(),
                            None => format!("{}:{port}", host_of(&r.hop)),
                        },
                    }),
                    ConnectStep::TunnelOpen { .. } => {
                        Some(StepDetail::Tunnel { hop: r.hop.clone() })
                    }
                    ConnectStep::Handshake { .. } if running => {
                        Some(StepDetail::WaitingForResponse)
                    }
                    ConnectStep::HostKeyCheck { .. } if waiting => {
                        Some(StepDetail::WaitingForDecision)
                    }
                    ConnectStep::Authentication { method, .. } => Some(StepDetail::AuthMethod {
                        method: auth_method_label(*method).to_string(),
                    }),
                    _ => None,
                };
                Self {
                    kind: step_kind(&r.step).to_string(),
                    hop_index: r.hop_index,
                    hop: r.hop.clone(),
                    status: status.to_string(),
                    error_code,
                    detail,
                }
            })
            .collect()
    }
}

/// Host part of a `user@host:port` hop label.
fn host_of(hop: &str) -> &str {
    let after_user = hop.rsplit_once('@').map_or(hop, |(_, h)| h);
    after_user.rsplit_once(':').map_or(after_user, |(h, _)| h)
}

fn step_kind(step: &ConnectStep) -> &'static str {
    match step {
        ConnectStep::DnsResolution { .. } => "dnsResolution",
        ConnectStep::TcpConnect { .. } => "tcpConnect",
        ConnectStep::TunnelOpen { .. } => "tunnelOpen",
        ConnectStep::Handshake { .. } => "handshake",
        ConnectStep::HostKeyCheck { .. } => "hostKeyCheck",
        ConnectStep::Authentication { .. } => "authentication",
        ConnectStep::SessionReady => "sessionReady",
        ConnectStep::OutdatedAlgorithms { .. } => "outdatedAlgorithms",
    }
}

fn auth_method_label(kind: AuthMethodKind) -> &'static str {
    match kind {
        AuthMethodKind::None => "none",
        AuthMethodKind::Password => "password",
        AuthMethodKind::KeyboardInteractive => "keyboard-interactive",
        AuthMethodKind::Agent => "publickey (agent)",
        AuthMethodKind::PrivateKey | AuthMethodKind::Certificate | AuthMethodKind::IdentityFile => {
            "publickey (key file)"
        }
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
    fn warn_outdated_merges_per_hop_without_duplicates() {
        let log = ConnectLog::new();
        log.warn_outdated(0, "u@a:22", vec!["ssh-rsa".into()]);
        log.warn_outdated(0, "u@a:22", vec!["ssh-rsa".into(), "3des-cbc".into()]);
        log.warn_outdated(1, "u@b:22", vec!["diffie-hellman-group1-sha1".into()]);
        let w = log.warnings();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].hop, "u@a:22");
        assert_eq!(w[0].algorithms, vec!["ssh-rsa", "3des-cbc"]);
        assert_eq!(w[1].algorithms, vec!["diffie-hellman-group1-sha1"]);
    }

    #[test]
    fn no_common_algorithm_error_names_hop_kind_and_offers() {
        use crate::ssh::{AlgorithmKind, SshError};
        let e = SshError::NoCommonAlgorithm {
            hop: crate::ssh::HopLabel {
                username: "u".into(),
                host: "a".into(),
                port: 22,
            },
            kind: AlgorithmKind::HostKey,
            server_offers: vec!["ssh-dss".into()],
        };
        assert_eq!(e.code(), "SSH_NO_COMMON_ALGORITHM");
        let m = e.to_string();
        assert!(
            m.contains("u@a:22") && m.contains("host key") && m.contains("ssh-dss"),
            "{m}"
        );
    }

    #[test]
    fn fail_at_hop_reattributes_a_running_step() {
        let log = ConnectLog::new();
        let id = log.start(
            1,
            "u@target:22",
            ConnectStep::TunnelOpen {
                host: "target".to_string(),
                port: 22,
            },
        );
        log.fail_at_hop(id, 0, "u@bastion:22", "SSH_CHANNEL_ERROR");
        let rec = &log.snapshot()[0];
        assert_eq!((rec.hop_index, rec.hop.as_str()), (0, "u@bastion:22"));
        assert!(matches!(rec.status, StepStatus::Failed { .. }));
    }

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
    fn timeout_during_tcp_step_gets_tcp_timeout_code() {
        let log = ConnectLog::new();
        let dns = log.start(
            0,
            "u@h:22",
            ConnectStep::DnsResolution {
                host: "10.255.255.1".into(),
                port: 22,
                addresses: vec![],
            },
        );
        log.skip(dns);
        log.start(
            0,
            "u@h:22",
            ConnectStep::TcpConnect {
                address: None,
                port: 22,
            },
        );
        assert_eq!(log.fail_running_timeout(), "SSH_TCP_TIMEOUT");
        let steps = log.snapshot();
        assert_eq!(steps[0].status, StepStatus::Skipped);
        assert_eq!(
            steps[1].status,
            StepStatus::Failed {
                code: "SSH_TCP_TIMEOUT".into()
            }
        );
    }

    #[test]
    fn timeout_in_later_step_keeps_ssh_timeout_code() {
        let log = ConnectLog::new();
        let tcp = log.start(
            0,
            "u@h:22",
            ConnectStep::TcpConnect {
                address: None,
                port: 22,
            },
        );
        log.succeed(tcp);
        log.start(0, "u@h:22", ConnectStep::SessionReady);
        assert_eq!(log.fail_running_timeout(), "SSH_TIMEOUT");
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

    #[test]
    fn sink_receives_full_snapshot_on_every_change() {
        let seen: Arc<Mutex<Vec<Vec<ConnectStepSnapshot>>>> = Arc::default();
        let sink_seen = seen.clone();
        let log = ConnectLog::with_sink(Arc::new(move |s| sink_seen.lock().unwrap().push(s)));
        let hs = log.start(
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
        log.update(hs, |_| {});
        log.fail(hs, "SSH_TIMEOUT");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0][0].status, "running");
        assert_eq!(seen[0][0].detail, Some(StepDetail::WaitingForResponse));
        assert_eq!(seen[2][0].status, "failed");
        assert_eq!(seen[2][0].error_code.as_deref(), Some("SSH_TIMEOUT"));
        assert_eq!(seen[2][0].detail, None);
    }

    #[test]
    fn auth_detail_names_only_the_method_and_host_key_wait_overlays() {
        let log = ConnectLog::new();
        let a = log.start(
            0,
            "u@h:22",
            ConnectStep::Authentication {
                method: AuthMethodKind::IdentityFile,
                remaining_methods: vec![],
                partial_success: false,
                round: None,
                prompts: vec![],
            },
        );
        log.succeed(a);
        let h = log.start(
            0,
            "u@h:22",
            ConnectStep::HostKeyCheck {
                key_type: Some("ssh-ed25519".into()),
                fingerprint: Some("SHA256:abc".into()),
                result: Some(HostKeyCheckResult::Unknown),
            },
        );
        log.fail(h, HOST_KEY_UNKNOWN);
        let plain = ConnectStepSnapshot::from_records(&log.snapshot(), false);
        assert_eq!(plain[1].status, "failed");
        let waiting = ConnectStepSnapshot::from_records(&log.snapshot(), true);
        assert_eq!(waiting[1].status, "running");
        assert_eq!(waiting[1].error_code, None);
        assert_eq!(waiting[1].detail, Some(StepDetail::WaitingForDecision));
        let json = serde_json::to_string(&waiting).unwrap();
        assert!(json.contains("publickey (key file)"));
        assert!(!json.contains("SHA256"), "no fingerprint/key data: {json}");
    }

    #[test]
    fn tunnel_step_snapshot_carries_kind_hop_label_and_hop_detail() {
        let log = ConnectLog::new();
        let t = log.start(
            1,
            "jump@bastion:22",
            ConnectStep::TunnelOpen {
                host: "target".into(),
                port: 22,
            },
        );
        log.succeed(t);
        let snap = ConnectStepSnapshot::from_records(&log.snapshot(), false);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].kind, "tunnelOpen");
        assert_eq!(snap[0].hop, "jump@bastion:22");
        assert_eq!(snap[0].hop_index, 1);
        assert_eq!(snap[0].status, "ok");
        assert_eq!(
            snap[0].detail,
            Some(StepDetail::Tunnel {
                hop: "jump@bastion:22".into()
            })
        );
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains(r#""code":"tunnel""#) && json.contains(r#""hop":"jump@bastion:22""#));
    }
}
