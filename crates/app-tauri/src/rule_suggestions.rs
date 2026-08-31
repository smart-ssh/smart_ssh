//! Regel-Schnellvorschlag im Bestätigungsdialog (Spec 0011) — reine,
//! testbare Logik; die `#[tauri::command]`-Wrapper in `crate::commands`
//! bleiben dünn, analog zu `crate::groups`/`crate::filter_rules`.

use persistence_sqlite::{PolicyStoreError, SqlitePolicyStore};
use ssh_manager_core::filter::{RuleAction, RuleId, Scope};

use crate::dto::{PatternSuggestionDto, PatternType, RuleInput};

/// Spec 0011, Abschnitt 2: bewusst einfache Wort-Tokenisierung (kein
/// `shell_words`/der volle `core::filter`-Parser mit Chaining-/
/// Substitutions-Erkennung) — die Heuristik hier will nur *Vorschläge*
/// liefern, keine sicherheitsrelevante Auswertung, "kein Anspruch auf
/// Vollständigkeit" laut Spec.
pub fn suggest_rule_patterns(command: &str) -> Vec<PatternSuggestionDto> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();

    let mut suggestions = Vec::new();
    let mut seen_patterns = std::collections::HashSet::new();

    // 1. Exakt.
    if seen_patterns.insert(trimmed.to_string()) {
        suggestions.push(PatternSuggestionDto {
            label: format!("Exakt: {trimmed}"),
            pattern_type: PatternType::Exact,
            pattern_value: trimmed.to_string(),
        });
    }

    if tokens.len() > 1 {
        // 2. Basis-Wildcard: erstes Token + " *".
        let base = format!("{} *", tokens[0]);
        if seen_patterns.insert(base.clone()) {
            suggestions.push(PatternSuggestionDto {
                label: format!("Alle `{}`-Aufrufe: {base}", tokens[0]),
                pattern_type: PatternType::Glob,
                pattern_value: base,
            });
        }

        // 3. Subkommando-Wildcard: nur falls das zweite Token nicht wie
        // eine Flag aussieht (beginnt nicht mit "-"/"--").
        if !tokens[1].starts_with('-') {
            let sub = format!("{} {} *", tokens[0], tokens[1]);
            if seen_patterns.insert(sub.clone()) {
                suggestions.push(PatternSuggestionDto {
                    label: format!("Alle `{} {}`-Aufrufe: {sub}", tokens[0], tokens[1]),
                    pattern_type: PatternType::Glob,
                    pattern_value: sub,
                });
            }
        }
    }

    // Spec Abschnitt 2: "Maximal drei Vorschläge" — durch die Reihenfolge
    // oben (Exakt, Basis, Subkommando) kann `suggestions` ohnehin nie mehr
    // als 3 Einträge enthalten, `truncate` ist hier nur eine explizite
    // Absicherung des in der Spec genannten Limits, kein aktiver Kürzungsfall.
    suggestions.truncate(3);
    suggestions
}

/// Spec 0011, Abschnitt 3, Schritt 1: legt die Regel mit fest `RuleAction::Allow`
/// an (s. Abschnitt 5, "Offene Punkte" — Confirm als Regel-Aktion böte hier
/// keinen Mehrwert gegenüber dem bestehenden Default-Fallback) und
/// `priority` Default `0`, falls nicht angegeben (die Schnellvorschlag-UI
/// bietet laut Abschnitt 4 kein eigenes Prioritäts-Feld an). Reine
/// Delegation an [`crate::filter_rules::create_rule`] (Spec 0009) — keine
/// eigene Anlege-Logik.
pub async fn create_quick_rule(
    policy_store: &SqlitePolicyStore,
    pattern_type: PatternType,
    pattern_value: String,
    scope: Scope,
    priority: Option<i32>,
) -> Result<RuleId, PolicyStoreError> {
    let input = RuleInput {
        pattern_type,
        pattern_value,
        action: RuleAction::Allow,
        scope,
        priority: priority.unwrap_or(0),
    };
    crate::filter_rules::create_rule(policy_store, input).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels_and_patterns(command: &str) -> Vec<(PatternType, String)> {
        suggest_rule_patterns(command)
            .into_iter()
            .map(|s| (s.pattern_type, s.pattern_value))
            .collect()
    }

    /// Spec 0011, Abschnitt 2, wörtliches Beispiel: `-la` sieht wie eine
    /// Flag aus (beginnt mit `-`), daher **kein** Subkommando-Wildcard —
    /// nur Exakt + Basis-Wildcard.
    #[test]
    fn test_ls_example_yields_exact_and_base_wildcard_only() {
        let result = labels_and_patterns("ls -la /var/log");
        assert_eq!(
            result,
            vec![
                (PatternType::Exact, "ls -la /var/log".to_string()),
                (PatternType::Glob, "ls *".to_string()),
            ]
        );
    }

    /// Spec 0011, Abschnitt 2, wörtliches Beispiel: `status` sieht **nicht**
    /// wie eine Flag aus, daher alle drei Vorschläge.
    #[test]
    fn test_systemctl_example_yields_all_three_suggestions() {
        let result = labels_and_patterns("systemctl status nginx");
        assert_eq!(
            result,
            vec![
                (PatternType::Exact, "systemctl status nginx".to_string()),
                (PatternType::Glob, "systemctl *".to_string()),
                (PatternType::Glob, "systemctl status *".to_string()),
            ]
        );
    }

    #[test]
    fn test_single_token_command_yields_only_exact() {
        let result = labels_and_patterns("uptime");
        assert_eq!(result, vec![(PatternType::Exact, "uptime".to_string())]);
    }

    #[test]
    fn test_empty_command_yields_no_suggestions() {
        assert!(suggest_rule_patterns("   ").is_empty());
    }

    #[test]
    fn test_duplicate_patterns_across_heuristics_are_deduplicated() {
        // Zwei Token, zweites beginnt mit "-": Basis-Wildcard "echo *" wäre
        // hier zufällig identisch mit dem, was eine (hier nicht zutreffende)
        // Subkommando-Regel ergäbe — dieser Test deckt stattdessen den Fall
        // ab, dass Exakt und Basis-Wildcard rein zufällig gleich sein
        // könnten (z. B. ein einzelnes Token, das bereits auf "*" endet),
        // ohne dass die Liste einen Duplikat-Eintrag bekommt.
        let result = suggest_rule_patterns("ls *");
        let values: Vec<&str> = result.iter().map(|s| s.pattern_value.as_str()).collect();
        let mut deduped = values.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(values.len(), deduped.len());
    }

    #[tokio::test]
    async fn test_create_quick_rule_defaults_priority_and_uses_allow_action() {
        let dir = tempfile::tempdir().expect("Temp-Verzeichnis sollte anlegbar sein");
        let db_path = dir.path().join("test.db");
        let store = persistence_sqlite::SqliteProfileStore::connect(&db_path)
            .await
            .expect(
                "frische SQLite-Datenbank mit angewendeten Migrationen sollte immer aufbaubar sein",
            )
            .policy_store();

        let rule_id = create_quick_rule(
            &store,
            PatternType::Glob,
            "systemctl *".to_string(),
            Scope::Global,
            None,
        )
        .await
        .unwrap();

        let stored = store.get(&rule_id).await.unwrap();
        assert_eq!(stored.action, RuleAction::Allow);
        assert_eq!(stored.priority, 0);
        assert_eq!(stored.scope, Scope::Global);
    }
}
