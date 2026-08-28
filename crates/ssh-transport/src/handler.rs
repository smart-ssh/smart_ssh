use russh::keys::PublicKeyOrCertificate;
use ssh_manager_core::ssh::HostKeyStore;
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
pub(crate) struct ClientHandler {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) host_keys: Arc<dyn HostKeyStore>,
}

impl russh::client::Handler for ClientHandler {
    type Error = TransportError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let raw_key = public_key_bytes(server_public_key)?;
        evaluate_host_key(self.host_keys.as_ref(), &self.host, self.port, raw_key)
    }
}
