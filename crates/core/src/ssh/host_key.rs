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
}
