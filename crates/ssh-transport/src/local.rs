//! Lokaler Pseudo-Server (Spec 0032): `LocalTransport`/`LocalShell`
//! implementieren `SshTransport`/`InteractiveShell` über direkte lokale
//! Prozessausführung statt über das SSH-Protokoll — kein `russh`, keine
//! Verbindung, kein Host-Key. `LocalFileSession` (s. `crate::local_sftp`)
//! implementiert `SftpSession` entsprechend über das lokale Dateisystem.
//!
//! Architektur-Vorteil (Spec 0032, Abschnitt 2): weil die Kernschleife,
//! die Filter-Engine und alles andere ausschließlich gegen die Traits aus
//! `ssh_manager_core::ssh` programmiert sind, braucht keine dieser
//! Komponenten irgendeine Änderung, um auch mit diesem lokalen Transport zu
//! funktionieren.

use std::io::{Read, Write};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use portable_pty::{native_pty_system, CommandBuilder, PtySize as PortablePtySize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use ssh_manager_core::ssh::{
    CommandOutput, InteractiveShell, PtySize, SftpSession, SshError, SshTransport,
};

use crate::exec::{CappedOutput, MAX_STREAM_OUTPUT_BYTES};
use crate::local_sftp::LocalFileSession;

fn io_err(context: &str, err: std::io::Error) -> SshError {
    SshError::ChannelError(format!("{context}: {err}"))
}

/// `sh -c <command>` (Unix) / `cmd /C <command>` (Windows) — dieselbe
/// Semantik wie ein SSH-`exec`-Channel, der ebenfalls das Kommando
/// unverändert an die Login-Shell des Zielsystems übergibt (Spec 0032,
/// Abschnitt 2).
fn shell_command(command: &str) -> Command {
    #[cfg(unix)]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
}

/// Standard-Shell des Nutzers für den interaktiven Modus (Spec 0032,
/// Abschnitt 2): `$SHELL` unter Unix (Fallback `/bin/sh`, falls die
/// Umgebungsvariable fehlt — z. B. in einer minimalen Prozessumgebung),
/// `powershell.exe` unter Windows (moderner Standard, im Gegensatz zu
/// `cmd.exe` für `execute()` oben — dort bewusst `cmd /C` als kleinster
/// gemeinsamer Nenner für ein einzelnes Kommando, hier PowerShell als
/// interaktive Shell, näher an dem, was ein Windows-Nutzer heute meist tatsächlich nutzt).
fn default_shell_command() -> CommandBuilder {
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        CommandBuilder::new(shell)
    }
    #[cfg(windows)]
    {
        CommandBuilder::new("powershell.exe")
    }
}

/// Kein Verbindungszustand nötig (Spec 0032, Abschnitt 2) — jeder
/// Methodenaufruf startet unabhängig einen neuen lokalen Prozess.
pub struct LocalTransport {
    /// Output-Cap für `execute()` (Spec 0044, dieselbe Mechanik/derselbe
    /// Default wie `RusshTransport::max_output_bytes`, Spec 0043 Fund A).
    /// Als Feld statt festem Konstantenzugriff, damit Tests einen
    /// kleineren Wert setzen können (über `SshTransport::
    /// set_max_output_bytes`), ohne echte Mehrbyte-Nutzlasten erzeugen zu
    /// müssen.
    max_output_bytes: usize,
}

impl LocalTransport {
    pub fn new() -> Self {
        Self {
            max_output_bytes: MAX_STREAM_OUTPUT_BYTES,
        }
    }
}

