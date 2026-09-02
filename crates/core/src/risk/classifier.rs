use crate::filter::{segment_command, Pattern};

use super::patterns::{data_risk_patterns, server_risk_patterns};
use super::types::{RiskAssessment, RiskClassifier, RiskLevel};

/// Regelbasierte Umsetzung von [`RiskClassifier`] (Spec 0026, Abschnitt 2) —
/// zustandslos, hält keine eigenen Daten (die Musterlisten sind
/// modulweite `OnceLock`s, s. `patterns.rs`), deshalb kein Feld nötig.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuleBasedRiskClassifier;

impl RiskClassifier for RuleBasedRiskClassifier {
    /// Zerlegt `command` mit derselben Logik wie die Filter-Engine
    /// (`crate::filter::segment_command`, Spec 0026 Abschnitt 2: "nutzt
    /// exakt dieselbe Logik wie die Filter-Engine"), klassifiziert jedes
    /// Teilkommando einzeln gegen beide Musterlisten und behält je Achse
    /// das höchste gefundene Level.
    fn classify(&self, command: &str) -> RiskAssessment {
        let mut segments = segment_command(command);
        // Zusätzlich das unzerlegte Gesamtkommando prüfen: `scan_top_level_
        // segments` (`filter::parser`) verfolgt nur `(`/`)`, keine `{`/`}` —
        // ein Muster wie die klassische Fork-Bombe `:(){ :|:& };:`, dessen
        // `|`/`;` innerhalb der `{}`-Klammern liegen, wird deshalb an
        // genau diesen Zeichen mit-aufgetrennt, obwohl es fachlich ein
        // einzelnes Kommando ist. Ein per-Segment-Match allein würde ein
        // extra/exakt formuliertes Muster für so einen Fall daher nie
        // treffen; das volle Kommando zusätzlich zu prüfen fängt das aber
        // ohne eine (hier nicht gewollte) Änderung an `filter::parser`
        // selbst auf.
        segments.push(command.to_lowercase());

        let mut server_risk = RiskLevel::None;
        let mut server_risk_reason: Option<&'static str> = None;
        let mut data_risk = RiskLevel::None;
        let mut data_risk_reason: Option<&'static str> = None;

        for segment in &segments {
            // Wie die Hard-Blacklist der Filter-Engine (s.
            // `filter::blacklist`-Modul-Kommentar) case-insensitiv über
            // Lowercasing statt eines `case_insensitive`-Glob-Builders —
            // dieselbe, bereits etablierte Konvention.
            let lower = segment.to_lowercase();

            if let Some((level, reason)) = best_match(server_risk_patterns(), &lower) {
                if level > server_risk {
                    server_risk = level;
                    server_risk_reason = Some(reason);
                }
            }
            if let Some((level, reason)) = best_match(data_risk_patterns(), &lower) {
                if level > data_risk {
                    data_risk = level;
                    data_risk_reason = Some(reason);
                }
            }
        }

        RiskAssessment {
            server_risk,
            server_risk_reason: server_risk_reason.map(str::to_string),
            data_risk,
            data_risk_reason: data_risk_reason.map(str::to_string),
            ai_reviewed: false,
        }
    }
}

/// Höchstes unter allen zutreffenden Mustern — ein Teilkommando kann
/// gleichzeitig ein Rot- und ein Gelb-Muster treffen (z. B. `rm -rf
/// /etc/shadow` träfe sowohl "rm -rf" als auch "shadow"), das strengere
/// Ergebnis soll gewinnen, nicht das zuerst in der Liste stehende.
fn best_match(
    patterns: &[(Pattern, RiskLevel, &'static str)],
    lower_segment: &str,
) -> Option<(RiskLevel, &'static str)> {
    patterns
        .iter()
        .filter(|(pattern, _, _)| pattern.matches(lower_segment))
        .map(|(_, level, reason)| (*level, *reason))
        .max_by_key(|(level, _)| *level)
}
