use russh::ChannelMsg;
use ssh_manager_core::ssh::{CommandOutput, ExecOutputChunk, ExecOutputSink, OutputStream};

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
    /// Issue #325: live copy of every byte that is kept (never the
    /// truncation notice), plus one `Truncated` when the cap is reached.
    /// Display only — `None` for the non-streaming paths.
    sink: Option<ExecOutputSink>,
    /// Issue #922: set once the live copy was cut off (consumer too slow or
    /// cap reached); nothing more is forwarded afterwards.
    live_closed: bool,
}

impl CappedOutput {
    pub(crate) fn with_limit_and_sink(limit: usize, sink: Option<ExecOutputSink>) -> Self {
        Self {
            limit,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            sink,
            live_closed: false,
        }
    }

    pub(crate) fn push_stdout(&mut self, data: &[u8]) {
        let (kept, newly_truncated) = Self::push(
            &mut self.stdout,
            &mut self.stdout_truncated,
            self.limit,
            data,
        );
        self.forward(OutputStream::Stdout, kept, newly_truncated);
    }

    pub(crate) fn push_stderr(&mut self, data: &[u8]) {
        let (kept, newly_truncated) = Self::push(
            &mut self.stderr,
            &mut self.stderr_truncated,
            self.limit,
            data,
        );
        self.forward(OutputStream::Stderr, kept, newly_truncated);
    }

    /// Sends the kept part of a chunk to the live sink. A closed receiver
    /// only ends the live display, never the command — send errors are
    /// deliberately ignored (the final result does not depend on them).
    fn forward(&mut self, stream: OutputStream, kept: &[u8], newly_truncated: bool) {
        let Some(sink) = &self.sink else {
            return;
        };
        if self.live_closed {
            return;
        }
        if !kept.is_empty() {
            // Issue #922: the last slot is reserved for `Truncated`, so the
            // consumer always learns that the live copy ends here. Whole
            // chunks only are dropped, never part of one.
            if sink.capacity() > 1 {
                let _ = sink.try_send(ExecOutputChunk::Data {
                    stream,
                    bytes: kept.to_vec(),
                });
            } else {
                let _ = sink.try_send(ExecOutputChunk::Truncated);
                self.live_closed = true;
                return;
            }
        }
        if newly_truncated {
            let _ = sink.try_send(ExecOutputChunk::Truncated);
            self.live_closed = true;
        }
    }

    /// Returns the part of `data` that was kept and whether this call hit
    /// the cap.
    fn push<'a>(
        buf: &mut Vec<u8>,
        truncated: &mut bool,
        limit: usize,
        data: &'a [u8],
    ) -> (&'a [u8], bool) {
        if *truncated {
            return (&[], false);
        }
        if buf.len() + data.len() <= limit {
            buf.extend_from_slice(data);
            (data, false)
        } else {
            let remaining = limit.saturating_sub(buf.len());
            buf.extend_from_slice(&data[..remaining]);
            buf.extend_from_slice(TRUNCATION_NOTICE);
            *truncated = true;
            (&data[..remaining], true)
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

/// Shell-style exit code (128 + Linux signal number) for an SSH signal name;
/// `None` for names without a well-known number.
fn signal_exit_code(sig: &russh::Sig) -> Option<i32> {
    use russh::Sig;
    let n = match sig {
        Sig::HUP => 1,
        Sig::INT => 2,
        Sig::QUIT => 3,
        Sig::ILL => 4,
        Sig::ABRT => 6,
        Sig::FPE => 8,
        Sig::KILL => 9,
        Sig::USR1 => 10,
        Sig::SEGV => 11,
        Sig::PIPE => 13,
        Sig::ALRM => 14,
        Sig::TERM => 15,
        Sig::Custom(_) => return None,
    };
    Some(128 + n)
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
    /// `RusshTransport`-Implementierung ruft immer `with_limit_and_sink` mit ihrem
    /// konfigurierten `max_output_bytes` auf, nie den Default direkt.
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_limit_and_sink(MAX_STREAM_OUTPUT_BYTES, None)
    }

    /// Issue #325: like [`Self::new`], with a live copy of the kept
    /// output (see [`CappedOutput::with_limit_and_sink`]).
    pub(crate) fn with_limit_and_sink(limit: usize, sink: Option<ExecOutputSink>) -> Self {
        Self {
            capped: CappedOutput::with_limit_and_sink(limit, sink),
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
            // A command killed by a signal ends with `exit-signal` instead of
            // `exit-status` (RFC 4254 6.10). Report it the way a shell does
            // (128 + signal number) instead of "no exit code"; a real
            // exit status, if any, wins.
            ChannelMsg::ExitSignal { signal_name, .. } if self.exit_code.is_none() => {
                self.exit_code = signal_exit_code(&signal_name);
            }
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

#[cfg(test)]
mod exit_signal_tests {
    use super::*;

    fn signal(sig: russh::Sig) -> ChannelMsg {
        ChannelMsg::ExitSignal {
            signal_name: sig,
            core_dumped: false,
            error_message: String::new(),
            lang_tag: String::new(),
        }
    }

    #[test]
    fn kill_signal_reports_137_and_exit_status_wins() {
        let mut acc = ExecAccumulator::new();
        acc.push(signal(russh::Sig::KILL));
        assert_eq!(acc.into_output().exit_code, Some(137));

        let mut acc = ExecAccumulator::new();
        acc.push(ChannelMsg::ExitStatus { exit_status: 3 });
        acc.push(signal(russh::Sig::KILL));
        assert_eq!(acc.into_output().exit_code, Some(3));

        let mut acc = ExecAccumulator::new();
        acc.push(signal(russh::Sig::Custom("X".into())));
        assert_eq!(acc.into_output().exit_code, None);
    }
}

#[cfg(test)]
mod live_sink_tests {
    use super::*;

    /// Issue #922: 50 MB in small chunks with a consumer that never reads —
    /// the channel stays within its capacity, the last slot holds
    /// `Truncated`, and the captured output is complete (up to its own cap).
    #[test]
    fn paused_consumer_bounds_channel_and_keeps_captured_output() {
        let (sink, mut rx) = ssh_manager_core::ssh::exec_output_channel();
        let total = 50 * 1024 * 1024;
        let mut out = CappedOutput::with_limit_and_sink(total, Some(sink));
        let chunk = vec![b'x'; 1024];
        for _ in 0..(total / chunk.len()) {
            out.push_stdout(&chunk);
        }
        let (stdout, _) = out.into_parts();
        assert_eq!(stdout.len(), total);

        let mut data = 0;
        let mut truncated = 0;
        while let Ok(c) = rx.try_recv() {
            match c {
                ExecOutputChunk::Data { .. } => {
                    assert_eq!(truncated, 0, "no data after Truncated");
                    data += 1;
                }
                ExecOutputChunk::Truncated => truncated += 1,
            }
        }
        assert_eq!(
            data + truncated,
            ssh_manager_core::ssh::EXEC_OUTPUT_SINK_CAPACITY
        );
        assert_eq!(truncated, 1);
    }
}