impl Default for LocalTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SshTransport for LocalTransport {
    /// Streamt `stdout`/`stderr` des lokalen Kindprozesses inkrementell
    /// (Spec 0044) statt sie über `Command::output()` erst vollständig zu
    /// puffern — dasselbe Muster wie `RusshTransport::execute` seit Spec
    /// 0043, Fund A, nur über rohe Pipes statt `ChannelMsg`s, deshalb
    /// über den geteilten [`CappedOutput`]-Kern statt [`crate::exec::
    /// ExecAccumulator`] (der an `ChannelMsg` gebunden ist). Sobald der Cap
    /// greift, wird nicht weiter gelesen und der Kindprozess beendet — der
    /// Rest seiner Ausgabe wird verworfen, das Ergebnis als `truncated`
    /// markiert.
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        let mut child = shell_command(command)
            // spec-reviewer-Fund (Review dieses Schritts): `Command::
            // output()` (der bisherige Aufruf hier) erbt `stdin` vom
            // Elternprozess wie `spawn()` auch — ein Kommando, das auf
            // Eingabe wartet (`cat`, `read x`, `sudo` ohne `-S`), würde
            // sonst unbegrenzt blockieren, während `execute_cancellable`
            // (Trait-Default) `cancel` ignoriert und die Session-weite
            // `transport`-Lock währenddessen gehalten wird — ein echter
            // Hänger ohne UI-Ausweg, genau das, was diese Spec (§5,
            // importiert aus Spec 0043 §1) ausdrücklich ausschließt. Ein
            // Nullstream statt geerbtem stdin lässt solche Kommandos
            // stattdessen sofort/schnell mit einem Fehler enden.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| io_err("lokale Ausführung fehlgeschlagen", e))?;
        let mut stdout = child
            .stdout
            .take()
            .expect("stdout wurde als Stdio::piped() angefordert");
        let mut stderr = child
            .stderr
            .take()
            .expect("stderr wurde als Stdio::piped() angefordert");

        let mut capped = CappedOutput::with_limit(self.max_output_bytes);
        let mut stdout_open = true;
        let mut stderr_open = true;
        let mut buf_out = [0u8; 8192];
        let mut buf_err = [0u8; 8192];
        // spec-reviewer-Fund: ein echter Lese-Fehler auf der Pipe ist NICHT
        // dasselbe wie ein reguläres EOF (`Ok(0)`) — beide vorher gleich zu
        // behandeln hätte eine unvollständige Ausgabe unmarkiert (weder
        // `truncated` noch `Err`) als vollständig/erfolgreich erscheinen
        // lassen, ein "sauberer, sichtbarer Abbruch" (Spec 0043 §1) sähe
        // dann aus wie ein stiller Erfolg. Der Fehler wird gemerkt und nach
        // der Schleife propagiert, statt den Stream einfach als beendet zu
        // behandeln.
        let mut io_error: Option<std::io::Error> = None;

        while (stdout_open || stderr_open) && !capped.cap_reached() {
            tokio::select! {
                res = stdout.read(&mut buf_out), if stdout_open => {
                    match res {
                        Ok(0) => stdout_open = false,
                        Ok(n) => capped.push_stdout(&buf_out[..n]),
                        Err(e) => { io_error.get_or_insert(e); stdout_open = false; }
                    }
                }
                res = stderr.read(&mut buf_err), if stderr_open => {
                    match res {
                        Ok(0) => stderr_open = false,
                        Ok(n) => capped.push_stderr(&buf_err[..n]),
                        Err(e) => { io_error.get_or_insert(e); stderr_open = false; }
                    }
                }
            }
        }

        // Cap gegriffen, bevor der Prozess von selbst beendet war (oder ein
        // Lese-Fehler zwingt zum Abbruch): Rest der Ausgabe verwerfen,
        // Kindprozess beenden statt auf sein reguläres Ende zu warten
        // (best effort — `kill()` beendet nur den direkten Kindprozess
        // selbst, z. B. `sh`; ein von ihm gestartetes Pipeline-Glied wie
        // bei `yes | head` läuft weiter, bis es beim nächsten Schreib-
        // versuch auf die inzwischen geschlossene Pipe `EPIPE`/`SIGPIPE`
        // bekommt — kein Hänger für uns, aber kein garantiertes sofortiges
        // Beenden der gesamten Pipeline, s. `RusshTransport::
        // drain_channel_cancellable`s identisch begründetem Best-effort-
        // Kommentar für den Remote-Fall).
        if capped.cap_reached() || io_error.is_some() {
            let _ = child.kill().await;
        }
        if let Some(e) = io_error {
            let _ = child.wait().await;
            return Err(io_err("Lesefehler bei lokaler Kommando-Ausgabe", e));
        }
        let status = child
            .wait()
            .await
            .map_err(|e| io_err("Warten auf lokalen Prozess fehlgeschlagen", e))?;

