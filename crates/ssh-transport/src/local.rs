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
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use portable_pty::{native_pty_system, CommandBuilder, PtySize as PortablePtySize};
use tokio::process::Command;

use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize, SftpSession, SshError, SshTransport};

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
pub struct LocalTransport;

impl LocalTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocalTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SshTransport for LocalTransport {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        let output = shell_command(command)
            .output()
            .await
            .map_err(|e| io_err("lokale Ausführung fehlgeschlagen", e))?;
        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code(),
        })
    }

    // `execute_with_stdin`/`execute_cancellable`/`execute_with_stdin_cancellable`:
    // bewusst auf den Trait-Default belassen (delegiert an `execute`, ohne
    // `stdin`/`cancel` zu berücksichtigen). Ein "echter" Abbruch wäre hier
    // durch `Child::kill()` sogar zuverlässiger möglich als bei SSH (Spec
    // 0027, dort nur Best-effort über ein optionales Signal) — für den
    // ersten Schritt aber bewusst nicht umgesetzt, um den Umfang klein zu
    // halten; `sudo -S`-Stdin-Zufuhr ist für den lokalen Pseudo-Server
    // ohnehin nicht relevant (kein hinterlegtes Sudo-Passwort möglich, s.
    // `crate::local`-Verwendung in `app-tauri`).

    async fn open_shell(&mut self, size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PortablePtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| SshError::ChannelError(format!("lokales PTY konnte nicht geöffnet werden: {e}")))?;

        let child = pair
            .slave
            .spawn_command(default_shell_command())
            .map_err(|e| SshError::ChannelError(format!("lokale Shell konnte nicht gestartet werden: {e}")))?;
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

    /// Spec 0032, Abschnitt 8: `LocalTransport::execute()` liefert
    /// korrekten stdout/stderr/exit-code für ein einfaches Testkommando.
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
}

