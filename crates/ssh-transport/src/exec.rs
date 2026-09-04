use russh::ChannelMsg;
use ssh_manager_core::ssh::CommandOutput;

/// Baut aus einer Sequenz von `ChannelMsg`s (wie sie `Channel::wait()` im
/// Exec-Modus liefert) das fertige [`CommandOutput`] zusammen.
///
/// Bewusst als reine Funktion von der eigentlichen `Channel`-I/O entkoppelt
/// — dadurch ohne echtes Netzwerk testbar (`cargo test -p ssh-transport
/// --lib`). Die echte `execute()`-Implementierung (s.
/// `crate::transport::RusshTransport`) treibt `channel.wait()` aber NICHT
/// erst vollständig durch, bevor sie ihre Nachrichten hier hineingibt (das
/// wäre exakt Fund A aus Spec 0043: der Cap griffe dann erst, nachdem ein
/// feindlicher Server bereits beliebig viel Speicher hätte belegen können)
/// — stattdessen speist [`ExecAccumulator::push`] jede Nachricht direkt beim
/// Eintreffen ein, und der Aufrufer bricht `channel.wait()` ab, sobald
/// [`ExecAccumulator::cap_reached`] `true` liefert.
/// Maximale Puffergröße pro Stream (stdout / stderr) vor dem Abschneiden (Spec 0013, SEC-09;
/// Spec 0043, Fund A: greift jetzt WÄHREND des Streamings, s. `crate::transport::drain_channel`).
/// Default — konfigurierbar über `RusshTransport::with_max_output_bytes`
/// bzw. `ExecAccumulator::with_limit` (Spec 0043, Abschnitt 7: interne
/// Konstante mit klarer Benennung reicht, kein Nutzer-Bedienknopf nötig).
pub const MAX_STREAM_OUTPUT_BYTES: usize = 2 * 1024 * 1024; // 2 MB
pub const TRUNCATION_NOTICE: &[u8] = b"\n[Output truncated: exceeded limit]";

/// Inkrementeller Sammler für `ChannelMsg`s eines Exec-Channels (Spec 0043,
/// Fund A). Wächst nie über das konfigurierte Limit pro Stream hinaus —
/// jede eingehende `ChannelMsg::Data`/`ExtendedData` wird sofort gegen das
/// Limit geprüft, statt roh in einem Zwischenpuffer zu landen, der erst am
/// Ende beschnitten wird.
#[derive(Debug)]
pub(crate) struct ExecAccumulator {
    limit: usize,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    exit_code: Option<i32>,
}

impl ExecAccumulator {
    /// Nur für Tests (s. `accumulate_exec_output` unten) — die echte
    /// `RusshTransport`-Implementierung ruft immer `with_limit` mit ihrem
    /// konfigurierten `max_output_bytes` auf, nie den Default direkt.
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_limit(MAX_STREAM_OUTPUT_BYTES)
    }

    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            exit_code: None,
        }
    }

    pub(crate) fn push(&mut self, msg: ChannelMsg) {
        match msg {
            ChannelMsg::Data { data } => {
                if !self.stdout_truncated {
                    if self.stdout.len() + data.len() <= self.limit {
                        self.stdout.extend_from_slice(&data);
                    } else {
                        let remaining = self.limit.saturating_sub(self.stdout.len());
                        self.stdout.extend_from_slice(&data[..remaining]);
                        self.stdout.extend_from_slice(TRUNCATION_NOTICE);
                        self.stdout_truncated = true;
                    }
                }
            }
            // Extended-Data-Code 1 = stderr (RFC4254 5.2); andere Codes sind
            // nicht spezifiziert und werden ignoriert.
            ChannelMsg::ExtendedData { data, ext: 1 } => {
                if !self.stderr_truncated {
                    if self.stderr.len() + data.len() <= self.limit {
                        self.stderr.extend_from_slice(&data);
                    } else {
                        let remaining = self.limit.saturating_sub(self.stderr.len());
                        self.stderr.extend_from_slice(&data[..remaining]);
                        self.stderr.extend_from_slice(TRUNCATION_NOTICE);
                        self.stderr_truncated = true;
                    }
                }
            }
            ChannelMsg::ExitStatus { exit_status } => self.exit_code = Some(exit_status as i32),
            _ => {}
        }
    }

    /// `true`, sobald IRGENDEIN Stream sein Limit erreicht hat — der
    /// Aufrufer (`crate::transport::drain_channel{,_cancellable}`) bricht
    /// `channel.wait()` dann ab, statt weiter auf Nachrichten zu warten.
    /// Bewusst ODER statt UND: ein Server, der ausschließlich stdout flutet
    /// (der praktisch häufigste Fall, z. B. `yes | head -c 5G`) und nie
    /// stderr schreibt, würde `stderr_truncated` sonst nie erreichen — ein
    /// UND hätte hier exakt den unbegrenzten Wartezustand zur Folge, den
    /// Fund A verhindern soll (kein Hänger, s. Spec 0043, Abschnitt 5),
    /// nur eben ohne unbegrenztes Speicherwachstum statt mit. Kehrseite:
    /// noch ausstehende (kleine) stderr-Ausgabe oder der Exit-Code können
    /// verloren gehen, wenn zuerst stdout abschneidet — hinnehmbar, da das
    /// Ergebnis ohnehin als `truncated` markiert wird und dieselbe
    /// Fail-safe-Richtung wie der Rest dieser Spec hat.
    pub(crate) fn cap_reached(&self) -> bool {
        self.stdout_truncated || self.stderr_truncated
    }

    pub(crate) fn into_output(self) -> CommandOutput {
        CommandOutput {
            stdout: self.stdout,
            stderr: self.stderr,
            exit_code: self.exit_code,
            truncated: self.stdout_truncated || self.stderr_truncated,
        }
    }
}

/// Nur für Unit-Tests (`src/tests.rs`) — treibt [`ExecAccumulator`] ohne
/// echte `Channel`-I/O über eine feste `Vec<ChannelMsg>`. Die echte
/// `execute()`-Implementierung nutzt `ExecAccumulator` direkt inkrementell
/// (s. `crate::transport::drain_channel`), nicht diesen Wrapper.
#[cfg(test)]
pub(crate) fn accumulate_exec_output(
    messages: impl IntoIterator<Item = ChannelMsg>,
) -> CommandOutput {
    let mut acc = ExecAccumulator::new();
    for msg in messages {
        acc.push(msg);
    }
    acc.into_output()
}
