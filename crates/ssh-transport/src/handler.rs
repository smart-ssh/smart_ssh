use russh::keys::{HashAlg, PublicKeyOrCertificate};
use ssh_manager_core::ssh::connect_log::{
    sanitize_server_version, HOST_KEY_CHANGED, HOST_KEY_UNKNOWN,
};
use ssh_manager_core::ssh::{
    ConnectLog, ConnectStep, ConnectionLoss, HostKeyCheckResult, HostKeyDecision, HostKeyStore,
    StepId,
};
use std::sync::Arc;

use crate::error::TransportError;
use crate::host_key::{evaluate_host_key, public_key_bytes};

/// `russh::client::Handler`-Implementierung für einen einzelnen Hop.
///
/// Hält `host`/`port` (für die `HostKeyStore`-Prüfung, die selbst keinen
/// Kontext darüber hat, mit welchem Server gerade gesprochen wird) sowie
/// einen `Arc<dyn HostKeyStore>` — **nicht** `&dyn HostKeyStore` wie in der
/// Spec-Signatur von `connect()` (Abschnitt 4/6) vorgeschlagen: der
/// `Handler`-Trait verlangt `Self: 'static` (er wird in einen gespawnten
/// Tokio-Task verschoben), eine geliehene Referenz mit der Lebensdauer des
/// `connect()`-Aufrufs kann diese Anforderung nicht erfüllen. Siehe
/// ADR-Vorschlag in der Abschluss-Nachricht.
///
/// Issue #51: `steps` zeichnet Handshake und Host-Key-Prüfung dieses Hops
/// auf. Nur Beobachtung — die Entscheidung in `check_server_key` ist
/// unverändert die von [`evaluate_host_key`], ihr Ergebnis geht wörtlich an
/// `russh` zurück.
pub(crate) struct ClientHandler {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) host_keys: Arc<dyn HostKeyStore>,
    pub(crate) steps: HopSteps,
    /// Issue #353: shared by all hops of one connection; set when any hop's
    /// session ends.
    pub(crate) loss: Arc<tokio::sync::watch::Sender<Option<ConnectionLoss>>>,
}

/// Aufzeichnungszustand eines Hops im Handler.
pub(crate) struct HopSteps {
    log: ConnectLog,
    hop_index: usize,
    hop: String,
    /// Der Handshake-Schritt, begonnen unmittelbar vor `connect_stream`.
    handshake: StepId,
    host_key: Option<StepId>,
    /// `kex_done` läuft auch bei jedem späteren Schlüsselwechsel — nur der
    /// erste gehört zum Verbindungsaufbau.
    initial_kex_seen: bool,
}

impl HopSteps {
    /// Beginnt den Handshake-Schritt dieses Hops.
    pub(crate) fn start(log: &ConnectLog, hop_index: usize, hop: &str) -> Self {
        let handshake = log.start(
            hop_index,
            hop,
            ConnectStep::Handshake {
                server_version: None,
                kex: None,
                host_key_algorithm: None,
                cipher: None,
                mac: None,
            },
        );
        Self {
            log: log.clone(),
            hop_index,
            hop: hop.to_string(),
            handshake,
            host_key: None,
            initial_kex_seen: false,
        }
    }

    fn host_key_step(&mut self) -> StepId {
        let (log, hop_index, hop) = (&self.log, self.hop_index, &self.hop);
        *self.host_key.get_or_insert_with(|| {
            log.start(
                hop_index,
                hop,
                ConnectStep::HostKeyCheck {
                    key_type: None,
                    fingerprint: None,
                    result: None,
                },
            )
        })
    }
}

impl russh::client::Handler for ClientHandler {
    type Error = TransportError;

