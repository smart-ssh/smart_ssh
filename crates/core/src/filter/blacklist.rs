use std::sync::OnceLock;

use super::types::Pattern;

/// Fest codierte, über die öffentliche API nicht entfernbare Menge
/// gefährlicher Kommando-Muster (Spec 0002, Abschnitt 3.1).
///
/// `pub(super)` statt privat: Spec 0009 Abschnitt 6 verlangt eine
/// **read-only** Anzeige dieser Liste im UI ("damit der Nutzer weiß, dass
/// diese existieren, auch wenn er sie nicht ändern kann") — die
/// ursprüngliche Kapselung war gegen *Veränderung/Umgehung* der Liste
/// gedacht, nicht gegen reinen Lesezugriff für Anzeigezwecke. Bleibt aber
/// weiterhin auf `filter` beschränkt (kein `pub` bis zur Crate-Wurzel);
/// [`crate::filter::hard_blacklist_patterns`] ist der öffentliche
/// Read-Only-Zugriffspunkt für Aufrufer außerhalb von `core`.
///
/// MVP-Heuristiken, kein Anspruch auf Vollständigkeit — jedes Muster deckt
/// bewusst eine ganze *Familie* gefährlicher Kommandos ab (nicht nur das
/// wörtliche Beispiel aus der Spec), damit einfache Varianten (andere
/// Flag-Reihenfolge, anderer Zielpfad) nicht durchrutschen. Case-insensitiv,
/// als zusätzliche Sicherheitsmarge (Verteidigung in der Tiefe) — anders als
/// bei Nutzerregeln, wo Groß-/Kleinschreibung bewusst relevant bleibt (siehe
/// `pattern.rs`).
pub(super) fn hard_blacklist() -> &'static [Pattern] {
    use Pattern::{Exact, Glob, Regex};
    static BLACKLIST: OnceLock<Vec<Pattern>> = OnceLock::new();
    BLACKLIST.get_or_init(|| {
        vec![
            // rm -rf/-fr (in beliebiger Flag-Reihenfolge/-Kombination) gegen
            // einen absoluten Pfad — nicht nur das wörtliche "rm -rf /".
            Regex(
                r"(?i)^rm\s+-[a-z]*r[a-z]*f[a-z]*\s+/\S*|^rm\s+-[a-z]*f[a-z]*r[a-z]*\s+/\S*"
                    .to_string(),
            ),
            Glob("dd if=* of=/dev/*".to_string()),
            Glob("mkfs*".to_string()),
            // Fork-Bombe (kanonische Schreibweise laut Spec)
            Exact(":(){ :|:& };:".to_string()),
            // Direkte Manipulation von /etc/shadow: Redirection hinein oder
            // Rechte-/Eigentümer-Änderung.
            Regex(
                r"(?i)(>{1,2}\s*/etc/shadow|chmod\s+\S+\s+/etc/shadow|chown\s+\S+\s+/etc/shadow)"
                    .to_string(),
            ),
            Glob("shutdown*".to_string()),
            Glob("reboot*".to_string()),
        ]
    })
}

/// Liefert einen Anzeigetext des **ersten** gegriffenen Musters, falls eines
/// gegriffen hat — für `EvaluationTrace::matched_hard_blacklist_entry` (Spec
/// 0009, Abschnitt 4) ebenso wie für die reine Ja/Nein-Prüfung in
/// `evaluate_segment_explained` (dort per `.is_some()`).
pub(super) fn matching_entry(cmd: &str) -> Option<String> {
    // Alle Blacklist-Muster sind bereits in Kleinschreibung formuliert, daher
    // reicht es, `cmd` einmal hier zu lowercasen, um case-insensitives
    // Matching für Glob/Exact/Regex einheitlich zu erreichen.
    let lower = cmd.to_lowercase();
    hard_blacklist()
        .iter()
        .find(|pattern| pattern.matches(&lower))
        .map(|pattern| pattern.display_text().to_string())
}
