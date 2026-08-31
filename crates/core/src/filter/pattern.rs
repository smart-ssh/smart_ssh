use globset::Glob;
use regex::Regex;

use super::types::Pattern;

impl Pattern {
    /// Prüft, ob `cmd` (bereits whitespace-normalisiert) auf dieses Muster
    /// passt.
    ///
    /// Bewusst `pub(super)` statt `pub`: Matching-Semantik ist ein internes
    /// Detail der Engine (Abschnitt 2/5 der Spec beschreiben `Pattern` nur
    /// als Datentyp, nicht als Aufrufer-API), keine Garantie für Code
    /// außerhalb von `filter`.
    ///
    /// Ein syntaktisch ungültiges Glob-/Regex-Muster matcht nie (statt zu
    /// panicken) — eine kaputt konfigurierte Allow-Regel darf niemals
    /// versehentlich zu AutoExec führen, sondern soll folgenlos durchfallen
    /// (fail-safe defaults, Spec Abschnitt 1).
    pub(super) fn matches(&self, cmd: &str) -> bool {
        match self {
            Pattern::Exact(expected) => expected == cmd,
            Pattern::Glob(pattern) => Glob::new(pattern)
                .map(|glob| glob.compile_matcher().is_match(cmd))
                .unwrap_or(false),
            Pattern::Regex(pattern) => Regex::new(pattern)
                .map(|re| re.is_match(cmd))
                .unwrap_or(false),
        }
    }

    /// Rohes Musterliteral, unabhängig von der Variante — für Anzeigezwecke
    /// (Hard-Blacklist-Liste, `EvaluationTrace::matched_hard_blacklist_entry`,
    /// Spec 0009 Abschnitt 4/6). Anders als [`Pattern::matches`] absichtlich
    /// `pub`: reiner Lesezugriff auf den Musterinhalt, kein Teil der
    /// Matching-Semantik, die Aufrufer außerhalb von `filter` nicht kennen
    /// sollen.
    pub fn display_text(&self) -> &str {
        match self {
            Pattern::Exact(s) | Pattern::Glob(s) | Pattern::Regex(s) => s,
        }
    }

    /// Kurzbezeichner der Variante (`"glob"`/`"regex"`/`"exact"`) — für DTOs
    /// außerhalb von `core`, die Typ und Wert getrennt darstellen wollen
    /// (Spec 0009, `RuleInput.pattern_type`).
    pub fn kind_str(&self) -> &'static str {
        match self {
            Pattern::Glob(_) => "glob",
            Pattern::Regex(_) => "regex",
            Pattern::Exact(_) => "exact",
        }
    }
}