        let truncated = capped.truncated();
        let (stdout, stderr) = capped.into_parts();
        Ok(CommandOutput {
            stdout,
            stderr,
            // Unix: `kill()` beendet den Prozess per Signal statt eines
            // regulären Exits — `status.code()` ist dann `None`, der
            // Exit-Code geht beim Cap-Abbruch also verloren (identisch zum
            // Remote-Pfad, dessen `ExitStatus`-Nachricht beim Cap ebenfalls
            // nie mehr eintrifft). Kein Datenverlust-Problem, da `truncated`
            // dieselbe "unvollständig, nicht vertrauenswürdig als vollständiges
            // Ergebnis" ohnehin trägt.
            exit_code: status.code(),
            truncated,
        })
    }

    fn set_max_output_bytes(&mut self, limit: usize) {
        // s. `RusshTransport::set_max_output_bytes`-Kommentar (Spec 0043-
        // Review-Fund): nach oben geklemmt, damit dieser Testhook den Cap
        // nur verschärfen (verkleinern), nie über den sicheren Default
        // hinaus lockern kann.
        self.max_output_bytes = limit.min(MAX_STREAM_OUTPUT_BYTES);
    }

    // `execute_with_stdin`/`execute_cancellable`/`execute_with_stdin_cancellable`:
    // bewusst auf den Trait-Default belassen (delegiert an `execute`, ohne
    // `stdin`/`cancel` zu berücksichtigen). Ein "echter" Abbruch wäre hier
    // durch `Child::kill()` sogar zuverlässiger möglich als bei SSH (Spec
    // 0027, dort nur Best-effort über ein optionales Signal) — für den
    // ersten Schritt aber bewusst nicht umgesetzt, um den Umfang klein zu
    // halten; `sudo -S`-Stdin-Zufuhr ist für den lokalen Pseudo-Server
    // ohnehin nicht relevant (kein hinterlegtes Sudo-Passwort möglich, s.
    // `crate::local`-Verwendung in `app-shell`).

    async fn open_shell(&mut self, size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PortablePtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| {
                SshError::ChannelError(format!("lokales PTY konnte nicht geöffnet werden: {e}"))
            })?;

        let child = pair
            .slave
            .spawn_command(default_shell_command())
            .map_err(|e| {
                SshError::ChannelError(format!("lokale Shell konnte nicht gestartet werden: {e}"))
            })?;
        // Das Slave-Ende gehört ab hier ausschließlich dem Kindprozess —
        // im Elternprozess offen gehalten, würde ein `read()` auf dem
        // Master nie ein EOF sehen, selbst nachdem die Shell beendet ist.
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| SshError::ChannelError(format!("PTY-Writer nicht verfügbar: {e}")))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| SshError::ChannelError(format!("PTY-Reader nicht verfügbar: {e}")))?;

        Ok(Box::new(LocalShell {
            master: pair.master,
            writer: Arc::new(StdMutex::new(writer)),
            reader: Arc::new(StdMutex::new(reader)),
            _child: child,
        }))
    }

    async fn open_sftp(&mut self) -> Result<Box<dyn SftpSession>, SshError> {
        Ok(Box::new(LocalFileSession::new()))
    }

    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

