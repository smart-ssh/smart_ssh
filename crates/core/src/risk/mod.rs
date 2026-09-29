//! Risiko-Indikatoren für KI-vorgeschlagene Aktionen (Spec 0026) — beeinflusst
//! innerhalb DIESES Crates nie die Filter-Engine-`Decision` (`crate::filter`):
//! die Einschätzung wird hier nur berechnet, nicht ausgewertet. Zwei
//! unabhängige Achsen (Server-Risiko/Daten-Risiko), s.
//! `types::RiskAssessment`.
//!
//! **Seit Spec 0092 kein rein informativer Wert mehr auf App-Ebene:**
//! `app-logic::orchestration::action_exec` liest die hier berechnete
//! Einschätzung und macht bei `Red` auf einer der beiden Achsen (Einstellung
//! „Bei rotem Risiko immer nachfragen" vorausgesetzt) aus einer sonst
//! automatisch laufenden Aktion eine bestätigungspflichtige — auch gegen
//! eine Allow-Regel. S. `docs/adr/0084-red-risk-requires-confirm.md`.
//!
//! `ReadRemoteFile`/`WriteRemoteFile` (Spec 0020) laufen NICHT durch dieses
//! Modul in Form eines eigenen Pfad-Parameters — der Aufrufer (Kernschleife
//! in `app-shell::orchestration`) mappt sie zuerst auf dieselben
//! `sftp-read <pfad>`/`sftp-write <pfad>`-Pseudokommandos, die bereits für
//! die Filter-Engine-Anbindung existieren (Spec 0020, Abschnitt 4.1), und
//! ruft dann [`RiskClassifier::classify`] wie für ein normales Kommando auf
//! — dieselbe Konvention, keine zweite Mapping-Logik in diesem Modul.

mod classifier;
mod patterns;
mod types;

#[cfg(test)]
mod tests;

pub use classifier::{
    secret_path_read_reason, sftp_server_invocation_reason, RuleBasedRiskClassifier,
};
pub use types::{RiskAssessment, RiskClassifier, RiskLevel};
