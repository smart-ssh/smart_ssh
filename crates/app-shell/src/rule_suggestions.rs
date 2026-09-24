//! Regel-Schnellvorschlag im Bestätigungsdialog (Spec 0011) — reine,
//! testbare Logik; die `#[tauri::command]`-Wrapper in `crate::commands`
//! bleiben dünn, analog zu `crate::groups`/`crate::filter_rules`.

use persistence_sqlite::SqlitePolicyStore;
use ssh_manager_core::filter::{is_elevation_or_passthrough_wrapper, RuleAction, RuleId, Scope};

use crate::dto::{PatternSuggestionDto, PatternType, RuleInput};
use crate::filter_rules::RuleWriteError;

/// Spec 0077, 3.1.5: Ein Vorschlag, der die Prüfung aus 3.1.1 nicht
/// besteht, wird **weggelassen** — nicht verändert.
///
/// Nötig, weil die Vorschläge oben ungeprüft aus Kommando-Token gebaut
/// werden (`"{token} *"`): Enthält ein Token eine öffnende Klammer, ist das
/// entstehende Glob-Muster syntaktisch kaputt. Ein solcher Vorschlag würde
/// beim Anlegen ohnehin an Schicht 1 scheitern — ihn gar nicht erst
/// anzubieten erspart dem Nutzer die Sackgasse.
///
/// Ein Muster zu „reparieren" wäre falsch: Die Regel hieße dann etwas
/// anderes, als der Nutzer im Dialog gelesen hat.
fn suggestion_pattern_is_valid(suggestion: &PatternSuggestionDto) -> bool {
    crate::dto::pattern_from_parts(suggestion.pattern_type, suggestion.pattern_value.clone())
        .validate()
        .is_ok()
}

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
        // 2. Basis-Wildcard: erstes Token + " *" — AUSSER das erste Token
        // ist eine Elevation (`sudo`/`doas`) oder ein durchreichendes
        // Wrapper-Kommando (`env`, ...): ein Vorschlag `sudo *` würde
        // buchstäblich JEDES sudo-Kommando AutoExec-fähig machen, egal was
        // der Nutzer im Bestätigungsdialog gerade tatsächlich bestätigt
        // (unabhängiger Review-Pass, Spec 0011 — verifiziert: `sudo
        // systemctl status nginx` schlug bislang `sudo *` vor). Stattdessen
        // wird — falls vorhanden — das ZWEITE Token als Basis verwendet
        // (`apt *` statt `sudo *`), analog zu `resolve_effective_command`
        // in der Filter-Engine. Dank des ohnehin bestehenden
        // Dual-Text-Matchings (ADR 0002) deckt eine solche Regel bewusst
        // weiterhin auch die sudo-Variante ab (`apt *` matcht auch `sudo
        // apt ...`) — das ist dieselbe, an anderer Stelle bereits
        // etablierte Absicht, nur ohne den unbeschränkten Catch-all für
        // JEDES sudo-Kommando.
        let wildcard_base_token = if is_elevation_or_passthrough_wrapper(tokens[0]) {
            tokens.get(1).copied()
        } else {
            Some(tokens[0])
        };
        if let Some(base_token) = wildcard_base_token {
            let base = format!("{base_token} *");
            if seen_patterns.insert(base.clone()) {
                suggestions.push(PatternSuggestionDto {
                    label: format!("Alle `{base_token}`-Aufrufe: {base}"),
                    pattern_type: PatternType::Glob,
                    pattern_value: base,
                });
            }
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

    // Spec 0077, 3.1.5: Vorschläge mit einem Muster, das sich nicht
    // übersetzen lässt, fallen weg. Bewusst **vor** `truncate`, damit das
    // Limit von drei für die verbleibenden, brauchbaren Vorschläge gilt
    // und nicht durch einen unbrauchbaren aufgebraucht wird.
    suggestions.retain(suggestion_pattern_is_valid);

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
///
/// Spec 0077, 3.1.2/3.1.3: Übernimmt damit auch die Prüfung aus Schicht 1
/// und denselben Fehlertyp — ein ungültiges Muster wird hier genauso
/// abgewiesen wie im Formular.
pub async fn create_quick_rule(
    policy_store: &SqlitePolicyStore,
    pattern_type: PatternType,
    pattern_value: String,
    scope: Scope,
    priority: Option<i32>,
) -> Result<RuleId, RuleWriteError> {
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

    // --- Spec 0077: Vorschläge und Schnellregel ---------------------------

    /// Spec 0077, T-6b (3.1.5): Für `ls [abc def` entstünde aus dem ersten
    /// Token der Glob `ls *` (gültig) und aus den ersten beiden
    /// `ls [abc *` — und der übersetzt nicht. Der unbrauchbare Vorschlag
    /// wird weggelassen, die brauchbaren bleiben.
    ///
    /// Scheitert, wenn Vorschläge ungeprüft angeboten werden: Dann stünde
    /// dem Nutzer ein Vorschlag zur Auswahl, den Schicht 1 anschließend
    /// abweist.
    #[test]
    fn test_spec_0077_t6b_suggestions_never_offer_a_pattern_that_does_not_compile() {
        let result = labels_and_patterns("ls [abc def");

        for (pattern_type, pattern_value) in &result {
            let pattern = crate::dto::pattern_from_parts(*pattern_type, pattern_value.clone());
            assert!(
                pattern.validate().is_ok(),
                "unbrauchbarer Vorschlag angeboten: {pattern_value:?}"
            );
        }
        assert!(
            !result.iter().any(|(_, value)| value == "ls [abc *"),
            "der nicht übersetzende Vorschlag muss wegfallen: {result:?}"
        );
        // Die brauchbaren Vorschläge bleiben erhalten — 3.1.5 lässt sie
        // weg, verändert sie nicht und wirft nicht alle weg.
        assert!(
            result.contains(&(PatternType::Exact, "ls [abc def".to_string())),
            "der Exakt-Vorschlag ist gültig und muss bleiben: {result:?}"
        );
        assert!(
            result.contains(&(PatternType::Glob, "ls *".to_string())),
            "der Basis-Wildcard ist gültig und muss bleiben: {result:?}"
        );
    }

    /// Spec 0077, T-6a: Die Schnellregel läuft über
    /// `filter_rules::create_rule` und wird damit von Schicht 1 gedeckt —
    /// ein ungültiger Glob wird abgewiesen, nichts wird gespeichert.
    #[tokio::test]
    async fn test_spec_0077_t6a_quick_rule_rejects_an_invalid_glob_and_stores_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = persistence_sqlite::SqliteProfileStore::connect(&dir.path().join("test.db"))
            .await
            .unwrap()
            .policy_store();

        let err = create_quick_rule(
            &store,
            PatternType::Glob,
            "ls [abc *".to_string(),
            Scope::Global,
            None,
        )
        .await
        .expect_err("ein Glob, der nicht übersetzt, darf nicht gespeichert werden");

        assert!(
            matches!(err, RuleWriteError::InvalidPattern(_)),
            "InvalidPattern erwartet, bekommen: {err:?}"
        );
        assert!(
            store.list_all().await.unwrap().is_empty(),
            "die Schnellregel darf nichts angelegt haben"
        );
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

    /// Regressionstest für den unabhängigen Review-Pass (Spec 0011): ein
    /// `sudo`-Kommando darf NIE einen bloßen `sudo *`-Basis-Wildcard
    /// vorschlagen — das würde ein einziger Klick zu einer Regel machen,
    /// die buchstäblich jedes sudo-Kommando AutoExec-fähig macht. Der
    /// Basis-Wildcard wird stattdessen aus dem zweiten Token gebildet.
    #[test]
    fn test_sudo_command_never_suggests_bare_sudo_wildcard() {
        let result = labels_and_patterns("sudo systemctl status nginx");
        assert!(
            !result.iter().any(|(_, pattern)| pattern == "sudo *"),
            "darf niemals einen 'sudo *'-Catch-all vorschlagen, bekam: {result:?}"
        );
        assert_eq!(
            result,
            vec![
                (
                    PatternType::Exact,
                    "sudo systemctl status nginx".to_string()
                ),
                (PatternType::Glob, "systemctl *".to_string()),
                (PatternType::Glob, "sudo systemctl *".to_string()),
            ]
        );
    }

    /// Dasselbe für einen durchreichenden Wrapper statt einer Elevation.
    #[test]
    fn test_wrapper_command_never_suggests_bare_wrapper_wildcard() {
        let result = labels_and_patterns("env rm -rf /var/log");
        assert!(
            !result.iter().any(|(_, pattern)| pattern == "env *"),
            "darf keinen 'env *'-Catch-all vorschlagen, bekam: {result:?}"
        );
        assert!(result.iter().any(|(_, pattern)| pattern == "rm *"));
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
