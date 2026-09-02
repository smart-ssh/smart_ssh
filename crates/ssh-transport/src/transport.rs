use async_trait::async_trait;
use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg};
use tokio::sync::oneshot;

use ssh_manager_core::ssh::{
    CommandOutput, ExecOutcome, InteractiveShell, PtySize, SftpSession, SshError, SshTransport,
};

use crate::error::map_russh_error;
use crate::exec::accumulate_exec_output;
use crate::handler::ClientHandler;
use crate::sftp::RusshSftpSession;
use crate::shell::RusshShell;

/// `russh`-gestützte Implementierung von `SshTransport` (Spec 0005,
/// Abschnitt 4).
pub struct RusshTransport {
    pub(crate) handle: Handle<ClientHandler>,
    /// Handles der Zwischen-Hops (alles außer dem letzten aus
    /// `ConnectionTarget::hops`, s. `crate::connect`). Werden selbst nie
    /// mehr direkt benutzt, müssen aber für die Lebensdauer dieses
    /// Transports am Leben gehalten werden — sonst bricht der darüber
    /// laufende `direct-tcpip`-Tunnel zum eigentlichen Ziel zusammen, wenn
    /// ein Zwischen-`Handle` gedroppt wird. Führendes `_`, damit Clippy
    /// dieses "nur am Leben halten"-Feld nicht als totes Feld anmahnt.
    pub(crate) _intermediate_hops: Vec<Handle<ClientHandler>>,
}

async fn drain_channel(mut channel: Channel<Msg>) -> Result<CommandOutput, SshError> {
    let mut messages = Vec::new();
    loop {
        match channel.wait().await {
            None => break,
            Some(msg) => {
                let is_close = matches!(msg, ChannelMsg::Close);
                messages.push(msg);
                if is_close {
                    break;
                }
            }
        }
    }
    Ok(accumulate_exec_output(messages))
}

/// Wie [`drain_channel`], aber bricht früh ab, sobald `cancel` auflöst
/// (Spec 0027) — für ein Kommando, das nie von selbst endet (`journalctl
/// -f`, `tail -f`, …). Liefert die bis dahin gesammelte Ausgabe mit
/// `cancelled: true` zurück statt weiter auf das reguläre Kanal-Ende zu
/// warten.
///
/// Best-effort-Abbruch in zwei Schritten: zuerst ein SSH-Channel-
/// Signal-Request (`Sig::INT`, RFC 4254 Abschnitt 6.9) — vom Server
/// **optional** unterstützt, OpenSSH liefert ihn beim Exec-Channel aber
/// üblicherweise an die Prozessgruppe des gestarteten Kommandos, ganz ohne
/// PTY. Danach in jedem Fall `eof()`/`close()`: selbst wenn der
/// Signal-Request ignoriert wurde, beendet das zuverlässig unser lokales
/// Warten, und die meisten CLI-Werkzeuge beenden sich zeitnah selbst,
/// sobald ihr nächster Schreibversuch auf die geschlossene Pipe mit
/// `SIGPIPE`/`EPIPE` fehlschlägt. Kein Schritt hiervon ist ein garantiertes
/// Töten des Remote-Prozesses (s. Spec 0027, Abschnitt 3).
async fn drain_channel_cancellable(
    mut channel: Channel<Msg>,
    mut cancel: oneshot::Receiver<()>,
) -> Result<ExecOutcome, SshError> {
    let mut messages = Vec::new();
    loop {
        tokio::select! {
            _ = &mut cancel => {
                let _ = channel.signal(russh::Sig::INT).await;
                let _ = channel.eof().await;
                let _ = channel.close().await;
                return Ok(ExecOutcome {
                    output: accumulate_exec_output(messages),
                    cancelled: true,
                });
            }
            msg = channel.wait() => {
                match msg {
                    None => break,
                    Some(msg) => {
                        let is_close = matches!(msg, ChannelMsg::Close);
                        messages.push(msg);
                        if is_close {
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(ExecOutcome {
        output: accumulate_exec_output(messages),
        cancelled: false,
    })
}

#[async_trait]
impl SshTransport for RusshTransport {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel.exec(true, command).await.map_err(map_russh_error)?;
        drain_channel(channel).await
    }

    /// Spec 0018, Abschnitt 5: identisch zu `execute`, schreibt `stdin`
    /// aber in den Channel und signalisiert danach EOF, bevor auf die
    /// Antwort gewartet wird — nötig, damit z. B. `sudo -S` das über Stdin
    /// gelieferte Passwort tatsächlich liest, statt (mangels TTY) auf einen
    /// nie eintreffenden Prompt zu warten.
    async fn execute_with_stdin(
        &mut self,
        command: &str,
        stdin: &[u8],
    ) -> Result<CommandOutput, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel.exec(true, command).await.map_err(map_russh_error)?;
        if !stdin.is_empty() {
            channel
                .data_bytes(stdin.to_vec())
                .await
                .map_err(map_russh_error)?;
        }
        channel.eof().await.map_err(map_russh_error)?;
        drain_channel(channel).await
    }

    async fn execute_cancellable(
        &mut self,
        command: &str,
        cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel.exec(true, command).await.map_err(map_russh_error)?;
        drain_channel_cancellable(channel, cancel).await
    }

    async fn execute_with_stdin_cancellable(
        &mut self,
        command: &str,
        stdin: &[u8],
        cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel.exec(true, command).await.map_err(map_russh_error)?;
        if !stdin.is_empty() {
            channel
                .data_bytes(stdin.to_vec())
                .await
                .map_err(map_russh_error)?;
        }
        channel.eof().await.map_err(map_russh_error)?;
        drain_channel_cancellable(channel, cancel).await
    }

    async fn open_shell(&mut self, size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel
            .request_pty(
                true,
                "xterm-256color",
                u32::from(size.cols),
                u32::from(size.rows),
                0,
                0,
                &[],
            )
            .await
            .map_err(map_russh_error)?;
        channel.request_shell(true).await.map_err(map_russh_error)?;
        Ok(Box::new(RusshShell { channel }))
    }

    /// Spec 0020, Abschnitt 3: `sftp`-Subsystem-Channel derselben
    /// Verbindung — kein zweiter Verbindungsaufbau, keine erneute Auth/
    /// Host-Key-Prüfung.
    async fn open_sftp(&mut self) -> Result<Box<dyn SftpSession>, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(map_russh_error)?;
        let client = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| SshError::ChannelError(format!("SFTP-Init fehlgeschlagen: {e}")))?;
        Ok(Box::new(RusshSftpSession::new(client)))
    }

    async fn disconnect(&mut self) -> Result<(), SshError> {
        self.handle
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await
            .map_err(map_russh_error)
    }
}
