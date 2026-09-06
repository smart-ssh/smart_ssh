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
/// Spec 0043, Fund A: greift jetzt WÄHREND des Streamings, s. `crate::transport::drain_channel`;
/// Spec 0044: derselbe Default/Mechanismus auch für `crate::local::LocalTransport`, s.
/// [`CappedOutput`]).
/// Default — konfigurierbar über `SshTransport::set_max_output_bytes`
/// (Testhook, nach oben geklemmt) bzw. `CappedOutput::with_limit`/
/// `ExecAccumulator::with_limit` (Spec 0043, Abschnitt 7: interne
/// Konstante mit klarer Benennung reicht, kein Nutzer-Bedienknopf nötig).
pub const MAX_STREAM_OUTPUT_BYTES: usize = 2 * 1024 * 1024; // 2 MB
pub const TRUNCATION_NOTICE: &[u8] = b"\n[Output truncated: exceeded limit]";

/// Transport-agnostischer Kern des Streaming-Caps (Spec 0043, Fund A; Spec
/// 0044, Abschnitt 2: "wenn die Cap-Logik ... als wiederverwendbare
/// Funktion/Helfer vorliegt, wird sie geteilt, nicht dupliziert"). Kennt
/// nur rohe `stdout`/`stderr`-Byte-Chunks, keine `ChannelMsg`s — dadurch
/// von [`ExecAccumulator`] (Remote-Pfad, füttert `ChannelMsg`-Payloads ein)
/// UND von `crate::local::LocalTransport::execute` (lokaler Pseudo-Server,
/// füttert Chunks direkt aus den Prozess-Pipes ein) gleichermaßen nutzbar.
/// Wächst nie über `limit` Bytes pro Stream hinaus — jeder eingehende Chunk
/// wird sofort gegen das Limit geprüft, statt roh in einem Zwischenpuffer
/// zu landen, der erst am Ende beschnitten wird.
#[derive(Debug)]
pub(crate) struct CappedOutput {
    limit: usize,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

impl CappedOutput {
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    pub(crate) fn push_stdout(&mut self, data: &[u8]) {
        Self::push(
            &mut self.stdout,
            &mut self.stdout_truncated,
            self.limit,
            data,
        );
    }

    pub(crate) fn push_stderr(&mut self, data: &[u8]) {
        Self::push(
            &mut self.stderr,
            &mut self.stderr_truncated,
            self.limit,
            data,
        );
    }

    fn push(buf: &mut Vec<u8>, truncated: &mut bool, limit: usize, data: &[u8]) {
        if *truncated {
            return;
        }
        if buf.len() + data.len() <= limit {
            buf.extend_from_slice(data);
        } else {
            let remaining = limit.saturating_sub(buf.len());
            buf.extend_from_slice(&data[..remaining]);
            buf.extend_from_slice(TRUNCATION_NOTICE);
            *truncated = true;
        }
    }

    /// `true`, sobald IRGENDEIN Stream sein Limit erreicht hat — der
    /// Aufrufer bricht das weitere Lesen dann ab, statt weiter auf Daten zu
    /// warten. Bewusst ODER statt UND: ein Kommando, das ausschließlich
    /// stdout flutet (der praktisch häufigste Fall, z. B. `yes`) und nie
    /// stderr schreibt, würde `stderr_truncated` sonst nie erreichen — ein
    /// UND hätte hier exakt den unbegrenzten Wartezustand zur Folge, den
    /// Fund A verhindern soll (kein Hänger, s. Spec 0043, Abschnitt 5), nur
    /// eben ohne unbegrenztes Speicherwachstum statt mit. Kehrseite: noch
    /// ausstehende (kleine) stderr-Ausgabe oder der Exit-Code können
    /// verloren gehen, wenn zuerst stdout abschneidet — hinnehmbar, da das
    /// Ergebnis ohnehin als `truncated` markiert wird und dieselbe
    /// Fail-safe-Richtung wie der Rest dieser Spec hat.
    pub(crate) fn cap_reached(&self) -> bool {
        self.stdout_truncated || self.stderr_truncated
    }

    /// Bewusst identisch zu [`Self::cap_reached`] — beide beantworten
    /// aktuell dieselbe Frage ("wurde irgendwo abgeschnitten?"), stehen
    /// aber für zwei konzeptionell unterschiedliche Aufrufer-Fragen: der
    /// eine bricht damit das weitere Lesen ab ("noch mehr holen?"), der
    /// andere markiert das fertige [`CommandOutput`] ("war das Ergebnis
    /// vollständig?"). Falls sich das je auseinanderentwickelt (z. B. ein
    /// zukünftiger Grund, weiterzulesen, obwohl schon abgeschnitten wurde),
    /// bewusst als zwei separate Methoden erhalten statt zu einer
    /// zusammenzufassen.
    pub(crate) fn truncated(&self) -> bool {
        self.stdout_truncated || self.stderr_truncated
    }

    pub(crate) fn into_parts(self) -> (Vec<u8>, Vec<u8>) {
        (self.stdout, self.stderr)
    }
}

/// Inkrementeller Sammler für `ChannelMsg`s eines Exec-Channels (Spec 0043,
/// Fund A) — dünner `ChannelMsg`-spezifischer Wrapper um [`CappedOutput`],
/// ergänzt um den Exit-Code (den der lokale Pfad anders bekommt, s.
/// `crate::local`, daher nicht Teil von `CappedOutput` selbst).
#[derive(Debug)]
pub(crate) struct ExecAccumulator {
    capped: CappedOutput,
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
            capped: CappedOutput::with_limit(limit),
            exit_code: None,
        }
    }

    pub(crate) fn push(&mut self, msg: ChannelMsg) {
        match msg {
            ChannelMsg::Data { data } => self.capped.push_stdout(&data),
            // Extended-Data-Code 1 = stderr (RFC4254 5.2); andere Codes sind
            // nicht spezifiziert und werden ignoriert.
            ChannelMsg::ExtendedData { data, ext: 1 } => self.capped.push_stderr(&data),
            ChannelMsg::ExitStatus { exit_status } => self.exit_code = Some(exit_status as i32),
            _ => {}
        }
    }

    pub(crate) fn cap_reached(&self) -> bool {
        self.capped.cap_reached()
    }

    pub(crate) fn into_output(self) -> CommandOutput {
        let truncated = self.capped.truncated();
        let (stdout, stderr) = self.capped.into_parts();
        CommandOutput {
            stdout,
            stderr,
            exit_code: self.exit_code,
            truncated,
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
