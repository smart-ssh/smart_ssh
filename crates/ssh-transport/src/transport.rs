use async_trait::async_trait;
use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg};
use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize, SshError, SshTransport};

use crate::error::map_russh_error;
use crate::exec::accumulate_exec_output;
use crate::handler::ClientHandler;
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

    async fn disconnect(&mut self) -> Result<(), SshError> {
        self.handle
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await
            .map_err(map_russh_error)
    }
}
