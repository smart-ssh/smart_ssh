/// Issue #330: die Zusammenfassungs-Instruktion folgt der Prompt-Sprache.
#[test]
fn test_summary_instruction_follows_the_prompt_language() {
    use crate::system_prompt::prompt_language;
    let lang = |c: &str| prompt_language(Some(&serde_json::json!(c)), None);
    assert!(summary_instruction(PromptLanguage::De).starts_with("Fasse die"));
    let en = summary_instruction(PromptLanguage::En);
    assert!(en.starts_with("Summarize the preceding conversation"));
    assert!(!en.contains("Always respond in"));
    assert_eq!(
        summary_instruction(lang("fr")),
        format!("{en} Always respond in French.")
    );
    assert_eq!(summary_instruction(lang("xx")), en);
}

use super::*;
use ssh_manager_core::ssh::CommandOutput;

fn text_message(role: Role, text: &str) -> ChatMessage {
    ChatMessage {
        role,
        content: MessageContent::Text(text.to_string()),
    }
}

fn user_message(text: &str) -> ChatMessage {
    text_message(Role::User, text)
}

fn command_result(command: &str, stdout: &str) -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::CommandResult {
            command: command.to_string(),
            output: CommandOutput {
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
                exit_code: Some(0),
                truncated: false,
            },
            cancelled: false,
            waiting_for_input: false,
        },
    }
}

fn context_with(system_context: String, history: Vec<ChatMessage>) -> SessionContext {
    SessionContext {
        system_context,
        history,
        available_actions: Vec::new(),
        max_tokens_hint: None,
    }
}

// --- Token-Schätzung, inkl. Fencing-Expansion (Spec 0057, §3.1) ---------

#[test]
fn test_estimate_tokens_uses_four_bytes_per_token_heuristic() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("ab"), 1);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abcde"), 2);
}

/// Spec 0057, §3.1: "Post-Fencing gerechnet" — ein Kommando-Output
/// voller `<`/`>`/`&` muss DEUTLICH mehr Token schätzen als eine
/// naive, rohe Zeichenzählung ergäbe (die alte, abgelöste
/// `chat_context_truncation::approximate_char_len` hätte das
/// systematisch unterschätzt, s. Moduldoc).
#[test]
fn test_estimate_message_tokens_accounts_for_fencing_expansion() {
    let raw_output = "<script>".repeat(200); // viele `<`/`>` = starke Expansion
    let message = command_result("cat evil.html", &raw_output);
    let estimated = estimate_message_tokens(&message);

    let naive_raw_char_estimate = estimate_tokens(&raw_output);
    assert!(
        estimated > naive_raw_char_estimate,
        "Post-Fencing-Schätzung ({estimated}) muss über der naiven \
             Rohzeichen-Schätzung ({naive_raw_char_estimate}) liegen"
    );

    // Exakte Gegenprobe: die Schätzung muss dem tatsächlichen,
    // gefenceten Text entsprechen (plus fester Overhead), nicht nur
    // "irgendwie größer" sein.
    let actually_fenced =
        fence_untrusted(UntrustedKind::CommandStdout, "cat evil.html", &raw_output);
    assert!(estimated >= estimate_tokens(&actually_fenced));
}

#[test]
fn test_estimate_request_tokens_sums_system_context_history_and_actions() {
    let context = context_with("x".repeat(400), vec![user_message(&"y".repeat(400))]);
    let estimated = estimate_request_tokens(&context);
    assert_eq!(estimated, 100 + 100); // 400/4 + 400/4, keine Actions
}

// --- Auslöser bei ~75 % (Spec 0057, §3.1) -----------------------------
//
// `compact_for_send` selbst braucht seit Etappe 3 eine `&Session`
// (Zusammenfassungs-KI-Aufruf in Schritt 1) — die zugehörigen Tests
// leben deshalb in `orchestration::tests`, wo die dafür nötigen
// Session-/MockAiProvider-Bausteine bereits existieren:
// `test_compact_for_send_is_noop_below_trigger_ratio` (unterhalb des
// Auslösers unverändert) und `test_compact_for_send_triggers_and_
// fits_under_budget_with_and_without_summary` (oberhalb des Auslösers,
// Budget-Einhaltung sowohl mit als auch ohne erfolgreiche
// Zusammenfassung — spec-reviewer-Fund, Review dieses Schritts: der
// eigentliche Etappe-2-Test dieser Zusage war beim Verschieben nach
// `orchestration::tests` versehentlich nicht wieder angelegt worden).

// --- Schritt 1: letzte N Runden bleiben immer voll erhalten -----------

