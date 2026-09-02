use serde::{Deserialize, Serialize};

/// Risiko-Stufe auf einer der beiden Achsen (Spec 0026, Abschnitt 2). Kein
/// drittes "grün" — Abwesenheit eines Badges im UI bedeutet bereits
/// "laut bekannten Mustern unauffällig" (s. Spec, Abschnitt 1).
///
/// Deklarationsreihenfolge ist absichtlich `None < Yellow < Red` (abgeleitetes
/// `Ord`) — Abschnitt 3 verlangt `max(regelbasiert, KI-Ergebnis)` für die
/// Eskalationslogik, das nutzt genau diese Ordnung direkt über `.max()`,
/// ohne eigene Vergleichslogik.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    None,
    Yellow,
    Red,
}

/// Rein informative Risiko-Einschätzung einer KI-vorgeschlagenen Aktion
/// (Spec 0026, Abschnitt 1/2) — beeinflusst nie die `Decision` der
/// Filter-Engine (`core::filter`), blockiert nichts, führt nichts aus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskAssessment {
    pub server_risk: RiskLevel,
    /// `None`, solange `server_risk == RiskLevel::None` — welches Muster
    /// gegriffen hat, sonst nichts zu begründen.
    pub server_risk_reason: Option<String>,
    pub data_risk: RiskLevel,
    /// Wie `server_risk_reason`, kann aber nach der optionalen
    /// KI-Zweitmeinung (Abschnitt 3) auf deren Begründung überschrieben
    /// werden, falls sie das regelbasierte Ergebnis anhebt.
    pub data_risk_reason: Option<String>,
    /// `true`, sobald die optionale KI-Zweitmeinung (nur Daten-Risiko-Achse,
    /// Abschnitt 3) tatsächlich eingeflossen ist — unterscheidet "noch nicht
    /// abgefragt" von "abgefragt, aber ergebnislos" für die UI (Abschnitt 4:
    /// Lade-Indikator, solange eine aktivierte Zweitmeinung noch aussteht).
    pub ai_reviewed: bool,
}

/// Klassifiziert ein Kommando (bzw. den auf ein Pseudokommando gemappten
/// Dateipfad, s. Modul-Doc) in eine [`RiskAssessment`] (Spec 0026,
/// Abschnitt 2). Synchron — die regelbasierte Prüfung ist rein lokal
/// (Pattern-Matching gegen fest codierte Listen) und schnell genug, um das
/// `chat-action-proposed`-Event nicht zu verzögern (Abschnitt 2, Punkt 5
/// der Aufgabenstellung).
pub trait RiskClassifier: Send + Sync {
    fn classify(&self, command: &str) -> RiskAssessment;
}
