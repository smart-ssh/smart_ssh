use crate::filter::{
    resolve_effective_command, segment_command, Pattern, DEFAULT_MAX_COMMAND_LENGTH,
};

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
        // Unabhängiger Review-Pass (Spec 0026): `segment_command` rekursiert
        // pro `$(...)`-Verschachtelungsebene ohne jede Tiefen-/Längenschranke
        // (anders als die Filter-Engine, die `DEFAULT_MAX_COMMAND_LENGTH`
        // bereits VOR jedem Parsing prüft, `filter::engine`) — ein
        // KI-vorgeschlagenes Kommando mit tausenden verschachtelten `$(`
        // bringt sonst den GESAMTEN Prozess per Stack-Overflow zum Absturz
        // (empirisch verifiziert), nicht nur die Session, noch bevor
        // überhaupt ein Bestätigungsdialog erscheint — erreichbar über einen
        // kompromittierten/prompt-injizierten KI-Provider oder MCP. Dieselbe
        // Schranke wie die Filter-Engine reicht laut Messung sicher aus; die
        // eigentliche Policy-Entscheidung für zu lange Kommandos trifft
        // ohnehin bereits die Filter-Engine (`FILTER_COMMAND_TOO_LONG`) —
        // hier zählt nur "nicht abstürzen", ein unklassifiziertes Ergebnis
        // ist ein akzeptabler Fail-safe.
        if command.len() > DEFAULT_MAX_COMMAND_LENGTH {
            return RiskAssessment {
                server_risk: RiskLevel::None,
                server_risk_reason: None,
                data_risk: RiskLevel::None,
                data_risk_reason: None,
                ai_reviewed: false,
            };
        }

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
        // Unabhängiger Review-Pass (Spec 0026): ohne dieselbe Normalisierung,
        // die die Filter-Engine für ihre eigene Hard-Blacklist anwendet
        // (`resolve_effective_command` — wiederholtes Entfernen von
        // `sudo`/`doas`/Wrapper-Präfixen und Variablen-Zuweisungen), sind
        // fast alle Risiko-Muster durch ein vorangestelltes `sudo`/`env`/
        // `bash -c` wirkungslos, weil sie am Anfang verankert sind (z. B.
        // die Daten-Risiko-Regexes `^(?:cat|less|head|tail|...)`,
        // Server-Risiko-Muster wie `shutdown*`/`kill *`) — empirisch
        // verifiziert: `sudo cat /etc/shadow` klassifizierte zuvor als "kein
        // Risiko" statt "Daten Rot", obwohl genau das die praktisch
        // häufigere UND gefährlichere Form ist. Zusätzlich zum rohen Segment
        // auch die aufgelöste Form prüfen — dasselbe Dual-Text-Muster wie
        // ADR 0002, hier für die Risiko-Einschätzung statt für Regeln.
        let resolved: Vec<String> = segments
            .iter()
            .map(|segment| resolve_effective_command(segment))
            .collect();
        segments.extend(resolved);

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