#[test]
fn test_split_into_rounds_groups_by_user_message_boundaries() {
    let history = vec![
        user_message("Frage 1"),
        command_result("ls", "ok"),
        user_message("Frage 2"),
    ];
    let rounds = split_into_rounds(history);
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0].len(), 2);
    assert_eq!(rounds[1].len(), 1);
}

/// spec-reviewer-Fund (Review des MCP-Ausschluss-Nachtrags, Punkt 5):
/// `group_mcp_flags_by_round` dupliziert `split_into_rounds`s
/// Rundengrenzen-Logik bewusst (s. dortiger Doc-Kommentar), statt sie
/// wiederzuverwenden — ohne einen Test, der beide Funktionen gegen
/// DIESELBE Historie prüft, könnte eine spätere Änderung an einer der
/// beiden Funktionen (z. B. eine Sonderbehandlung für den
/// Platzhalter-Eintrag) unbemerkt auseinanderdriften. Prüft: gleiche
/// Rundenzahl, gleiche Rundengrößen — die Gruppierungs-Struktur ist
/// identisch, unabhängig vom `mcp_flags`-Inhalt.
#[test]
fn test_group_mcp_flags_by_round_matches_split_into_rounds_boundaries() {
    let history = vec![
        user_message("Frage 1"),
        command_result("ls", "ok"),
        command_result("cat x", "ok"),
        user_message("Frage 2"),
        user_message("Frage 3"), // zwei `User`-Nachrichten hintereinander
        command_result("ls", "ok"),
    ];
    let mcp_flags = vec![false, true, false, true, false, true];

    let expected_round_sizes: Vec<usize> = split_into_rounds(history.clone())
        .iter()
        .map(Vec::len)
        .collect();
    let actual_round_sizes: Vec<usize> = group_mcp_flags_by_round(&history, &mcp_flags)
        .iter()
        .map(Vec::len)
        .collect();

    assert_eq!(
        actual_round_sizes, expected_round_sizes,
        "die Rundengrenzen beider Funktionen müssen für dieselbe Historie identisch sein"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts): Schritt 1 darf nicht
/// unbedingt auf `MIN_PRESERVED_ROUNDS` kürzen, sondern nur so viele
/// alte Runden entfernen, wie tatsächlich nötig sind, um wieder unters
/// Budget zu kommen ("nur so weit wie nötig", Aufgabenstellung) —
/// reicht das Entfernen EINER alten Runde bereits, müssen die übrigen
/// (auch über `MIN_PRESERVED_ROUNDS` hinaus) unangetastet bleiben.
#[test]
fn test_determine_round_cut_count_stops_as_soon_as_it_fits() {
    let history = vec![
        user_message("alte Runde 1"),
        user_message("alte Runde 2"),
        user_message(&"a".repeat(20_000)), // treibt die Gesamtgröße hoch
        user_message("Runde 4"),
        user_message("Runde 5"),
    ];
    // Budget genau so bemessen, dass das Entfernen NUR der ältesten
    // Runde ("alte Runde 1") bereits reicht.
    let rounds = split_into_rounds(history);
    let budget = estimate_message_tokens(&round_truncation_placeholder(PromptLanguage::De, 1))
        + rounds[1..]
            .iter()
            .flatten()
            .map(estimate_message_tokens)
            .sum::<usize>();

    let cut_count = determine_round_cut_count(&rounds, 1, 0, budget, PromptLanguage::De);

    assert_eq!(
        cut_count, 1,
        "nur die älteste Runde hätte entfernt werden dürfen, das Budget passte schon danach"
    );
}

fn placeholder_text(message: &ChatMessage) -> &str {
    match &message.content {
        MessageContent::Text(text) => text,
        other => panic!("Platzhalter muss eine Text-Nachricht sein: {other:?}"),
    }
}

/// Issue #129: der Kürzungshinweis folgt der übergebenen Sprache —
/// Rolle und Platzierung (erste Nachricht) bleiben gleich.
#[test]
fn test_round_truncation_placeholder_follows_the_prompt_language() {
    let english = round_truncation_placeholder(PromptLanguage::En, 2);
    let english = [english];
    assert_eq!(english[0].role, Role::ActionResult);
    let text = placeholder_text(&english[0]);
    assert!(text.contains("older conversation truncated"), "{text}");
    assert!(text.contains("2 earlier conversation round(s)"), "{text}");
    assert!(!text.contains("ältere Konversation gekürzt"), "{text}");

    let german = [round_truncation_placeholder(PromptLanguage::De, 2)];
    assert_eq!(german[0].role, Role::ActionResult);
    assert_eq!(
        placeholder_text(&german[0]),
        "[Hinweis: ältere Konversation gekürzt — 2 frühere Gesprächsrunde(n) wurden aus \
             Platzgründen aus diesem Kontext entfernt. Der vollständige Verlauf bleibt im \
             Session-Ledger erhalten.]"
    );
}

/// Issue #129: die Cut-Count-Schätzung muss denselben Text schätzen,
/// der tatsächlich eingefügt wird. Das Budget ist je Sprache genau das
/// Minimum für "eine Runde entfernen" mit dem Hinweis DIESER Sprache:
/// Schätzte die Suche den Hinweis der anderen Sprache, entfernte sie
/// entweder eine Runde zu viel (anderer Text länger) oder das Ergebnis
/// läge über dem Budget (anderer Text kürzer).
#[test]
fn test_round_cut_estimate_uses_the_inserted_notice_language() {
    assert_ne!(
        estimate_message_tokens(&round_truncation_placeholder(PromptLanguage::De, 1)),
        estimate_message_tokens(&round_truncation_placeholder(PromptLanguage::En, 1)),
        "Vorbedingung: die beiden Hinweise müssen sich in der Schätzung unterscheiden"
    );
    let history = vec![
        user_message(&"a".repeat(4_000)),
        user_message(&"b".repeat(4_000)),
        user_message(&"c".repeat(4_000)),
        user_message("Runde 4"),
    ];
    for language in [PromptLanguage::De, PromptLanguage::En] {
        let budget = estimate_message_tokens(&round_truncation_placeholder(language, 1))
            + history[1..]
                .iter()
                .map(estimate_message_tokens)
                .sum::<usize>();
        let rounds = split_into_rounds(history.clone());
        let cut_count = determine_round_cut_count(&rounds, 1, 0, budget, language);
        assert_eq!(
            cut_count, 1,
            "{language:?}: genau eine Runde hätte entfernt werden müssen"
        );
        let mut context = context_with(String::new(), Vec::new());
        context.history = build_round_result(
            rounds,
            cut_count,
            round_truncation_placeholder(language, cut_count),
        );
        assert_eq!(context.history.len(), 1 + 3);
        assert_eq!(
            context.history[0],
            round_truncation_placeholder(language, 1),
            "{language:?}: eingefügt werden muss der Hinweis der Anfrage-Sprache"
        );
        let estimated = estimate_request_tokens(&context);
        assert!(
            estimated <= budget,
            "{language:?}: geschätzt {estimated}, Budget {budget}"
        );
    }
}

// --- Schritt 2: Riesen-Einzelausgabe gekürzt, Runde nicht verworfen ---

/// spec-reviewer-Fund (Review dieses Schritts): die budgetbewusste
/// Variante darf NICHT jede übergroße Ausgabe unbedingt kürzen —
/// reicht das Kürzen der ersten (ältesten) schon, um wieder unters
/// Budget zu kommen, muss die zweite unangetastet bleiben ("nur so
/// weit wie nötig", Aufgabenstellung).
#[test]
fn test_compact_oversized_outputs_for_budget_stops_as_soon_as_it_fits() {
    let history = vec![
        command_result("cat a.log", &"a".repeat(30_000)),
        command_result("cat b.log", &"b".repeat(30_000)),
    ];
    // Budget dynamisch bestimmt: genau die Größe, die entsteht, wenn
    // NUR die erste (älteste) Ausgabe gekürzt ist — die zweite bleibt
    // dann bewusst über dem Cap, muss aber trotzdem unangetastet
    // bleiben, weil das Budget an dieser Stelle schon erreicht ist.
    let mut budget_probe = context_with(String::new(), history.clone());
    let MessageContent::CommandResult { output, .. } = &mut budget_probe.history[0].content else {
        panic!("erwartete CommandResult");
    };
    truncate_oversized_output(output);
    let budget = estimate_request_tokens(&budget_probe);

    let mut context = context_with(String::new(), history);
    compact_oversized_outputs_for_budget(&mut context, budget);

    let MessageContent::CommandResult { output: first, .. } = &context.history[0].content else {
        panic!("erwartete CommandResult");
    };
    let MessageContent::CommandResult { output: second, .. } = &context.history[1].content else {
        panic!("erwartete CommandResult");
    };
    assert!(
        first.truncated,
        "die erste (älteste) Ausgabe muss gekürzt werden"
    );
    assert!(
        !second.truncated,
        "die zweite Ausgabe darf nicht angefasst werden, sobald das Budget schon passt"
    );
}

// --- Schritt 3: Notiz verkürzt, Scope-Priorisierung -------------------

#[test]
fn test_compact_notes_for_budget_drops_general_scope_sections_first() {
    let parts = SystemContextParts {
        base: "Basis".to_string(),
        note_sections: vec![
            ("Gruppe \"Global\"".to_string(), "g".repeat(3000)),
            ("Server \"db1\"".to_string(), "s".repeat(200)),
        ],
        language: crate::system_prompt::PromptLanguage::De,
    };
    let mut context = context_with(parts.assemble(), Vec::new());
    // Budget knapp über dem, was die Server-Notiz allein braucht.
    let budget = estimate_tokens(&parts.assemble_with_notes(&[parts.note_sections[1].clone()])) + 5;

    compact_notes_for_budget(&mut context, &parts, budget);

    assert!(
        !context.system_context.contains("Gruppe"),
        "die allgemeinere Gruppen-Notiz muss zuerst entfernt worden sein: {}",
        context.system_context
    );
    assert!(
        context.system_context.contains("db1"),
        "die server-spezifische Notiz muss (mindestens teilweise) erhalten bleiben: {}",
        context.system_context
    );
    // Die ORIGINAL-Sektionen in `parts` selbst dürfen nie verändert
    // werden (Spec 0057 §4.1: gespeicherte Notiz bleibt unangetastet).
    assert_eq!(parts.note_sections[0].1.len(), 3000);
}

#[test]
fn test_compact_notes_for_budget_shrinks_last_section_instead_of_dropping_it() {
    let parts = SystemContextParts {
        base: "Basis".to_string(),
        note_sections: vec![("Server \"db1\"".to_string(), "s".repeat(50_000))],
        language: crate::system_prompt::PromptLanguage::De,
    };
    let mut context = context_with(parts.assemble(), Vec::new());
    let budget = 100; // sehr klein — zwingt zur Kürzung der letzten Sektion.

    compact_notes_for_budget(&mut context, &parts, budget);

    assert!(
        context.system_context.contains("db1"),
        "die letzte Sektion wird gekürzt, nie ganz entfernt"
    );
    assert!(
        context.system_context.len() < parts.assemble().len(),
        "der gesendete Kontext muss kleiner sein als das Original"
    );
    assert!(
        context.system_context.contains("für diese Anfrage gekürzt"),
        "muss den Kürzungs-Hinweis tragen: {}",
        context.system_context
    );
    // Untergrenze eingehalten (Notiztext + Label + Fence-Tags + Hinweis
    // bleiben spürbar über 0, s. `MIN_LAST_NOTE_SECTION_BYTES`).
    assert!(context.system_context.len() >= MIN_LAST_NOTE_SECTION_BYTES);
}

/// Issue #90: Mit englischem System-Prompt sind auch die Notiz-
/// Überschrift und der Kürzungshinweis englisch — kein deutscher Rest
/// im ganzen gesendeten System-Prompt.
#[test]
fn test_english_parts_use_english_notes_heading_and_truncation_notice() {
    let parts = SystemContextParts {
        base: "Base".to_string(),
        note_sections: vec![("Server \"db1\"".to_string(), "s".repeat(50_000))],
        language: crate::system_prompt::PromptLanguage::En,
    };
    assert!(parts.assemble().contains("\n\n## Notes / context\n"));
    assert!(!parts.assemble().contains("Notizen"));

    let mut context = context_with(parts.assemble(), Vec::new());
    compact_notes_for_budget(&mut context, &parts, 100);

    assert!(
        context
            .system_context
            .contains("shortened for this request — the stored note is complete"),
        "English truncation notice expected: {}",
        context.system_context
    );
    assert!(!context.system_context.contains("gekürzt"));
    assert!(!context.system_context.contains("Notizen"));
}

// --- Modell-Kontextfenster ---------------------------------------------

#[test]
fn test_model_context_window_tokens_known_and_unknown_models() {
    assert_eq!(
        model_context_window_tokens(ProviderType::Anthropic, "claude-sonnet-4-5"),
        200_000
    );
    assert_eq!(
        model_context_window_tokens(ProviderType::OpenAi, "gpt-3.5-turbo"),
        16_000
    );
    assert_eq!(
        model_context_window_tokens(ProviderType::OpenAi, "gpt-4o"),
        128_000
    );
    assert_eq!(
        model_context_window_tokens(ProviderType::Ollama, "some-tiny-local-model"),
        DEFAULT_CONTEXT_WINDOW_TOKENS
    );
    assert_eq!(
        model_context_window_tokens(ProviderType::GenericOpenAiCompatible, "unknown"),
        DEFAULT_CONTEXT_WINDOW_TOKENS
    );
}
