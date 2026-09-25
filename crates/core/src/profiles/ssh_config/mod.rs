//! `ssh_config` lesen und auf Server-Profile abbilden (Spec 0075).
//!
//! **Reine Logik, kein Dateisystem, kein Tauri** (§4.1). Dieses Modul
//! bekommt bereits gelesene Dateien als Bytes und gibt einen
//! [`plan::ImportPlan`] zurück — die Liste der Profile und Gruppen, die
//! entstehen *würden*, samt Herkunft, Konflikten und nicht übernommenen
//! Direktiven. Es legt nichts an.
//!
//! Der Plan ist zugleich die Vorschau (§3.1.7) und das, was bestätigt
//! ausgeführt wird. Genau deshalb liegt er hier und nicht in `app-shell`:
//! Wären es zwei Objekte, liefen Vorschau und Ergebnis früher oder später
//! auseinander.
//!
//! Wer die Dateien beschafft und `Include` auflöst — mit Tiefe,
//! Schleifenerkennung und den Gesamtgrenzen aus §3.3 — ist `app-shell`
//! (§7.2). Diese Trennung ist der Grund, warum die Rekursion überhaupt
//! begrenzbar ist (§9/Q-1, letzter Absatz).
//!
//! **Eigener Parser, keine Abhängigkeit** (§4.2, §9/M-1 und §9/Q-1): Die
//! Messung aus §7.0 hat `ssh2-config` 0.7.2 verworfen — unter anderem,
//! weil sie keine Zeilennummern liefert (§3.1.5), `Include` selbst gegen
//! `$HOME/.ssh` auflöst und einen `Match`-Block in den vorhergehenden
//! `Host`-Block mischt.
//!
//! Die **Gegenrichtung** — Profile nach `ssh_config` schreiben (§3.2) —
//! liegt in [`export`]. Beide Richtungen teilen sich [`quoting`]: welche
//! Zeichen ein Wert quoten braucht, ist eine Eigenschaft des Formats, nicht
//! der Leserichtung (§9/Q-1 Punkt 5).

pub mod export;
pub mod parser;
pub mod pattern;
pub mod plan;
pub mod quoting;

#[cfg(test)]
mod tests;

pub use export::{build_export, ExportPlan, ExportedServer};
pub use parser::{
    parse_source, FileParse, HostBlock, IncludeDirective, ParsedFile, SkippedDirective,
    SkippedKind, Value, ValueTooLong, MAX_VALUE_CHARS,
};
pub use pattern::{block_matches, matches_pattern, HostClause};
pub use plan::{
    build_plan, Conflict, ConflictKind, IdentityFilePlan, ImportPlan, ImportSource, Inventory,
    JumpTarget, MatchedRule, PlannedEntry, PlannedGroup, PlannedTag, Provenance,
    ProxyJumpRejection, SkipReason, SkippedReport, Sourced,
};
pub use quoting::{needs_quoting, quote_value};
