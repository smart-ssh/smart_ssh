//! Filter-/Policy-Engine.
//!
//! Setzt `docs/specs/0002-filter-engine-spec.md` um: entscheidet für jedes
//! (KI-vorgeschlagene) Kommando, ob es automatisch ausgeführt werden darf
//! (`AutoExec`), eine Nutzerbestätigung braucht (`Confirm`) oder komplett
//! verweigert wird (`Deny`). Siehe die Spec für die vollständige
//! Präzedenz-Kette (Abschnitt 3) und den Test-Katalog (Abschnitt 6).

mod blacklist;
mod engine;
mod parser;
mod pattern;
mod types;

#[cfg(test)]
mod tests;

pub use engine::{scope_applies, FilterEngine, PolicyStore, DEFAULT_MAX_COMMAND_LENGTH};
pub use types::{
    Decision, EffectiveScope, EvalContext, EvaluationTrace, Pattern, Rule, RuleAction, RuleId,
    Scope,
};
// Spec 0026, Abschnitt 2: von `crate::risk` wiederverwendet, s.
// `parser::segment_command`-Doc-Kommentar. `pub(crate)` statt `pub` — bleibt
// ein internes Detail zwischen den beiden Modulen dieser Crate, keine
// Garantie für Aufrufer außerhalb.
pub(crate) use parser::segment_command;
// Unabhängiger Review-Pass, Spec 0026: ebenfalls von `crate::risk` wieder-
// verwendet — ohne diese Normalisierung sind die Risiko-Muster durch ein
// vorangestelltes `sudo`/`env`/`bash -c` wirkungslos, weil fast alle Muster
// am Anfang verankert sind (`^(?:cat|less|...)`, `shutdown*`, `kill *`
// usw.), s. `risk::classifier`-Doc-Kommentar. Wie `segment_command`
// bewusst `pub(crate)` — internes Detail zwischen den beiden Modulen.
pub(crate) use parser::resolve_effective_command;
// Unabhängiger Review-Pass, Spec 0011: von `app-tauri::rule_suggestions`
// wiederverwendet, damit die Regel-Schnellvorschlag-Heuristik dieselbe
// Elevation-/Wrapper-Erkennung nutzt wie die Hard-Blacklist-Prüfung, statt
// eine eigene, potenziell abweichende Liste zu pflegen. Hier bewusst `pub`
// (nicht nur `pub(crate)`) — anders als `segment_command` wird das von
// außerhalb dieser Crate gebraucht.
pub use parser::is_elevation_or_passthrough_wrapper;
// `ServerId` ist seit Spec 0003 ein von `filter` und `profiles` gemeinsam
// genutzter Typ, siehe `crate::shared`-Modul-Kommentar.
pub use crate::shared::ServerId;

/// Öffentlicher Read-Only-Zugriff auf die fest codierte Hard-Blacklist (Spec
/// 0002, Abschnitt 3.1) — für `list_hard_blacklist` (Spec 0009, Abschnitt 3):
/// zeigt dem Nutzer, welche Muster fest im Core codiert sind, ohne sie
/// bearbeiten zu können. Siehe Doc-Kommentar bei `blacklist::hard_blacklist`
/// zur Kapselungs-Entscheidung.
pub fn hard_blacklist_patterns() -> &'static [Pattern] {
    blacklist::hard_blacklist()
}
