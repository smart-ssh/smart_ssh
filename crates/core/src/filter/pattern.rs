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
}
