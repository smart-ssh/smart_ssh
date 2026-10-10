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
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use portable_pty::{native_pty_system, CommandBuilder, PtySize as PortablePtySize};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::oneshot;

use ssh_manager_core::ssh::{
    CommandOutput, ExecOutcome, InteractiveShell, PtySize, SftpSession, SshError, SshTransport,
};

use crate::exec::{CappedOutput, MAX_STREAM_OUTPUT_BYTES};
use crate::local_sftp::{user_home_dir, LocalFileSession};

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

/// Whether `path` is an existing directory (following symlinks, like
/// `Path::is_dir`), checked without blocking the async runtime (issue #23).
/// Any error (missing path, no permission, …) counts as "not a directory".
async fn is_existing_dir(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

/// Issue #324: wie lange ein abgebrochenes Kommando nach dem sanften
/// Abbruch (`SIGINT` an die Prozessgruppe) noch Zeit bekommt, sich selbst zu
/// beenden und letzte Ausgabe zu schreiben, bevor die Gruppe hart beendet
/// wird. Deutlich unter der 2-Sekunden-Zusage aus issue #324.
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// Die beiden Ausgabe-Pipes eines lokalen Kindprozesses samt Lesezustand.
struct ChildPipes {
    stdout: ChildStdout,
    stderr: ChildStderr,
    stdout_open: bool,
    stderr_open: bool,
    buf_out: [u8; 8192],
    buf_err: [u8; 8192],
}

impl ChildPipes {
    fn new(stdout: ChildStdout, stderr: ChildStderr) -> Self {
        Self {
            stdout,
            stderr,
            stdout_open: true,
            stderr_open: true,
            buf_out: [0u8; 8192],
            buf_err: [0u8; 8192],
        }
    }

    fn any_open(&self) -> bool {
        self.stdout_open || self.stderr_open
    }

    /// Liest den nächsten Block von der Pipe, die zuerst Daten (oder EOF)
    /// liefert, in `capped`. Nur aufrufen, solange [`any_open`](Self::any_open)
    /// gilt. Abbruchsicher: wird das Future verworfen, geht nichts verloren.
    /// Ein Lesefehler schließt die betroffene Pipe und wird zurückgegeben.
    async fn read_next(&mut self, capped: &mut CappedOutput) -> std::io::Result<()> {
        tokio::select! {
            res = self.stdout.read(&mut self.buf_out), if self.stdout_open => {
                match res {
                    Ok(0) => self.stdout_open = false,
                    Ok(n) => capped.push_stdout(&self.buf_out[..n]),
                    Err(e) => { self.stdout_open = false; return Err(e); }
                }
            }
            res = self.stderr.read(&mut self.buf_err), if self.stderr_open => {
                match res {
                    Ok(0) => self.stderr_open = false,
                    Ok(n) => capped.push_stderr(&self.buf_err[..n]),
                    Err(e) => { self.stderr_open = false; return Err(e); }
                }
            }
        }
        Ok(())
    }
}

/// Issue #324 (Spec 0027): beendet ein abgebrochenes lokales Kommando samt
/// allen Prozessen, die es gestartet hat, und sammelt bis dahin noch
/// geschriebene Ausgabe ein (unter demselben Cap, Spec 0044).
///
/// Unix: das Kommando läuft in einer eigenen Prozessgruppe (pgid = `pid`).
/// Erst `SIGINT` an die Gruppe (wie `Ctrl+C`), dann höchstens
/// [`CANCEL_GRACE`] lang weiterlesen, bis alle Pipes geschlossen sind, dann
/// in jedem Fall `SIGKILL` an die Gruppe — auch für Glieder, die `SIGINT`
/// ignorieren (z. B. Hintergrundprozesse einer nicht-interaktiven Shell).
/// Der Kindprozess wird erst danach eingesammelt (`wait`): bis dahin hält er
/// als Zombie die Gruppen-ID belegt, `killpg` kann also keine fremde Gruppe
/// treffen. Ein Prozess, der die Gruppe selbst verlässt (`setsid`), wird
/// nicht erreicht.
///
/// Windows: `taskkill /T /F` beendet den Prozessbaum ab `cmd.exe`.
async fn terminate_tree(
    child: &mut Child,
    pid: Option<u32>,
    pipes: &mut ChildPipes,
    capped: &mut CappedOutput,
) {
    #[cfg(unix)]
    let group = pid
        .and_then(|p| libc::pid_t::try_from(p).ok())
        .filter(|p| *p > 0);
    #[cfg(unix)]
    if let Some(pgid) = group {
        // SAFETY: `killpg` hat keine Speicher-Vorbedingungen; `pgid` ist die
        // Gruppe unseres noch nicht eingesammelten Kindprozesses.
        unsafe {
            libc::killpg(pgid, libc::SIGINT);
        }
    }
    #[cfg(windows)]
    if let Some(pid) = pid {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }

    let _ = tokio::time::timeout(CANCEL_GRACE, async {
        while pipes.any_open() && !capped.cap_reached() {
            let _ = pipes.read_next(capped).await;
        }
    })
    .await;

    #[cfg(unix)]
    if let Some(pgid) = group {
        // SAFETY: wie oben; der Kindprozess ist weiterhin nicht eingesammelt.
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
    // Rückfallebene (keine Gruppe/kein `taskkill`): zumindest den direkten
    // Kindprozess beenden. Für einen schon beendeten Prozess wirkungslos.
    let _ = child.start_kill();
    let _ = child.wait().await;
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
    /// User's home directory (issue #10): working directory of `execute()`
    /// and base for relative paths of the `LocalFileSession` from
    /// `open_sftp()` — the counterpart of the login home a real SSH server
    /// runs exec channels and SFTP in. `None` if it cannot be determined;
    /// both then keep the process cwd as before.
    home: Option<PathBuf>,
}

impl LocalTransport {
    pub fn new() -> Self {
        Self::with_home(user_home_dir())
    }

    /// Explicit home directory — for tests, so they need not mutate the
    /// process environment (`HOME`/`USERPROFILE`) in parallel test runs.
    pub(crate) fn with_home(home: Option<PathBuf>) -> Self {
        Self {
            max_output_bytes: MAX_STREAM_OUTPUT_BYTES,
            home,
        }
    }

    /// Gemeinsamer Kern von `execute`/`execute_cancellable` (s. deren
    /// Doc-Kommentare). `cancel = None`: nicht abbrechbar.
    async fn run(
        &self,
        command: &str,
        cancel: Option<oneshot::Receiver<()>>,
    ) -> Result<ExecOutcome, SshError> {
        let mut cmd = shell_command(command);
        // Issue #10: run in the home directory like an SSH exec channel
        // does (ADR 0059 assumes a home cwd). Only the working directory
        // changes, the command string is untouched. A home that is not an
        // existing directory would make every spawn fail, so it falls back
        // to the process cwd (as before) instead of breaking the session.
        // Issue #23: checked via `tokio::fs` so a slow (e.g. network-mounted)
        // home does not block a runtime worker thread; re-checked on every
        // call, so a home that appears or disappears mid-session is handled.
        if let Some(home) = self.home.as_deref() {
            if is_existing_dir(home).await {
                cmd.current_dir(home);
            }
        }
        cmd
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
            .stderr(Stdio::piped());
        // Issue #324: eigene Prozessgruppe (pgid = pid des `sh`), damit ein
        // Abbruch die ganze Pipeline samt Subshells erreicht, nicht nur den
        // direkten Kindprozess (s. `terminate_tree`).
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .spawn()
            .map_err(|e| io_err("lokale Ausführung fehlgeschlagen", e))?;
        // Vor dem ersten `wait()` gelesen: danach liefert `id()` `None`.
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .expect("stdout wurde als Stdio::piped() angefordert");
        let stderr = child
            .stderr
            .take()
            .expect("stderr wurde als Stdio::piped() angefordert");

        let mut capped = CappedOutput::with_limit(self.max_output_bytes);
        let mut pipes = ChildPipes::new(stdout, stderr);
        // spec-reviewer-Fund: ein echter Lese-Fehler auf der Pipe ist NICHT
        // dasselbe wie ein reguläres EOF (`Ok(0)`) — beide vorher gleich zu
        // behandeln hätte eine unvollständige Ausgabe unmarkiert (weder
        // `truncated` noch `Err`) als vollständig/erfolgreich erscheinen
        // lassen, ein "sauberer, sichtbarer Abbruch" (Spec 0043 §1) sähe
        // dann aus wie ein stiller Erfolg. Der Fehler wird gemerkt und nach
        // der Schleife propagiert, statt den Stream einfach als beendet zu
        // behandeln.
        let mut io_error: Option<std::io::Error> = None;

        // Issue #324: löst nur bei einem tatsächlich gesendeten Abbruch aus.
        // Ein ohne Senden gedroppter Sender (kein Abbruch angefordert) oder
        // `cancel = None` bleiben für immer offen — das Kommando läuft dann
        // regulär zu Ende.
        let cancel_requested = async move {
            if let Some(rx) = cancel {
                if rx.await.is_ok() {
                    return;
                }
            }
            std::future::pending::<()>().await
        };
        tokio::pin!(cancel_requested);
        let mut cancelled = false;

        while pipes.any_open() && !capped.cap_reached() {
            tokio::select! {
                res = pipes.read_next(&mut capped) => {
                    if let Err(e) = res {
                        io_error.get_or_insert(e);
                    }
                }
                _ = &mut cancel_requested => {
                    cancelled = true;
                    break;
                }
            }
        }

        // Spec 0027 / issue #324: Abbruch, solange noch Ausgabe-Pipes offen
        // waren. Ein Abbruch, der erst danach eintrifft (Kommando bereits
        // fertig), wird oben nicht mehr abgefragt und hat keine Wirkung —
        // das reguläre Ergebnis gilt (Spec 0027, §3.5).
        if cancelled {
            terminate_tree(&mut child, pid, &mut pipes, &mut capped).await;
            let truncated = capped.truncated();
            let (stdout, stderr) = capped.into_parts();
            return Ok(ExecOutcome {
                output: CommandOutput {
                    stdout,
                    stderr,
                    // Wie im SSH-Pfad: kein regulärer Exit-Code nach Abbruch.
                    exit_code: None,
                    truncated,
                },
                cancelled: true,
            });
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
        let output = CommandOutput {
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
        };
        Ok(ExecOutcome {
            output,
            cancelled: false,
        })
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
        Ok(self.run(command, None).await?.output)
    }

    fn set_max_output_bytes(&mut self, limit: usize) {
        // s. `RusshTransport::set_max_output_bytes`-Kommentar (Spec 0043-
        // Review-Fund): nach oben geklemmt, damit dieser Testhook den Cap
        // nur verschärfen (verkleinern), nie über den sicheren Default
        // hinaus lockern kann.
        self.max_output_bytes = limit.min(MAX_STREAM_OUTPUT_BYTES);
    }

    // `execute_with_stdin`: bewusst auf dem Trait-Default (delegiert an
    // `execute`, ignoriert `stdin`) — `sudo -S`-Stdin-Zufuhr ist für den
    // lokalen Pseudo-Server nicht relevant (kein hinterlegtes
    // Sudo-Passwort möglich, s. `crate::local`-Verwendung in `app-shell`),
    // `stdin` bleibt immer `Stdio::null()`.

    /// Spec 0027 / issue #324: abbrechbar wie der SSH-Pfad. Bei Abbruch wird
    /// die ganze Prozessgruppe des Kommandos beendet (s. [`terminate_tree`])
    /// und die bis dahin gesammelte Ausgabe mit `cancelled: true`
    /// zurückgegeben.
    async fn execute_cancellable(
        &mut self,
        command: &str,
        cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        self.run(command, Some(cancel)).await
    }

    /// Wie [`execute_cancellable`](Self::execute_cancellable); `stdin` wird
    /// wie bei `execute_with_stdin` ignoriert (`Stdio::null()` bleibt).
    async fn execute_with_stdin_cancellable(
        &mut self,
        command: &str,
        _stdin: &[u8],
        cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        self.run(command, Some(cancel)).await
    }

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
        Ok(Box::new(LocalFileSession::with_home(self.home.clone())))
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

    /// Wall-clock budget for the "`execute()` does not hang" tests
    /// (`test_t44_execute_caps_output_during_streaming`,
    /// `test_t44_execute_does_not_hang_on_a_command_waiting_for_stdin`).
    ///
    /// These tests only have to tell "finishes" apart from "never finishes":
    /// the regressions they guard against (inherited stdin, a flooding child
    /// that is never killed at the cap) block *forever*, not a few seconds
    /// longer. The budget still has to cover process spawn, shell start-up
    /// and the pipe read loop, which on a heavily loaded machine or CI runner
    /// (parallel `cargo test --workspace`, build load) has exceeded a tight
    /// 5 s margin even though nothing hung (#73). A generous margin removes
    /// those false failures without weakening what the tests prove.
    const NO_HANG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

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

    /// Issue #10: a local command runs in the home directory, not in the
    /// process cwd (the crate directory under `cargo test`) — proven via a
    /// marker file that exists only in the injected home.
    #[tokio::test]
    async fn test_execute_runs_in_home_directory() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("issue-10-marker.txt"), b"x").unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));
        #[cfg(unix)]
        let command = "ls";
        #[cfg(windows)]
        let command = "dir /b";

        let output = transport.execute(command).await.unwrap();

        let listing = String::from_utf8_lossy(&output.stdout);
        assert!(
            listing.contains("issue-10-marker.txt"),
            "command did not run in home: {listing}"
        );
        assert!(!listing.contains("Cargo.toml"), "ran in the cwd: {listing}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_execute_pwd_prints_home_directory() {
        let home = tempfile::tempdir().unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));

        let output = transport.execute("pwd -P").await.unwrap();

        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            home.path().canonicalize().unwrap().to_string_lossy()
        );
    }

    /// Issue #10: without a home, or with one that does not exist, local
    /// exec keeps the process cwd instead of failing the session.
    #[tokio::test]
    async fn test_execute_without_usable_home_falls_back_to_cwd() {
        let missing = tempfile::tempdir().unwrap().path().join("gone");
        for home in [None, Some(missing)] {
            let mut transport = LocalTransport::with_home(home);
            #[cfg(unix)]
            let command = "ls";
            #[cfg(windows)]
            let command = "dir /b";

            let output = transport.execute(command).await.unwrap();

            assert!(String::from_utf8_lossy(&output.stdout).contains("Cargo.toml"));
        }
    }

    /// Issue #23: a home path that exists but is a regular file is not a
    /// usable working directory — local exec keeps the process cwd instead
    /// of failing the spawn.
    #[tokio::test]
    async fn test_execute_with_home_that_is_a_file_falls_back_to_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let file_home = dir.path().join("home-is-a-file");
        std::fs::write(&file_home, b"x").unwrap();
        let mut transport = LocalTransport::with_home(Some(file_home));
        #[cfg(unix)]
        let command = "ls";
        #[cfg(windows)]
        let command = "dir /b";

        let output = transport.execute(command).await.unwrap();

        assert_eq!(output.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&output.stdout).contains("Cargo.toml"));
    }

    /// Issue #23: the non-blocking directory check matches `Path::is_dir`
    /// for an existing directory, a regular file and a missing path.
    #[tokio::test]
    async fn test_is_existing_dir_matches_path_is_dir() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        let missing = dir.path().join("missing");

        assert!(is_existing_dir(dir.path()).await);
        assert!(!is_existing_dir(&file).await);
        assert!(!is_existing_dir(&missing).await);
    }

    /// Issue #10: the SFTP session from `open_sftp()` uses the same home.
    #[tokio::test]
    async fn test_open_sftp_lists_home_for_dot() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("from-transport.txt"), b"x").unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));

        let mut sftp = transport.open_sftp().await.unwrap();
        let entries = sftp.list_dir(".").await.unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "from-transport.txt");
    }

    #[test]
    fn test_new_uses_the_user_home_directory() {
        assert_eq!(LocalTransport::new().home, user_home_dir());
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

        let output = tokio::time::timeout(NO_HANG_TIMEOUT, transport.execute(command))
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

        let output = tokio::time::timeout(NO_HANG_TIMEOUT, transport.execute("cat"))
            .await
            .expect("execute() darf bei einem auf stdin wartenden Kommando nicht hängen bleiben")
            .expect("execute() sollte trotz sofortigem EOF auf stdin Ok liefern");

        assert!(output.stdout.is_empty());
        assert_eq!(output.exit_code, Some(0));
    }

    // --- Issue #324: Abbruch auf dem lokalen Pseudo-Server (Spec 0027) ---

    /// Waits until `marker` exists (the command has reached the point where
    /// it creates it), so the cancel is sent while the command is really
    /// running, independent of machine load.
    async fn wait_for_marker(marker: &Path) {
        tokio::time::timeout(NO_HANG_TIMEOUT, async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the command never created its marker file");
    }

    /// Runs `command` via `execute_cancellable` in `home`, sends the cancel
    /// as soon as `home/marker` exists, and returns the outcome plus the time
    /// from cancel to return.
    async fn run_and_cancel_at_marker(
        transport: &mut LocalTransport,
        home: &Path,
        command: &str,
    ) -> (ExecOutcome, std::time::Duration) {
        let (tx, rx) = oneshot::channel();
        let marker = home.join("marker");
        let exec = async {
            let outcome = transport.execute_cancellable(command, rx).await;
            (outcome, std::time::Instant::now())
        };
        let canceller = async {
            wait_for_marker(&marker).await;
            let at = std::time::Instant::now();
            tx.send(())
                .expect("command already finished before the cancel");
            at
        };
        let ((outcome, finished_at), cancelled_at) =
            tokio::time::timeout(NO_HANG_TIMEOUT, async { tokio::join!(exec, canceller) })
                .await
                .expect("execute_cancellable ignored the cancel and kept running");
        (
            outcome.expect("a cancelled command is Ok, not an error"),
            finished_at.saturating_duration_since(cancelled_at),
        )
    }

    /// Whether process `pid` is still alive (a zombie only waits for its
    /// reaper and counts as gone).
    #[cfg(unix)]
    fn process_alive(pid: &str) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-A", "-o", "pid=", "-o", "stat="])
            .output()
            .expect("ps not available");
        String::from_utf8_lossy(&out.stdout).lines().any(|line| {
            let mut cols = line.split_whitespace();
            cols.next() == Some(pid) && !cols.next().unwrap_or("Z").starts_with('Z')
        })
    }

    /// Every pid the command printed (one per line on stdout/stderr) must be
    /// gone within 2 s of the cancel returning.
    #[cfg(unix)]
    async fn assert_printed_pids_gone(output: &CommandOutput, expected: usize) {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let pids: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(pids.len(), expected, "unexpected pid output {text:?}");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        for pid in pids {
            assert!(pid.parse::<u32>().is_ok(), "not a pid: {pid:?}");
            while process_alive(pid) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "process {pid} is still running after the cancel"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }

    /// Issue #324 regression test: on the old code (trait default, cancel
    /// ignored) this ran into the timeout, because `sleep 600` is waited for.
    /// Now the command ends within 2 s of the cancel, the partial output is
    /// kept and the outcome is marked cancelled without an exit code, as on
    /// the SSH path.
    #[tokio::test]
    async fn test_issue324_cancel_ends_long_running_local_command() {
        let home = tempfile::tempdir().unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));
        #[cfg(unix)]
        let command = "echo ready; touch marker; sleep 600";
        #[cfg(windows)]
        let command = "echo ready& type nul > marker& ping -n 600 127.0.0.1 > NUL";

        let (outcome, after_cancel) =
            run_and_cancel_at_marker(&mut transport, home.path(), command).await;

        assert!(outcome.cancelled, "the cancel must be reported");
        assert_eq!(outcome.output.exit_code, None);
        assert_eq!(
            String::from_utf8_lossy(&outcome.output.stdout).trim(),
            "ready"
        );
        assert!(!outcome.output.truncated);
        assert!(
            after_cancel < std::time::Duration::from_secs(2),
            "command ended {after_cancel:?} after the cancel"
        );
    }

    /// Issue #324: a pipeline is terminated completely — no member of the
    /// command's process group survives the cancel. The marker is created
    /// inside the pipeline, so both `sleep` and `cat` are running.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_issue324_cancel_terminates_whole_pipeline() {
        let home = tempfile::tempdir().unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));

        // `sleep 600 | cat`, with each side printing its pid before `exec`
        // (same process); the marker appears once both sides run.
        let (outcome, after_cancel) = run_and_cancel_at_marker(
            &mut transport,
            home.path(),
            "echo $$; \
             sh -c 'echo $$ >&2; touch left; exec sleep 600' | \
             sh -c 'echo $$ >&2; while [ ! -f left ]; do sleep 0.05; done; \
                    touch marker; exec cat'",
        )
        .await;

        assert!(outcome.cancelled);
        assert!(after_cancel < std::time::Duration::from_secs(2));
        // The outer shell, `sleep` and `cat`.
        assert_printed_pids_gone(&outcome.output, 3).await;
    }

    /// Issue #324: processes that ignore `SIGINT` (here the shell and its
    /// `sleep`, via `trap '' INT`; background jobs of a non-interactive
    /// shell behave the same) are killed after the grace period.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_issue324_cancel_kills_commands_that_ignore_sigint() {
        let home = tempfile::tempdir().unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));

        let (outcome, after_cancel) = run_and_cancel_at_marker(
            &mut transport,
            home.path(),
            "trap '' INT; echo $$; sleep 600 & echo $!; touch marker; wait",
        )
        .await;

        assert!(outcome.cancelled);
        assert!(after_cancel < std::time::Duration::from_secs(2));
        // The shell and the background `sleep`.
        assert_printed_pids_gone(&outcome.output, 2).await;
    }

    /// Issue #324 / Spec 0044: output written after the cancel (here by an
    /// `INT` trap) still goes through the output cap.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_issue324_output_cap_applies_to_partial_output_after_cancel() {
        let home = tempfile::tempdir().unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));
        transport.set_max_output_bytes(4);

        let (outcome, _) = run_and_cancel_at_marker(
            &mut transport,
            home.path(),
            "trap 'printf 0123456789; exit 1' INT; printf ab; touch marker; \
             while :; do sleep 1; done",
        )
        .await;

        assert!(outcome.cancelled);
        assert!(outcome.output.truncated, "the cap must have been hit");
        assert!(
            outcome.output.stdout.starts_with(b"ab01"),
            "unexpected partial output {:?}",
            String::from_utf8_lossy(&outcome.output.stdout)
        );
        assert!(outcome.output.stdout.len() <= 4 + crate::exec::TRUNCATION_NOTICE.len());
    }

    /// Issue #324: the stdin variant (sudo path) is cancellable too; `stdin`
    /// itself stays ignored for the local transport.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_issue324_execute_with_stdin_cancellable_honours_cancel() {
        let home = tempfile::tempdir().unwrap();
        let mut transport = LocalTransport::with_home(Some(home.path().to_path_buf()));
        let (tx, rx) = oneshot::channel();
        let marker = home.path().join("marker");

        let (outcome, ()) = tokio::time::timeout(NO_HANG_TIMEOUT, async {
            tokio::join!(
                transport.execute_with_stdin_cancellable(
                    "touch marker; sleep 600",
                    b"secret\n",
                    rx
                ),
                async {
                    wait_for_marker(&marker).await;
                    tx.send(()).unwrap();
                }
            )
        })
        .await
        .expect("execute_with_stdin_cancellable ignored the cancel");

        assert!(outcome.unwrap().cancelled);
    }

    /// Issue #324 / Spec 0027 §3.5: a command that finishes before any
    /// cancel returns its regular result; a cancel sent afterwards has no
    /// effect.
    #[tokio::test]
    async fn test_issue324_late_cancel_has_no_effect() {
        let mut transport = LocalTransport::new();
        let (tx, rx) = oneshot::channel();

        let outcome = tokio::time::timeout(
            NO_HANG_TIMEOUT,
            transport.execute_cancellable("echo done", rx),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(!outcome.cancelled);
        assert_eq!(outcome.output.exit_code, Some(0));
        assert_eq!(
            String::from_utf8_lossy(&outcome.output.stdout).trim(),
            "done"
        );
        assert!(tx.send(()).is_err(), "nobody listens for the late cancel");
    }

    /// Issue #324: a cancel sender that is dropped without sending is not a
    /// cancel — the command runs to its regular end.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_issue324_dropped_cancel_sender_does_not_cancel() {
        let mut transport = LocalTransport::new();
        let (tx, rx) = oneshot::channel::<()>();
        drop(tx);

        let outcome = tokio::time::timeout(
            NO_HANG_TIMEOUT,
            transport.execute_cancellable("echo a; sleep 0.2; echo b", rx),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(!outcome.cancelled);
        assert_eq!(outcome.output.exit_code, Some(0));
        assert_eq!(String::from_utf8_lossy(&outcome.output.stdout), "a\nb\n");
    }
}
