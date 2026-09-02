//! Risiko-Indikatoren für KI-vorgeschlagene Aktionen (Spec 0026) — rein
//! informativ, beeinflusst nie die Filter-Engine-`Decision`
//! (`crate::filter`). Zwei unabhängige Achsen (Server-Risiko/Daten-Risiko),
//! s. `types::RiskAssessment`.
//!
//! `ReadRemoteFile`/`WriteRemoteFile` (Spec 0020) laufen NICHT durch dieses
//! Modul in Form eines eigenen Pfad-Parameters — der Aufrufer (Kernschleife
//! in `app-tauri::orchestration`) mappt sie zuerst auf dieselben
//! `sftp-read <pfad>`/`sftp-write <pfad>`-Pseudokommandos, die bereits für
//! die Filter-Engine-Anbindung existieren (Spec 0020, Abschnitt 4.1), und
//! ruft dann [`RiskClassifier::classify`] wie für ein normales Kommando auf
//! — dieselbe Konvention, keine zweite Mapping-Logik in diesem Modul.

mod classifier;
mod patterns;
mod types;

#[cfg(test)]
mod tests;

pub use classifier::RuleBasedRiskClassifier;
pub use types::{RiskAssessment, RiskClassifier, RiskLevel};
