use std::sync::OnceLock;

use super::types::Pattern;

/// Fest codierte, über die öffentliche API nicht entfernbare Menge
/// gefährlicher Kommando-Muster (Spec 0002, Abschnitt 3.1).
///
/// Bewusst ohne `pub`: kein Aufrufer außerhalb dieses Moduls bekommt eine
/// Referenz auf die Liste selbst, die einzige Zugriffsstelle ist
/// [`matches_any`].
///
/// MVP-Heuristiken, kein Anspruch auf Vollständigkeit — jedes Muster deckt
/// bewusst eine ganze *Familie* gefährlicher Kommandos ab (nicht nur das
/// wörtliche Beispiel aus der Spec), damit einfache Varianten (andere
/// Flag-Reihenfolge, anderer Zielpfad) nicht durchrutschen. Case-insensitiv,
/// als zusätzliche Sicherheitsmarge (Verteidigung in der Tiefe) — anders als
/// bei Nutzerregeln, wo Groß-/Kleinschreibung bewusst relevant bleibt (siehe
/// `pattern.rs`).
fn hard_blacklist() -> &'static [Pattern] {
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

pub(super) fn matches_any(cmd: &str) -> bool {
    // Alle Blacklist-Muster sind bereits in Kleinschreibung formuliert, daher
    // reicht es, `cmd` einmal hier zu lowercasen, um case-insensitives
    // Matching für Glob/Exact/Regex einheitlich zu erreichen.
    let lower = cmd.to_lowercase();
    hard_blacklist()
        .iter()
        .any(|pattern| pattern.matches(&lower))
}
