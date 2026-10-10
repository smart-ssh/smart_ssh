use super::error::SshError;
use super::types::HostKeyDecision;

/// Speicher bekannter Host-Keys, Trust-on-First-Use (Spec 0005, Abschnitt
/// 6). Kein automatisches Akzeptieren unbekannter oder geänderter Keys —
/// die Entscheidung, wie mit `Unknown`/`Mismatch` umgegangen wird, liegt
/// beim Aufrufer (UI-Bestätigungsdialog), nicht bei diesem Trait.
///
/// Bewusst synchron (kein `async_trait`), wie in der Spec vorgegeben — die
/// konkrete Speicherung (eigene Tabelle in `persistence-sqlite` oder
/// `known_hosts`-Datei) ist explizit nicht Teil dieser Spec (Abschnitt 6),
/// dieser Trait modelliert nur die reine Entscheidungslogik.
pub trait HostKeyStore: Send + Sync {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision;
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError>;

    /// Read-only listing of the stored keys for `(host, port)`, for display
    /// only (algorithm + fingerprint, never the raw key). Never changes
    /// the trust state. Default: nothing listed.
    fn stored_keys(&self, _host: &str, _port: u16) -> Vec<StoredHostKeyInfo> {
        Vec::new()
    }
}

/// Display-only description of one stored host key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredHostKeyInfo {
    /// SSH algorithm name (e.g. `ssh-ed25519`), or `unknown` when the
    /// stored blob has no parseable algorithm.
    pub algorithm: String,
    /// OpenSSH format `SHA256:<base64>`.
    pub fingerprint: String,
}