    /// Issue #353: record that the session ended (also for keepalive
    /// timeouts) before the default handling runs.
    async fn disconnected(
        &mut self,
        reason: russh::client::DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        let text = loss_reason(&reason);
        // `send_if_modified`: the first loss wins; works without receivers.
        self.loss.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(ConnectionLoss { reason: text });
            true
        });
        match reason {
            russh::client::DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            russh::client::DisconnectReason::Error(e) => Err(e),
        }
    }

    /// Issue #51: nur die Namen der ausgehandelten Algorithmen und die
    /// Versionskennung des Servers werden gelesen. Das gemeinsame Geheimnis
    /// des Schlüsselaustauschs wird nicht angefasst.
    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        if self.steps.initial_kex_seen {
            return Ok(());
        }
        self.steps.initial_kex_seen = true;
        let version = sanitize_server_version(session.remote_sshid());
        self.steps.log.update(self.steps.handshake, |step| {
            if let ConnectStep::Handshake {
                server_version,
                kex,
                host_key_algorithm,
                cipher,
                mac,
            } = step
            {
                *server_version = Some(version);
                *kex = Some(names.kex.as_ref().to_string());
                *host_key_algorithm = Some(names.key.as_str().to_string());
                *cipher = Some(names.cipher.as_ref().to_string());
                *mac = Some(names.client_mac.as_ref().to_string());
            }
        });
        self.steps.log.succeed(self.steps.handshake);
        self.steps.host_key_step();
        Ok(())
    }

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let raw_key = public_key_bytes(server_public_key)?;
        let public_key = server_public_key.public_key();
        let key_type = public_key.algorithm().as_str().to_string();
        let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();

        let decision = evaluate_host_key(self.host_keys.as_ref(), &self.host, self.port, raw_key);

        let (result, failure) = match &decision {
            Ok(true) => (Some(HostKeyCheckResult::Known), None),
            Err(TransportError::HostKey {
                decision: HostKeyDecision::Unknown { .. },
                ..
            }) => (Some(HostKeyCheckResult::Unknown), Some(HOST_KEY_UNKNOWN)),
            Err(TransportError::HostKey {
                decision: HostKeyDecision::Mismatch { .. },
                ..
            }) => (Some(HostKeyCheckResult::Changed), Some(HOST_KEY_CHANGED)),
            // Heute nicht erreichbar (`evaluate_host_key` liefert nie
            // `Ok(false)`). Der Schritt bleibt dann offen und wird vom
            // Aufrufer mit dem Fehlercode des Versuchs geschlossen.
            _ => (None, None),
        };
        let step = self.steps.host_key_step();
        self.steps.log.update(step, |s| {
            if let ConnectStep::HostKeyCheck {
                key_type: kt,
                fingerprint: fp,
                result: r,
            } = s
            {
                *kt = Some(key_type);
                *fp = Some(fingerprint);
                *r = result;
            }
        });
        match (result, failure) {
            (Some(_), None) => self.steps.log.succeed(step),
            (Some(_), Some(code)) => self.steps.log.fail(step, code),
            _ => {}
        }

        decision
    }
}

/// Issue #353: user-facing reason for a session end. `None` for a clean
/// disconnect message.
pub(crate) fn loss_reason(
    reason: &russh::client::DisconnectReason<TransportError>,
) -> Option<String> {
    match reason {
        russh::client::DisconnectReason::ReceivedDisconnect(_) => None,
        russh::client::DisconnectReason::Error(TransportError::Russh(
            russh::Error::KeepaliveTimeout,
        )) => Some("The server stopped responding".to_string()),
        russh::client::DisconnectReason::Error(_) => Some("The connection was lost".to_string()),
    }
}

#[cfg(test)]
mod loss_tests {
    use super::*;
    use russh::client::DisconnectReason;

    #[test]
    fn keepalive_timeout_says_the_server_stopped_responding() {
        let reason = DisconnectReason::Error(TransportError::Russh(russh::Error::KeepaliveTimeout));
        assert_eq!(
            loss_reason(&reason).as_deref(),
            Some("The server stopped responding")
        );
    }

    #[test]
    fn other_errors_get_a_generic_reason() {
        let err = DisconnectReason::Error(TransportError::Russh(russh::Error::Disconnect));
        assert_eq!(
            loss_reason(&err).as_deref(),
            Some("The connection was lost")
        );
    }
}
