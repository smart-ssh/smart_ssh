use async_trait::async_trait;
use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use ssh_manager_core::ssh::{InteractiveShell, PtySize, SshError};

use crate::error::map_russh_error;

/// PTY-Shell für den interaktiven Modus (Spec 0005, Abschnitt 4).
///
/// Implementiert `core::ssh::InteractiveShell` — der `core`-Trait nutzt
/// `async-trait` (Boxing, damit er als `Box<dyn InteractiveShell>` nutzbar
/// bleibt), daher hier ebenfalls `#[async_trait]` auf dem Impl-Block. Das
/// ist bewusst anders als bei `russh::client::Handler` (s. `crate::handler`,
/// `crate::server` in den Integrationstests): `russh`s eigene Traits nutzen
/// natives `async fn` in Traits (kein Boxing nötig, da `russh` sie nicht als
/// Trait-Objekt verwendet) — beide Stile koexistieren in dieser Crate, je
/// nachdem, wessen Trait gerade implementiert wird.
pub(crate) struct RusshShell {
    pub(crate) channel: Channel<Msg>,
}

#[async_trait]
impl InteractiveShell for RusshShell {
    async fn write(&mut self, data: &[u8]) -> Result<(), SshError> {
        self.channel
            .data_bytes(data.to_vec())
            .await
            .map_err(map_russh_error)
    }

    async fn read(&mut self) -> Result<Vec<u8>, SshError> {
        loop {
            match self.channel.wait().await {
                Some(ChannelMsg::Data { data }) => return Ok(data.to_vec()),
                Some(ChannelMsg::ExtendedData { data, .. }) => return Ok(data.to_vec()),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => return Ok(Vec::new()),
                // Andere Channel-Nachrichten (z. B. ExitStatus) sind für den
                // rohen Tastatur-/Bildschirm-Stream irrelevant — überspringen
                // und auf die nächste Nachricht warten.
                Some(_) => continue,
            }
        }
    }

    async fn resize(&mut self, size: PtySize) -> Result<(), SshError> {
        self.channel
            .window_change(u32::from(size.cols), u32::from(size.rows), 0, 0)
            .await
            .map_err(map_russh_error)
    }
}
