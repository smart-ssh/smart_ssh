use russh::keys::PublicKeyOrCertificate;
use ssh_manager_core::ssh::HostKeyStore;

use crate::error::TransportError;

/// Extrahiert die rohen Public-Key-Bytes aus einem `PublicKeyOrCertificate`
/// (russh übergibt diesen Typ an `check_server_key`, unabhängig davon, ob
/// der Server einen einfachen Host-Key oder ein Zertifikat präsentiert) —
/// für [`HostKeyStore::check`], das laut Spec 0005 Abschnitt 6 einen
/// `&[u8]`-Schlüssel erwartet.
pub(crate) fn public_key_bytes(key: &PublicKeyOrCertificate) -> Result<Vec<u8>, TransportError> {
    // `PublicKeyOrCertificate::public_key()` ist eine von `russh` selbst
    // bereitgestellte Convenience-Methode, die beide Varianten einheitlich
    // auf einen `ssh_key::PublicKey` abbildet (beim Zertifikat über
    // `PublicKey::new(cert.public_key().clone(), "")`) — kein Grund, das
    // hier manuell nachzubauen.
    key.public_key().to_bytes().map_err(TransportError::from)
}

/// Prüft einen präsentierten Host-Key gegen den [`HostKeyStore`] und
/// entscheidet, ob der Handshake fortgesetzt werden darf.
///
/// `Trusted` → `Ok(true)` (Handshake läuft weiter). `Unknown`/`Mismatch` →
/// `Err(TransportError::HostKey{..})` statt `Ok(false)`: die reine
/// `bool`-Rückgabe der `check_server_key`-Callback-API von `russh` kann die
/// *Art* der Ablehnung nicht transportieren, der Fehlerkanal (`Self::Error`)
/// dagegen schon — s. Doc-Kommentar bei [`TransportError`].
pub(crate) fn evaluate_host_key(
    host_keys: &dyn HostKeyStore,
    host: &str,
    port: u16,
    raw_key: Vec<u8>,
) -> Result<bool, TransportError> {
    use ssh_manager_core::ssh::HostKeyDecision;

    match host_keys.check(host, port, &raw_key) {
        HostKeyDecision::Trusted => Ok(true),
        decision @ (HostKeyDecision::Unknown { .. } | HostKeyDecision::Mismatch { .. }) => {
            Err(TransportError::HostKey { raw_key, decision })
        }
    }
}