/// PTY-Shell für den interaktiven Modus des lokalen Pseudo-Servers.
///
/// `portable-pty`s Lese-/Schreib-Handles sind Standard-`std::io`
/// (blockierend, echte OS-Datei-Deskriptoren) — jeder Aufruf läuft daher
/// über `spawn_blocking`, statt den Tokio-Executor mit einem blockierenden
/// Syscall zu belegen. Die Handles selbst liegen hinter `Arc<StdMutex<_>>`
/// statt direkt in `self`, damit sie in die `'static`-Closure von
/// `spawn_blocking` verschoben (und danach wieder freigegeben) werden
/// können, ohne den ganzen `LocalShell` zu bewegen.
pub(crate) struct LocalShell {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Arc<StdMutex<Box<dyn Write + Send>>>,
    reader: Arc<StdMutex<Box<dyn Read + Send>>>,
    // Nur am Leben gehalten (beendet den Kindprozess beim Drop, je nach
    // Plattform) — nie direkt angesprochen, führendes `_` gegen Clippys
    // "totes Feld"-Warnung.
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[async_trait]
impl InteractiveShell for LocalShell {
    async fn write(&mut self, data: &[u8]) -> Result<(), SshError> {
        let writer = self.writer.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut guard = writer.lock().expect("PTY-Writer-Mutex vergiftet");
            guard.write_all(&data).and_then(|()| guard.flush())
        })
        .await
        .map_err(|e| SshError::ChannelError(format!("PTY-Schreib-Task abgebrochen: {e}")))?
        .map_err(|e| io_err("PTY-Schreibfehler", e))
    }

    /// Blockiert, bis Daten verfügbar sind oder EOF erreicht wird — wie von
    /// `InteractiveShell::read` gefordert. Ein `Ok(0)` vom zugrunde
    /// liegenden `Read` (EOF, z. B. weil die Shell beendet wurde) wird als
    /// leerer `Vec` zurückgegeben, exakt wie bei `RusshShell::read` bei
    /// `ChannelMsg::Eof`/`Close`.
    async fn read(&mut self) -> Result<Vec<u8>, SshError> {
        let reader = self.reader.clone();
        tokio::task::spawn_blocking(move || {
            let mut buf = [0u8; 8192];
            let mut guard = reader.lock().expect("PTY-Reader-Mutex vergiftet");
            match guard.read(&mut buf) {
                Ok(0) => Ok(Vec::new()),
                Ok(n) => Ok(buf[..n].to_vec()),
                Err(e) => Err(e),
            }
        })
        .await
        .map_err(|e| SshError::ChannelError(format!("PTY-Lese-Task abgebrochen: {e}")))?
        .map_err(|e| io_err("PTY-Lesefehler", e))
    }

    async fn resize(&mut self, size: PtySize) -> Result<(), SshError> {
        self.master
            .resize(PortablePtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| SshError::ChannelError(format!("PTY-Größenänderung fehlgeschlagen: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 0032: `LocalTransport::execute()` liefert korrekten
    /// stdout/stderr/exit-code für ein einfaches Testkommando.
    #[tokio::test]
    async fn test_execute_returns_stdout_stderr_and_exit_code() {
        let mut transport = LocalTransport::new();
        #[cfg(unix)]
        let command = "echo out; echo err 1>&2; exit 7";
        #[cfg(windows)]
        let command = "echo out & echo err 1>&2 & exit 7";

        let output = transport.execute(command).await.unwrap();

        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "err");
        assert_eq!(output.exit_code, Some(7));
    }

    #[tokio::test]
    async fn test_execute_reports_nonzero_exit_code_without_error() {
        let mut transport = LocalTransport::new();
        let output = transport.execute("exit 1").await.unwrap();
        assert_eq!(output.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_disconnect_is_a_no_op_success() {
        let mut transport = LocalTransport::new();
        assert!(transport.disconnect().await.is_ok());
    }

    /// Spec 0044: kein Auseinanderdriften der Default-Caps zwischen Remote-
    /// (`RusshTransport`, s. `crate::connect::connect_hop_chain`, das
    /// `max_output_bytes` ebenfalls aus `exec::MAX_STREAM_OUTPUT_BYTES`
    /// initialisiert) und lokalem Pseudo-Server — beide lesen denselben
    /// benannten Konstanten-Wert statt je einer eigenen Literal-Kopie.
    #[test]
    fn test_t44_default_output_cap_matches_shared_remote_constant() {
        let transport = LocalTransport::new();
        assert_eq!(
            transport.max_output_bytes,
            crate::exec::MAX_STREAM_OUTPUT_BYTES
        );
    }

    /// Spec 0044: ein lokales Kommando, das mehr als das (hier klein
    /// gesetzte) Limit ausgibt, darf den Puffer nie darüber hinaus wachsen
    /// lassen — analog zum Remote-Test aus Spec 0043
    /// (`test_t43_execute_caps_output_during_streaming` in
    /// `tests/integration.rs`), hier gegen den lokalen Pseudo-Server. `yes`
    /// endet nie von selbst — beweist zugleich, dass `execute()` trotzdem
    /// zurückkehrt (kein Hänger, Kindprozess wird beim Cap beendet).
    #[tokio::test]
    async fn test_t44_execute_caps_output_during_streaming() {
        let mut transport = LocalTransport::new();
        const SMALL_LIMIT: usize = 4096;
        transport.set_max_output_bytes(SMALL_LIMIT);

        #[cfg(unix)]
        let command = "yes AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        #[cfg(windows)]
        // spec-reviewer-Fund: `for /L %i in ()` (leere Mengenangabe) ist
        // kein gültiges `FOR /L` — `cmd` bricht mit Syntaxfehler ab bzw.
        // iteriert nicht, der Cap würde also nie greifen. `(1,0,2)`
        // (Start 1, Schrittweite 0, Ende 2) zählt nie über 2 hinaus und
        // läuft damit endlos — die eigentlich gewollte Endlosschleife.
        let command = "for /L %i in (1,0,2) do @echo AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            transport.execute(command),
        )
        .await
        .expect("execute() darf bei einem flutenden lokalen Kommando nicht hängen bleiben")
        .expect("execute() sollte trotz Abschneiden Ok liefern");

        assert!(
            output.stdout.len() <= SMALL_LIMIT + crate::exec::TRUNCATION_NOTICE.len(),
            "stdout darf nie über das konfigurierte Limit hinauswachsen, war aber {} Bytes",
            output.stdout.len()
        );
        assert!(
            output.truncated,
            "CommandOutput.truncated muss gesetzt sein, wenn der Cap gegriffen hat"
        );
    }

    /// Ein normales, kurzes Kommando bleibt unbeschnitten — kein Fehlalarm
    /// durch den neuen Cap-Mechanismus.
    #[tokio::test]
    async fn test_t44_execute_leaves_small_output_untruncated() {
        let mut transport = LocalTransport::new();
        transport.set_max_output_bytes(4096);

        let output = transport.execute("echo hello").await.unwrap();

        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
        assert!(!output.truncated);
    }

    /// spec-reviewer-Fund (Review dieses Schritts): ein Kommando, das auf
    /// Eingabe wartet (hier `cat` ohne Argumente), darf nicht auf geerbtes
    /// Eltern-`stdin` warten — das wäre ein echter, UI-loser Hänger (die
    /// Session-`transport`-Lock bliebe belegt, `execute_cancellable` hat
    /// hier keinen echten Abbruch). `stdin(Stdio::null())` lässt `cat`
    /// sofort per EOF beenden statt zu blockieren.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_t44_execute_does_not_hang_on_a_command_waiting_for_stdin() {
        let mut transport = LocalTransport::new();

        let output =
            tokio::time::timeout(std::time::Duration::from_secs(5), transport.execute("cat"))
                .await
                .expect(
                    "execute() darf bei einem auf stdin wartenden Kommando nicht hängen bleiben",
                )
                .expect("execute() sollte trotz sofortigem EOF auf stdin Ok liefern");

        assert!(output.stdout.is_empty());
        assert_eq!(output.exit_code, Some(0));
    }
}
