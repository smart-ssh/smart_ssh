//! Tests für die Kompaktierung/rollierende Zusammenfassung des an die KI
//! gesendeten Kontexts sowie die additive Re-Redaction vor jedem `send()`
//! — Spec 0083: reine Verschiebung aus `orchestration::tests`, keine
//! Verhaltensänderung.

use std::sync::Mutex as StdMutex;

use uuid::Uuid;

use tokio::sync::Mutex as AsyncMutex;

use ssh_manager_core::ai::{
    fence_untrusted, AiError, AiProvider, DefaultOutputRedactor, SessionContext, UntrustedKind,
};
use ssh_manager_core::filter::{Decision, FilterEngine};
use ssh_manager_core::profiles::PostIngestPolicy;
use ssh_manager_core::ssh::mock::MockSftpSession;
use ssh_manager_core::ssh::CommandOutput;

use crate::dto::ActionUserDecision;
use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

// --- Spec 0057, §3 + §4.1: Kompaktierung (Etappe 2) --------------------

/// Spec 0057, §3.3/§6: "Kompaktierung betrifft nur den an die KI
/// gesendeten Kontext, nie den Ledger." End-to-End-Beweis: Runde 1
/// führt ein Kommando mit einer riesigen Ausgabe aus (landet
/// VOLLSTÄNDIG im Ledger, s. `execute_suggested_command`s
/// `write_ledger_entry`-Aufruf mit dem noch unkomprimierten `output`).
/// Ein winziges `model_context_window_tokens` zwingt Runde 2s
/// `send()`-Aufruf dazu, genau diese Ausgabe für die gesendete Kopie zu
/// kürzen (Schritt 2, Spec 0057 §3.2) — der `MockAiProvider` zeichnet
/// den tatsächlich empfangenen Kontext auf, das Ledger bleibt davon
/// unberührt.
#[tokio::test]
async fn test_compaction_shrinks_sent_copy_but_ledger_keeps_full_output() {
    let huge_output = "L".repeat(100_000);
    let (mut session, _chat_store, chat_session_id, _tmp_dir, ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done], // wird unten sofort durch den echten Mehr-Runden-Provider ersetzt
            MockSshTransport::default().with_response("cat big.log", output(&huge_output)),
        )
        .await;
    // `session_with_real_chat_and_ledger_persistence` konfiguriert nur
    // einen `MockAiProvider::new` (eine Runde) — hier wird stattdessen
    // ein waschechter Mehr-Runden-Provider gebraucht (Runde 1: Kommando
    // vorschlagen, Runde 2: nur noch Text antworten).
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    session.parts_mut_for_tests().ai_provider = Box::new(MockAiProvider {
        rounds: StdMutex::new(
            vec![
                vec![
                    AiEvent::ActionProposed(AiAction::SuggestCommand {
                        command: "cat big.log".to_string(),
                    }),
                    AiEvent::Done,
                ],
                vec![AiEvent::TextDelta("Erledigt.".to_string()), AiEvent::Done],
            ]
            .into(),
        ),
        received_contexts: received_contexts.clone(),
    });
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    // Winzig: erzwingt, dass die 100.000-Byte-Ausgabe beim ZWEITEN
    // `send()` (der die erste Runde bereits in der Historie trägt)
    // gekürzt werden MUSS.
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    // Runde 1: Kommando ausführen (landet mit voller Ausgabe im
    // Ledger).
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;
    // Runde 2 (neue Nutzer-Nachricht): erzwingt einen weiteren
    // `send()`, dessen Kontext bereits Runde 1s Riesen-Ausgabe trägt —
    // genau der Aufruf, der kompaktiert werden muss.
    {
        let mut ctx = session.context.lock().await;
        ctx.history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Danke, das reicht.".to_string()),
        });
        session.mcp_origin_flags.lock().unwrap().push(false);
    }
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    // Die zuletzt tatsächlich an den Provider gesendete Kopie muss die
    // Riesen-Ausgabe gekürzt haben.
    let last_sent = received_contexts
        .lock()
        .unwrap()
        .last()
        .expect("mindestens ein send()-Aufruf muss stattgefunden haben")
        .clone();
    let sent_stdout_len: usize = last_sent
        .history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::CommandResult { output, .. } => {
                Some(String::from_utf8_lossy(&output.stdout).len())
            }
            _ => None,
        })
        .sum();
    assert!(
        sent_stdout_len < 100_000,
        "die an den Provider gesendete Kopie muss gekürzt sein, war {sent_stdout_len} Byte"
    );

    // Das Ledger dagegen muss die VOLLE, unkomprimierte Ausgabe
    // enthalten — Kompaktierung betrifft nie das Ledger.
    let entries = ledger_store.load_entries(chat_session_id).await.unwrap();
    let ledger_stdout_len = entries
        .iter()
        .find_map(|e| match &e.content {
            ssh_manager_core::audit::LedgerEntryContent::CommandExecuted { output, .. } => {
                Some(output.stdout.len())
            }
            _ => None,
        })
        .expect("ein CommandExecuted-Eintrag muss existieren");
    assert_eq!(
        ledger_stdout_len, 100_000,
        "das Ledger muss die volle Ausgabe behalten, unabhängig von der Kompaktierung \
         der gesendeten Kopie"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts) + CLAUDE.md-Pflicht für
/// Redaction-berührende Änderungen: ein Fake-Secret in einer riesigen
/// Kommando-Ausgabe, die Schritt 2 (Spec 0057 §3.2) für den Versand
/// kürzen MUSS, darf trotzdem nicht unredigiert beim Provider ankommen.
/// Das Secret sitzt hier bewusst weit VOR der Kürzungs-Kante (Schritt 2
/// schneidet nur das Ende ab) — der Regelfall, den die Kompaktierung
/// nicht brechen darf: Kompaktierung läuft vor
/// `reapply_redaction_for_send`, die Redaction sieht also immer die
/// zuletzt gesendete, bereits gekürzte Fassung.
#[tokio::test]
async fn test_compaction_does_not_bypass_redaction_for_truncated_output() {
    let mut huge_output = "password=hunter2geheim\n".to_string();
    huge_output.push_str(&"X".repeat(100_000));
    let (mut session, _chat_store, _chat_session_id, _tmp_dir, _ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done],
            MockSshTransport::default().with_response("cat secret.log", output(&huge_output)),
        )
        .await;
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    session.parts_mut_for_tests().ai_provider = Box::new(MockAiProvider {
        rounds: StdMutex::new(
            vec![
                vec![
                    AiEvent::ActionProposed(AiAction::SuggestCommand {
                        command: "cat secret.log".to_string(),
                    }),
                    AiEvent::Done,
                ],
                vec![AiEvent::TextDelta("Erledigt.".to_string()), AiEvent::Done],
            ]
            .into(),
        ),
        received_contexts: received_contexts.clone(),
    });
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;
    {
        let mut ctx = session.context.lock().await;
        ctx.history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Danke.".to_string()),
        });
        session.mcp_origin_flags.lock().unwrap().push(false);
    }
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let last_sent = received_contexts
        .lock()
        .unwrap()
        .last()
        .expect("mindestens ein send()-Aufruf muss stattgefunden haben")
        .clone();
    let sent_stdouts: Vec<String> = last_sent
        .history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::CommandResult { output, .. } => {
                Some(String::from_utf8_lossy(&output.stdout).into_owned())
            }
            _ => None,
        })
        .collect();
    assert!(
        !sent_stdouts.iter().any(|s| s.contains("hunter2geheim")),
        "das Secret darf auch nach Kürzung+Kompaktierung nie unredigiert gesendet werden: \
         {sent_stdouts:?}"
    );
    assert!(
        sent_stdouts.iter().any(|s| s.contains("REDACTED")),
        "die gesendete Kopie muss den redigierten Platzhalter enthalten: {sent_stdouts:?}"
    );
}

/// Spec 0057, §3/§4.1, §7 ("Immich-Fall"): eine sehr große, über
/// mehrere Scopes verteilte Notiz UND eine lange Historie zusammen
/// hätten vor Etappe 2 unbegrenzt an den Provider gesendet werden
/// können (der eigentliche, diagnostizierte Hänger). Beweis: nach der
/// Kompaktierung bleibt die TATSÄCHLICH gesendete Anfrage unter dem
/// Budget — kein Hänger.
#[tokio::test]
async fn test_immich_case_large_note_and_long_history_stays_under_budget() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(vec![vec![AiEvent::Done]].into()),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    // Wie `GenericOpenAiCompatible`/`Ollama` ohne erkannten Modellnamen
    // (Spec 0057 §3.1: konservativer Default), s.
    // `compaction::DEFAULT_CONTEXT_WINDOW_TOKENS`.
    session.parts_mut_for_tests().model_context_window_tokens = 32_000;

    // Große, über drei Scopes verteilte Notiz (~200.000 Byte) — analog
    // zum Immich-Fall aus Spec 0057 §8.
    let parts = crate::compaction::SystemContextParts {
        base: "Du bist ein Assistent.".to_string(),
        note_sections: vec![
            ("Gruppe \"Global\"".to_string(), "g".repeat(150_000)),
            ("Gruppe \"Media-Server\"".to_string(), "m".repeat(40_000)),
            ("Server \"immich\"".to_string(), "s".repeat(10_000)),
        ],
    };

    // Lange Historie: 15 Runden mit je einer moderaten Kommando-Ausgabe.
    let mut history = Vec::new();
    for i in 0..15 {
        history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text(format!("Frage {i}")),
        });
        history.push(ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: format!("docker logs immich-{i}"),
                output: CommandOutput {
                    stdout: "log-zeile\n".repeat(200).into_bytes(), // ~2 KB
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
                cancelled: false,
            },
        });
    }
    history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text("Was ist der aktuelle Stand?".to_string()),
    });

    {
        let mut ctx = session.context.lock().await;
        ctx.system_context = parts.assemble();
        ctx.history = history;
    }
    session.parts_mut_for_tests().system_context_parts = AsyncMutex::new(parts);

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let sent = received_contexts
        .lock()
        .unwrap()
        .last()
        .expect("mindestens ein send()-Aufruf muss stattgefunden haben")
        .clone();
    let budget = (session.model_context_window_tokens as f64 * 0.75) as usize;
    let estimated = crate::compaction::estimate_request_tokens(&sent);
    assert!(
        estimated <= budget,
        "die TATSÄCHLICH gesendete Anfrage muss unter dem Budget bleiben \
         (geschätzt: {estimated} Token, Budget: {budget} Token) — das ist der \
         eigentliche Beweis, dass Etappe 2 den Immich-Hänger löst"
    );
}

// --- Spec 0057, §2: rollierende Zusammenfassung (Etappe 3) -------------

fn user_msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: MessageContent::Text(text.to_string()),
    }
}

fn command_result_msg(command: &str, stdout: &str) -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::CommandResult {
            command: command.to_string(),
            output: output(stdout),
            cancelled: false,
        },
    }
}

/// Baut `count` Runden (je eine `User`- + eine `ActionResult`-
/// Nachricht) direkt in `session.context` — für Tests, die eine lange
/// Historie brauchen, ohne dafür jede Runde über einen echten
/// `run_chat_turn` laufen zu lassen.
async fn push_synthetic_rounds(session: &Session, count: usize, stdout_bytes_each: usize) {
    let mut ctx = session.context.lock().await;
    let mut flags = session.mcp_origin_flags.lock().unwrap();
    for i in 0..count {
        ctx.history.push(user_msg(&format!("Frage {i}")));
        flags.push(false);
        ctx.history.push(command_result_msg(
            &format!("cmd-{i}"),
            &"x".repeat(stdout_bytes_each),
        ));
        flags.push(false);
    }
}

#[tokio::test]
async fn test_compact_for_send_is_noop_below_trigger_ratio() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default(),
    );
    let context = SessionContext {
        system_context: String::new(),
        history: vec![user_msg(&"a".repeat(2400))], // ~600 Token
        available_actions: Vec::new(),
        max_tokens_hint: None,
    };
    let parts = crate::compaction::SystemContextParts::default();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        context.clone(),
        &parts,
        1_000,
    )
    .await;
    assert_eq!(
        result, context,
        "unterhalb des Auslösers darf nichts verändert werden"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts): der Etappe-2-Test, der
/// die eigentliche Zusage der Leiter prüfte — "nach der Kompaktierung
/// passt der Request unters Budget" (Spec 0057, §7) — ging beim
/// Verschieben nach `orchestration::tests` (Etappe 3, `&Session`-
/// Parameter) verloren. Hier für BEIDE Pfade wiederhergestellt: mit
/// erfolgreicher Zusammenfassung (Schritt 1 allein reicht) und mit
/// fehlgeschlagener Zusammenfassung (Fallback auf den Etappe-2-
/// Platzhalter, der ebenfalls unters Budget passen muss — dafür ist
/// `determine_round_cut_count` überhaupt konservativ anhand der
/// Platzhalter-Größe bemessen, s. ADR 0049 Punkt 3).
#[tokio::test]
async fn test_compact_for_send_triggers_and_fits_under_budget_with_and_without_summary() {
    let history = vec![
        user_msg("Runde 1"),
        user_msg("Runde 2"),
        user_msg("Runde 3"),
        user_msg("Runde 4"),
        command_result_msg("cat big.log", &"a".repeat(100_000)),
    ];
    let budget = (10_000_f64 * 0.75) as usize;

    // Pfad 1: Zusammenfassung gelingt.
    {
        let session = session_with_ai_provider(
            MockAiProvider::new(vec![
                AiEvent::TextDelta("Kurze Zusammenfassung.".to_string()),
                AiEvent::Done,
            ]),
            MockSshTransport::default(),
        );
        {
            let mut ctx = session.context.lock().await;
            ctx.history = history.clone();
        }
        let request_context = session.context.lock().await.clone();
        let parts = session.system_context_parts.lock().await.clone();
        let result = crate::compaction::compact_for_send(
            &session,
            Uuid::new_v4(),
            &TestEmitter::default(),
            request_context,
            &parts,
            10_000,
        )
        .await;
        let estimated = crate::compaction::estimate_request_tokens(&result);
        assert!(
            estimated <= budget,
            "mit erfolgreicher Zusammenfassung muss der Request unters Budget passen \
             (geschätzt {estimated}, Budget {budget})"
        );
    }

    // Pfad 2: Zusammenfassung schlägt fehl -> Fallback.
    {
        let session = session_with_ai_provider(
            MockAiProvider::new(vec![AiEvent::Error(AiError::RateLimited)]),
            MockSshTransport::default(),
        );
        {
            let mut ctx = session.context.lock().await;
            ctx.history = history.clone();
        }
        let request_context = session.context.lock().await.clone();
        let parts = session.system_context_parts.lock().await.clone();
        let result = crate::compaction::compact_for_send(
            &session,
            Uuid::new_v4(),
            &TestEmitter::default(),
            request_context,
            &parts,
            10_000,
        )
        .await;
        let estimated = crate::compaction::estimate_request_tokens(&result);
        assert!(
            estimated <= budget,
            "auch der Fallback-Platzhalter (ohne Zusammenfassung) muss unters Budget \
             passen (geschätzt {estimated}, Budget {budget})"
        );
    }
}

/// Spec 0057, §2.1: greift Schritt 1 der Kürzungs-Leiter, wird beim
/// ERSTEN Mal eine echte Zusammenfassung erzeugt (kein bisheriger
/// Stand vorhanden, der wiederverwendet werden könnte) — der
/// Platzhalter muss den KI-generierten Text tragen, nicht den
/// generischen Etappe-2-Hinweis, und `session.summary` muss
/// aktualisiert sein.
#[tokio::test]
async fn test_compact_for_send_uses_summary_when_the_call_succeeds() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![
            AiEvent::TextDelta("Nutzer prüfte Logs, alles unauffällig.".to_string()),
            AiEvent::Done,
        ]),
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    let placeholder_text = result
        .history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::Text(t) if t.contains("Zusammenfassung") => Some(t.clone()),
            _ => None,
        })
        .expect("ein Zusammenfassungs-Platzhalter muss in der gekürzten Historie stehen");
    assert!(
        placeholder_text.contains("Nutzer prüfte Logs, alles unauffällig."),
        "der Platzhalter muss den tatsächlichen KI-Text tragen: {placeholder_text}"
    );
    assert!(
        !placeholder_text.contains("ältere Konversation gekürzt"),
        "bei Erfolg darf NICHT der generische Etappe-2-Hinweis verwendet werden"
    );
    let stored = session.summary.lock().await.clone();
    assert_eq!(
        stored.map(|s| s.text),
        Some("Nutzer prüfte Logs, alles unauffällig.".to_string())
    );
}

/// Spec 0057, §2.1: "nicht jedes Mal die ganze History von vorne" —
/// deckt eine bereits gespeicherte Zusammenfassung den benötigten
/// `cut_count` schon ab, darf KEIN zweiter KI-Aufruf stattfinden.
#[tokio::test]
async fn test_compact_for_send_reuses_existing_summary_without_a_new_ai_call() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(vec![vec![AiEvent::Done]].into()),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;
    *session.summary.lock().await = Some(crate::compaction::RollingSummary {
        text: "Bereits vorhandene Zusammenfassung.".to_string(),
        // Großzügig: deckt mehr Runden ab, als für das aktuelle Budget
        // überhaupt gekürzt werden müssten.
        rounds_covered: 6,
    });

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    assert!(
        received_contexts.lock().unwrap().is_empty(),
        "die vorhandene Summary deckt den Bedarf bereits ab — es darf kein KI-Aufruf \
         stattfinden"
    );
    assert!(
        result
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("Bereits vorhandene Zusammenfassung."))),
        "die wiederverwendete Summary muss im gesendeten Kontext stehen"
    );
}

/// Spec 0057, §2.1 ("rollierend"): eine bereits vorhandene, aber nicht
/// mehr ausreichende Zusammenfassung wird nicht verworfen und neu von
/// vorne erzeugt, sondern nur um die NEU zu kürzenden Runden ergänzt —
/// der KI-Aufruf darf ausschließlich das neue Stück + den bisherigen
/// Summary-Text enthalten, nicht die bereits zusammengefassten,
/// alten Runden im Rohformat.
#[tokio::test]
async fn test_compact_for_send_folds_only_newly_cut_rounds_into_existing_summary() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(
                vec![vec![
                    AiEvent::TextDelta("Erweiterte Summary.".to_string()),
                    AiEvent::Done,
                ]]
                .into(),
            ),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    // 8 Runden, `rounds_covered: 2` -> die bereits abgedeckten Runden
    // 0/1 dürfen im KI-Aufruf NICHT im Rohformat auftauchen.
    push_synthetic_rounds(&session, 8, 5_000).await;
    *session.summary.lock().await = Some(crate::compaction::RollingSummary {
        text: "Alte Zusammenfassung (Runden 0-1).".to_string(),
        rounds_covered: 2,
    });

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let _ = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    let sent = received_contexts
        .lock()
        .unwrap()
        .last()
        .expect("der Summary-Aufruf muss stattgefunden haben")
        .clone();
    assert!(
        sent.history.iter().any(
            |m| matches!(&m.content, MessageContent::Text(t) if t.contains("Alte Zusammenfassung (Runden 0-1)."))
        ),
        "der bisherige Summary-Text muss in den Aufruf eingehen: {:?}",
        sent.history
    );
    assert!(
        !sent.history.iter().any(
            |m| matches!(&m.content, MessageContent::CommandResult { command, .. } if command == "cmd-0" || command == "cmd-1")
        ),
        "bereits abgedeckte Runden (0/1) dürfen NICHT erneut im Rohformat gesendet werden: \
         {:?}",
        sent.history
    );
    assert!(
        sent.history.iter().any(
            |m| matches!(&m.content, MessageContent::CommandResult { command, .. } if command == "cmd-2")
        ),
        "die neu zu kürzende Runde 2 muss im Aufruf enthalten sein: {:?}",
        sent.history
    );
}

/// Spec 0057, §2.2 (KRITISCH): schlägt der Zusammenfassungs-Aufruf fehl
/// (hier: `AiEvent::Error`), muss die Sitzung auf das reine
/// Etappe-2-Abschneiden zurückfallen — kein Hang, kein Absturz, `
/// session.summary` bleibt unverändert (hier: weiterhin `None`).
#[tokio::test]
async fn test_compact_for_send_falls_back_to_plain_truncation_on_summary_error() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Error(AiError::RateLimited)]),
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    assert!(
        result
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("ältere Konversation gekürzt"))),
        "bei einem Fehlschlag muss der generische Etappe-2-Hinweis verwendet werden: {:?}",
        result.history
    );
    assert!(
        session.summary.lock().await.is_none(),
        "ein fehlgeschlagener Versuch darf `session.summary` nicht verändern"
    );
}

/// Wie oben, aber der Aufruf liefert nur eine leere/Whitespace-Antwort
/// — Spec 0057, §2.2 zählt das ausdrücklich als Fehlschlag ("leere/
/// unbrauchbare Antwort"), nicht als Erfolg mit leerem Inhalt.
#[tokio::test]
async fn test_compact_for_send_falls_back_to_plain_truncation_on_empty_summary_response() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![
            AiEvent::TextDelta("   \n  ".to_string()),
            AiEvent::Done,
        ]),
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    assert!(
        result
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("ältere Konversation gekürzt"))),
        "eine leere Antwort zählt als Fehlschlag, muss auf den Etappe-2-Hinweis zurückfallen"
    );
    assert!(session.summary.lock().await.is_none());
}

/// spec-reviewer-Fund (Review dieses Schritts, Testabdeckungs-Lücke):
/// ein Provider-Stream, der ohne `AiEvent::Done`/`AiEvent::Error`
/// einfach endet (z. B. eine abgebrochene Verbindung mitten im
/// Streaming), muss GENAUSO wie ein expliziter Fehler behandelt
/// werden — nicht stillschweigend als Erfolg mit dem bis dahin
/// akkumulierten Text.
#[tokio::test]
async fn test_compact_for_send_falls_back_when_stream_ends_without_done_or_error() {
    let mut session = session_with_ai_provider(
        // Kein `AiEvent::Done` am Ende — der Stream versiegt einfach.
        MockAiProvider::new(vec![AiEvent::TextDelta(
            "Unvollständige Antwort".to_string(),
        )]),
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    assert!(
        result
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("ältere Konversation gekürzt"))),
        "ein Stream-Ende ohne Done/Error muss wie ein Fehlschlag behandelt werden, nicht \
         als Erfolg mit unvollständigem Text: {:?}",
        result.history
    );
    assert!(session.summary.lock().await.is_none());
}

/// Ein `AiProvider`, dessen Stream nie ein Item liefert (simuliert
/// einen Provider, der mitten im Aufruf hängen bleibt, ohne je einen
/// Fehler zu melden) — für den Zeitrahmen-Test unten.
struct NeverRespondingProvider;
impl AiProvider for NeverRespondingProvider {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::pending())
    }
}

/// spec-reviewer-Fund (Review dieses Schritts, Testabdeckungs-Lücke):
/// beweist, dass [`crate::compaction::SUMMARY_CALL_TIMEOUT`] (Spec
/// 0057 §2.1: "nicht ungeschützt") tatsächlich feuert, statt sich nur
/// auf den Kommentar zu verlassen — ein Provider, dessen Stream
/// NIEMALS ein Item liefert (auch keinen Fehler), darf die
/// Kompaktierung nicht unbegrenzt blockieren.
#[tokio::test(start_paused = true)]
async fn test_compact_for_send_falls_back_when_summary_call_never_responds() {
    let mut session =
        session_with_ai_provider(NeverRespondingProvider, MockSshTransport::default());
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let emitter = TestEmitter::default();
    let compaction = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &emitter,
        request_context,
        &parts,
        session.model_context_window_tokens,
    );
    tokio::pin!(compaction);

    // Unter `start_paused = true` gäbe es ohne aktives Vorspulen
    // nichts, wogegen der Timeout liefe — `tokio::time::advance`
    // (dasselbe Muster wie bei `PENDING_ACTION_CONFIRM_TIMEOUT`-Tests)
    // schiebt die virtuelle Uhr über `SUMMARY_CALL_TIMEOUT` hinaus.
    let advancer = tokio::time::advance(std::time::Duration::from_secs(121));
    let (result, ()) = tokio::join!(&mut compaction, advancer);

    assert!(
        result
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("ältere Konversation gekürzt"))),
        "ein niemals antwortender Provider muss nach dem Zeitrahmen auf den Fallback \
         zurückfallen, nicht unbegrenzt blockieren: {:?}",
        result.history
    );
    assert!(session.summary.lock().await.is_none());
}

/// spec-reviewer-Pflicht (CLAUDE.md, Redaction-berührende Änderungen) +
/// Aufgabenstellung: ein Fake-Secret darf weder im AUSGEHENDEN
/// Summary-Aufruf noch in der ZURÜCKKOMMENDEN (und gespeicherten)
/// Zusammenfassung unredigiert auftauchen. Die zu faltende Runde trägt
/// das Secret hier bewusst in `MessageContent::Text` (anders als
/// `CommandResult`, das schon beim Ausführen redigiert wird, s.
/// `execute_suggested_command`, läuft `Text`-Inhalt nie automatisch
/// durch den Redactor, bevor er in der Historie landet) — genau der
/// Fall, für den `generate_rolling_summary`s zusätzliche,
/// defensive Re-Redaction gedacht ist.
#[tokio::test]
async fn test_generate_rolling_summary_redacts_secrets_outgoing_and_incoming() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(
                vec![vec![
                    AiEvent::TextDelta(
                        "Zusammenfassung: Zugriff erfolgte mit password=hunter2geheim.".to_string(),
                    ),
                    AiEvent::Done,
                ]]
                .into(),
            ),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    {
        let mut ctx = session.context.lock().await;
        let mut flags = session.mcp_origin_flags.lock().unwrap();
        for i in 0..6 {
            ctx.history.push(user_msg(&format!("Frage {i}")));
            flags.push(false);
            if i == 0 {
                ctx.history.push(ChatMessage {
                    role: Role::Assistant,
                    content: MessageContent::Text(format!(
                        "Verbindung mit password=hunter2geheim aufgebaut. {}",
                        "x".repeat(5_000)
                    )),
                });
            } else {
                ctx.history
                    .push(command_result_msg(&format!("cmd-{i}"), &"x".repeat(5_000)));
            }
            flags.push(false);
        }
    }

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    let placeholder_text = result
        .history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::Text(t) if t.contains("Zusammenfassung") => Some(t.clone()),
            _ => None,
        })
        .expect("Zusammenfassungs-Platzhalter erwartet");
    assert!(
        !placeholder_text.contains("hunter2geheim"),
        "das Secret darf nicht unredigiert in der gespeicherten/gesendeten Zusammenfassung \
         landen: {placeholder_text}"
    );
    assert!(
        placeholder_text.contains("REDACTED"),
        "muss den redigierten Platzhalter enthalten: {placeholder_text}"
    );
    let stored = session.summary.lock().await.clone();
    assert!(
        !stored.unwrap().text.contains("hunter2geheim"),
        "auch der persistierte In-Memory-Stand darf das Secret nicht enthalten"
    );

    // spec-reviewer-Fund (Review dieses Schritts): der bisherige Test
    // prüfte nur die eingehende Richtung (die zurückkommende
    // Zusammenfassung) — hier zusätzlich die AUSGEHENDE Richtung: der
    // Zusammenfassungs-Aufruf selbst faltet Runde 0 (mit dem Secret in
    // `MessageContent::Text`, das NIE automatisch beim Ablegen
    // redigiert wird, anders als `CommandResult`) — die tatsächlich an
    // den Provider gesendete Anfrage darf das Secret ebenfalls nicht
    // unredigiert enthalten (`reapply_redaction_for_send` in
    // `generate_rolling_summary`).
    let sent = received_contexts
        .lock()
        .unwrap()
        .first()
        .expect("der Zusammenfassungs-Aufruf muss stattgefunden haben")
        .clone();
    let sent_texts: Vec<String> = sent
        .history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !sent_texts.iter().any(|t| t.contains("hunter2geheim")),
        "das Secret darf auch im AUSGEHENDEN Zusammenfassungs-Aufruf nicht unredigiert \
         auftauchen: {sent_texts:?}"
    );
    assert!(
        sent_texts.iter().any(|t| t.contains("REDACTED")),
        "die ausgehende Fassung muss den redigierten Platzhalter enthalten: {sent_texts:?}"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts): eine bereits
/// gespeicherte (z. B. aus der DB nach `resume` geladene) Zusammen-
/// fassung, die einen unredigierten Secret-artigen String trägt (etwa
/// weil sie mit einem älteren Redactor-Musterstand erzeugt wurde),
/// darf beim FALTEN in einen neuen Zusammenfassungs-Aufruf nicht
/// unverändert (unredigiert) an den Provider gehen — dieselbe
/// additive Re-Redaction wie für jeden anderen ausgehenden Inhalt.
#[tokio::test]
async fn test_generate_rolling_summary_reredacts_a_stale_previous_summary() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(
                vec![vec![
                    AiEvent::TextDelta("Neue Zusammenfassung.".to_string()),
                    AiEvent::Done,
                ]]
                .into(),
            ),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    *session.summary.lock().await = Some(crate::compaction::RollingSummary {
        text: "Alte Zusammenfassung mit password=altesecretgeheim.".to_string(),
        rounds_covered: 1,
    });
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let _ = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    let sent = received_contexts
        .lock()
        .unwrap()
        .first()
        .expect("der Zusammenfassungs-Aufruf muss stattgefunden haben")
        .clone();
    let sent_texts: Vec<String> = sent
        .history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !sent_texts.iter().any(|t| t.contains("altesecretgeheim")),
        "eine bereits gespeicherte Zusammenfassung muss beim erneuten Falten re-redigiert \
         werden: {sent_texts:?}"
    );
}

/// Spec 0057, §2.3: die rollierende Zusammenfassung wird verschlüsselt
/// mit der Session persistiert und bei `resume` wieder geladen — sonst
/// müsste jede wiederaufgenommene Sitzung bei der nächsten
/// Kompaktierung wieder bei `rounds_covered = 0` anfangen.
#[tokio::test]
async fn test_summary_round_trips_through_persistence() {
    // spec-reviewer-Fund (Review dieses Schritts): ein reiner
    // `save_summary`/`load_summary`-Aufrufpaar über denselben Store
    // prüft nur die Store-API selbst (jetzt eigenständig und
    // verschlüsselungs-scharf abgedeckt in
    // `persistence_sqlite::chat_session_store::tests::
    // test_direct_sql_access_to_summary_text_column_never_reveals_
    // plaintext`) — hier stattdessen der tatsächliche END-ZU-ENDE-Pfad:
    // ein echter `compact_for_send`-Lauf erzeugt über
    // `persist_rolling_summary` einen DB-Eintrag, den `load_summary`
    // danach unabhängig wiederfindet.
    let (mut session, chat_store, chat_session_id, _tmp_dir, _ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![
                AiEvent::TextDelta("Persistierte Zusammenfassung.".to_string()),
                AiEvent::Done,
            ],
            MockSshTransport::default(),
        )
        .await;
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    let loaded = chat_store.load_summary(chat_session_id).await.unwrap();
    assert_eq!(
        loaded,
        Some(("Persistierte Zusammenfassung.".to_string(), 3)),
        "die von `compact_for_send` erzeugte Zusammenfassung muss über den echten \
         `persist_rolling_summary`-Pfad in der DB gelandet sein"
    );
}

/// Spec 0057, §3.3/§6 (wie bereits in Etappe 1/2 verifiziert, hier für
/// den Zusammenfassungs-Pfad wiederholt): das Ledger bekommt von der
/// Kompaktierung — egal ob mit oder ohne Zusammenfassung — nichts zu
/// Gesicht, und die gespeicherte Notiz bleibt unangetastet.
#[tokio::test]
async fn test_summarization_leaves_ledger_and_stored_note_untouched() {
    let (mut session, _chat_store, chat_session_id, _tmp_dir, ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done], // wird unten sofort ersetzt
            MockSshTransport::default().with_response("cat notes.log", output("ok")),
        )
        .await;
    // Zwei Runden: die ERSTE wird von der Kompaktierung selbst
    // verbraucht (die 6 synthetischen Runden unten lösen vor dem
    // eigentlichen Chat-Aufruf eine Zusammenfassung aus), erst die
    // ZWEITE ist der tatsächliche Chat-Turn.
    session.parts_mut_for_tests().ai_provider = Box::new(MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::TextDelta("Zusammenfassung der alten Runden.".to_string()),
            AiEvent::Done,
        ],
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "cat notes.log".to_string(),
            }),
            AiEvent::Done,
        ],
    ]));
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    let parts = crate::compaction::SystemContextParts {
        base: "Basis".to_string(),
        note_sections: vec![(
            "Server \"web-01\"".to_string(),
            "Wichtige Notiz".to_string(),
        )],
    };
    {
        let mut ctx = session.context.lock().await;
        ctx.system_context = parts.assemble();
    }
    session.parts_mut_for_tests().system_context_parts = AsyncMutex::new(parts.clone());
    push_synthetic_rounds(&session, 6, 5_000).await;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    // Die gespeicherte Notiz (in `SystemContextParts`, die Quelle der
    // Wahrheit für den nächsten `send_chat_message_impl`-Aufbau) bleibt
    // exakt wie zu Beginn.
    assert_eq!(
        session.system_context_parts.lock().await.note_sections,
        parts.note_sections
    );

    // Ledger: der zuvor ausgeführte Befehl (aus der ersten Runde des
    // echten Chat-Turns) muss weiterhin vollständig vorhanden sein —
    // die Kompaktierung der VORHER synthetisch angehängten Runden darf
    // daran nichts ändern.
    let entries = ledger_store.load_entries(chat_session_id).await.unwrap();
    assert!(
        entries.iter().any(|e| matches!(
            &e.content,
            ssh_manager_core::audit::LedgerEntryContent::CommandExecuted { command, .. }
                if command == "cat notes.log"
        )),
        "das Ledger muss den ausgeführten Befehl unabhängig von der Kompaktierung \
         enthalten: {entries:?}"
    );

    // spec-reviewer-Fund (Review dieses Schritts): `MockAiProvider`
    // liefert bei einer erschöpften Runden-Queue still `[AiEvent::
    // Done]` zurück (bequemer Test-Default) — ohne diese Assertion
    // könnte ein Bug, der die Kompaktierung/Zusammenfassung komplett
    // überspringt, unbemerkt bleiben: die ERSTE konfigurierte Runde
    // (die eigentlich für den Zusammenfassungs-Aufruf gedacht ist)
    // würde dann einfach direkt als Kommando-Vorschlag durchgehen, und
    // der Test bliebe trotzdem grün. Diese Assertion beweist
    // unabhängig davon, dass tatsächlich eine Zusammenfassung erzeugt
    // wurde.
    assert_eq!(
        session
            .summary
            .lock()
            .await
            .as_ref()
            .map(|s| s.text.as_str()),
        Some("Zusammenfassung der alten Runden."),
        "die Kompaktierung muss tatsächlich eine Zusammenfassung erzeugt haben, nicht nur \
         zufällig dieselbe Ledger-/Notiz-Aussage über einen anderen Pfad erfüllt haben"
    );
}

/// **Der kritische 0039-Wechselwirkungs-Test** (explizit von der
/// Aufgabenstellung verlangt, nach der Etappe-2-Regression): eine
/// Runde, die untrusted Content enthielt und `untrusted_content_
/// ingested` gesetzt hat, wird später von der Kompaktierung zu einer
/// Zusammenfassung verdichtet — die Post-Ingest-Eskalation
/// (`AutoExec` → `Confirm`, Spec 0039 §5.1) muss für eine DANACH neu
/// vorgeschlagene Aktion trotzdem weiter greifen. Beweist die
/// Invariante strukturell: `untrusted_content_ingested` ist ein
/// eigenständiges, monotones Flag (gesetzt beim tatsächlichen
/// Ausführen/Ingest, s. `execute_suggested_command`), nicht aus
/// `context.history` zur Sendezeit abgeleitet — Kompaktierung/
/// Zusammenfassung können es deshalb strukturell nicht "vergessen".
#[tokio::test]
async fn test_untrusted_content_escalation_survives_round_summarization() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![
            AiEvent::TextDelta("Zusammenfassung der alten Runde.".to_string()),
            AiEvent::Done,
        ]),
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().post_ingest_policy = PostIngestPolicy::Strict;
    session.parts_mut_for_tests().model_context_window_tokens = 500; // winzig, erzwingt Kompaktierung schnell

    // Runde 0: der ursprüngliche untrusted-Content-Ingest (wie
    // `execute_suggested_command` es täte — dort wird der Flag exakt
    // an dieser Stelle gesetzt, s. dortiger Kommentar).
    {
        let mut ctx = session.context.lock().await;
        let mut flags = session.mcp_origin_flags.lock().unwrap();
        ctx.history.push(user_msg("Zeig mir die Logs"));
        flags.push(false);
        ctx.history
            .push(command_result_msg("cat app.log", "verdächtige Zeile"));
        flags.push(false);
    }
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);

    // Weitere Runden, um Runde 0 aus dem erhaltenen Fenster zu drängen.
    push_synthetic_rounds(&session, 5, 200).await;

    // Kompaktierung auslösen — Runde 0 muss dabei aus dem gesendeten
    // Kontext verschwinden (in eine Zusammenfassung verdichtet).
    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let compacted = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;
    assert!(
        !compacted.history.iter().any(
            |m| matches!(&m.content, MessageContent::CommandResult { command, .. } if command == "cat app.log")
        ),
        "Runde 0 (mit dem untrusted Content) muss aus dem gesendeten Kontext verdichtet \
         worden sein, sonst beweist dieser Test nichts: {:?}",
        compacted.history
    );

    // Das Flag bleibt gesetzt ...
    assert!(
        session
            .untrusted_content_ingested
            .load(std::sync::atomic::Ordering::SeqCst),
        "untrusted_content_ingested darf durch Kompaktierung/Zusammenfassung nie \
         zurückgesetzt werden"
    );

    // ... und eine NEU vorgeschlagene, per Allow-Regel eigentlich
    // AutoExec-fähige Aktion muss trotzdem zu `Confirm` eskaliert
    // werden (Spec 0039, Abschnitt 5.1: `Strict` -> jede AutoExec-
    // Aktion wird eskaliert).
    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        },
    )
    .await;
    assert!(
        matches!(decision, Decision::Confirm { .. }),
        "Post-Ingest-Eskalation muss trotz Verdichtung der ursprünglichen Runde weiter \
         greifen, war: {payload}"
    );
}

/// Spec 0040, Abschnitt 4 (Regressionstest, "Verbindliche Entscheidung
/// aus der Spec"): eine MCP-Aktion, die dieselbe `Session` (samt
/// `chat_session_store`/`chat_session_id`) eines bereits offenen
/// Menschen-Tabs mitnutzt, darf trotzdem keine Zeile in dessen
/// persistierter, wiederaufnehmbarer Historie erzeugen — sie bleibt
/// reines In-Memory-/Live-UI-Verhalten. Prüft zugleich, dass
/// `untrusted_content_ingested` (Spec 0039, Abschnitt 5) durch diese
/// Persistenz-Unterdrückung NICHT umgangen wird — der Flag muss trotzdem
/// gesetzt werden, weil er rein in-memory lebt und nie über
/// `push_history`/`push_history_scoped` läuft.
#[tokio::test]
async fn test_mcp_action_on_shared_human_session_writes_no_persisted_history() {
    let (mut session, chat_store, chat_session_id, _tmp_dir) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    // `AllowEverythingPolicyStore`: würde bei `ActionOrigin::Internal`
    // zu AutoExec führen — bei MCP-Ursprung erzwingt die Filter-Engine
    // trotzdem `Confirm` (s. `test_mcp_origin_downgrades_autoexec_to_
    // confirm_despite_allow_rule` oben), der Test simuliert daher die
    // menschliche Genehmigung über `approve_first_proposed_action`.
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    let mock_sftp = MockSftpSession::new().with_file(
        "/home/deploy/app.conf",
        b"host=localhost\npassword=hunter2\n".to_vec(),
    );
    session.set_sftp_for_tests(Box::new(mock_sftp)).await;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let action_future = handle_action_proposed(
        &session,
        session_id,
        AiAction::ReadRemoteFile {
            path: "/home/deploy/app.conf".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    // Kernaussage: kein einziger Eintrag in der persistierten,
    // wiederaufnehmbaren Historie — obwohl dieselbe `chat_session_id`
    // eines Menschen-Tabs verwendet wurde.
    let loaded = chat_store.load_session(chat_session_id).await.unwrap();
    assert!(
        loaded.is_empty(),
        "MCP-Herkunft darf nie in die persistierte Historie schreiben, geladen: {loaded:?}"
    );

    // Live im UI/In-Memory-Kontext erscheint der Dateiinhalt trotzdem —
    // nur der DB-Schreibzugriff wurde unterdrückt, nicht das In-Memory-
    // Verhalten (s. `push_history_scoped`-Doc-Kommentar).
    let history = session.context.lock().await.history.clone();
    assert!(
        history.iter().any(|m| matches!(
            &m.content,
            MessageContent::Text(t) if t.contains("host=localhost")
        )),
        "der Dateiinhalt muss trotzdem live im In-Memory-Kontext erscheinen: {history:?}"
    );

    // `untrusted_content_ingested` bleibt unabhängig von der
    // Persistenz-Unterdrückung funktionsfähig.
    assert!(
        session
            .untrusted_content_ingested
            .load(std::sync::atomic::Ordering::SeqCst),
        "untrusted_content_ingested darf durch die MCP-Persistenz-Unterdrückung nicht umgangen werden"
    );
}

/// Spec 0057, Nachtrag (MCP-Ausschluss aus der rollierenden Summary,
/// Stefans Entscheidung s. ADR 0049): MCP- und Chat-Runden mischen sich
/// in EINER Session (Teil 0 der Aufgabenstellung — bestätigt über
/// `mcp_backend::AppMcpBackend::ensure_session`, das dieselbe `Session`
/// eines bereits offenen Menschen-Tabs wiederverwendet). Nach
/// Kompaktierung/Faltung darf die persistierte Summary keinen
/// MCP-Content enthalten — das Ledger dagegen weiterhin den vollen
/// MCP-Content (Audit-Funktion, Etappe 1, unberührt von diesem
/// Ausschluss).
#[tokio::test]
async fn test_mcp_rounds_excluded_from_persisted_summary_but_retained_in_ledger() {
    let (mut session, chat_store, chat_session_id, _tmp_dir, ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done], // unten sofort ersetzt, s. `received_contexts`
            MockSshTransport::default()
                .with_response("cat mcp_secret.log", output("MCP_GEHEIM_KENNUNG_42")),
        )
        .await;
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    // `received_contexts` erfasst den TATSÄCHLICH an den Provider
    // gesendeten Request der Zusammenfassungs-KI-Anfrage — die einzige
    // Stelle, an der sich beweisen lässt, dass MCP-Content NICHT in die
    // Faltung eingeht (der kanonische Mock-Text unten wäre sonst
    // unabhängig vom tatsächlichen Input immer derselbe und würde die
    // Aussage nicht beweisen).
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    session.parts_mut_for_tests().ai_provider = Box::new(MockAiProvider {
        rounds: StdMutex::new(
            vec![vec![
                AiEvent::TextDelta("Zusammenfassung der Chat-Runden.".to_string()),
                AiEvent::Done,
            ]]
            .into(),
        ),
        received_contexts: received_contexts.clone(),
    });

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    // Runde 0: MCP-Ursprung, auf DERSELBEN Session wie ein
    // Menschen-Tab (s. `test_mcp_action_on_shared_human_session_writes_
    // no_persisted_history` oben) — landet im Ledger, nie in
    // `chat_messages`, aber sehr wohl im In-Memory-`context.history`
    // (und damit als Kandidat für die Summary-Faltung, wäre da nicht
    // der neue Ausschluss).
    let action_future = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "cat mcp_secret.log".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    // Weitere, echte Chat-Runden drängen Runde 0 aus dem erhaltenen
    // Fenster und lösen die Kompaktierung/Faltung aus.
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    // Kernaussage 1: der tatsächlich an die Zusammenfassungs-KI
    // gesendete Request enthielt den MCP-Content nicht — das ist die
    // eigentliche Faltungs-Eingabe, nicht nur ihr (im Mock ohnehin
    // kanonischer) Text-Output.
    let summarization_requests = received_contexts.lock().unwrap().clone();
    assert!(
        !summarization_requests.is_empty(),
        "die Kompaktierung muss tatsächlich einen Zusammenfassungs-Aufruf ausgelöst haben"
    );
    for request in &summarization_requests {
        let sent_text = format!("{request:?}");
        assert!(
            !sent_text.contains("MCP_GEHEIM_KENNUNG_42") && !sent_text.contains("mcp_secret.log"),
            "MCP-Content darf nicht in den Zusammenfassungs-Request eingehen: {sent_text}"
        );
    }

    // Kernaussage 1b: entsprechend enthält auch die persistierte
    // Summary (die aus genau diesem Provider-Aufruf hervorgeht) keinen
    // MCP-Content.
    let loaded_summary = chat_store.load_summary(chat_session_id).await.unwrap();
    let summary_text = loaded_summary
        .as_ref()
        .map(|(text, _)| text.as_str())
        .unwrap_or_default();
    assert!(
        !summary_text.contains("MCP_GEHEIM_KENNUNG_42"),
        "MCP-Content darf nicht in die persistierte Summary gefaltet werden: {summary_text}"
    );

    // Kernaussage 2: das Ledger enthält den MCP-Content weiterhin
    // vollständig — sowohl den MCP-originierten Vorschlag (`source:
    // McpAgent`, s. `test_ledger_captures_mcp_origin_independent_of_
    // chat_persist_flag` oben: die AUSFÜHRUNG selbst trägt `source:
    // User`, weil ein Mensch bestätigt hat, das Kommando blieb aber
    // MCP-initiiert) als auch die tatsächliche Kommandoausgabe.
    let entries = ledger_store.load_entries(chat_session_id).await.unwrap();
    let mcp_proposal_present = entries.iter().any(|e| {
        e.source == LedgerSource::McpAgent
            && matches!(
                &e.content,
                LedgerEntryContent::CommandProposed { command }
                    if command == "cat mcp_secret.log"
            )
    });
    assert!(
        mcp_proposal_present,
        "das Ledger muss den MCP-originierten Vorschlag mit `source: McpAgent` behalten: \
         {entries:?}"
    );
    let executed_output_present = entries.iter().any(|e| {
        matches!(
            &e.content,
            LedgerEntryContent::CommandExecuted { output, .. }
                if String::from_utf8_lossy(&output.stdout).contains("MCP_GEHEIM_KENNUNG_42")
        )
    });
    assert!(
        executed_output_present,
        "das Ledger muss die volle MCP-Kommandoausgabe behalten: {entries:?}"
    );
}

/// Spec 0057, Nachtrag: der Randfall aus `compact_rounds_with_summary`
/// — sind ALLE neu zu kürzenden Runden MCP-originiert, gibt es nichts
/// Chat-Relevantes zu fassen. Kein KI-Aufruf, aber die MCP-Runden
/// verschwinden trotzdem aus dem gesendeten Kontext (wie jede gekürzte
/// Runde), und `rounds_covered` rückt trotzdem vor (kein Platzhalter-
/// Vakuum bei der nächsten Kompaktierung).
#[tokio::test]
async fn test_compaction_skips_ai_call_when_all_newly_cut_rounds_are_mcp() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(vec![vec![AiEvent::Done]].into()),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    {
        let mut ctx = session.context.lock().await;
        let mut flags = session.mcp_origin_flags.lock().unwrap();
        // Sechs rein MCP-originierte Runden — genug, um die Kürzung
        // auszulösen, aber ohne jeden Chat-relevanten Inhalt.
        for i in 0..6 {
            ctx.history.push(user_msg(&format!("MCP-Frage {i}")));
            flags.push(true);
            ctx.history.push(command_result_msg(
                &format!("mcp-cmd-{i}"),
                &"x".repeat(5_000),
            ));
            flags.push(true);
        }
    }

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    assert!(
        received_contexts.lock().unwrap().is_empty(),
        "sind alle neu zu kürzenden Runden MCP-originiert, darf kein KI-Aufruf \
         stattfinden — es gibt nichts Chat-Relevantes zu fassen"
    );
    // Der plain Etappe-2-Platzhalter (keine bestehende Summary, auf die
    // zurückgegriffen werden könnte) muss stehen — Beweis, dass der
    // neue "alle neu geschnittenen Runden sind MCP"-Zweig tatsächlich
    // gegriffen hat, statt eines echten KI-Aufrufs.
    assert!(
        result.history.iter().any(
            |m| matches!(&m.content, MessageContent::Text(t) if t.contains("ältere Konversation gekürzt"))
        ),
        "der generische Kürzungs-Platzhalter muss stehen: {:?}",
        result.history
    );
    // Die tatsächlich GESCHNITTENEN Runden (die ältesten) müssen aus
    // dem gesendeten Kontext verschwunden sein — nur die zuletzt
    // erhaltenen Runden dürfen noch da sein.
    let remaining_mcp_rounds = result
        .history
        .iter()
        .filter(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("MCP-Frage")))
        .count();
    assert!(
        remaining_mcp_rounds < 6,
        "mindestens die ältesten MCP-Runden müssen geschnitten worden sein: {:?}",
        result.history
    );
}

/// spec-reviewer-Fund (Review dieses Nachtrags, Punkt 1 — der
/// gewichtigste Fund): MCP-Aktionen pushen ausnahmslos
/// `Role::ActionResult`, nie `Role::User` — im geteilten-Session-
/// Regelfall (MCP nutzt die Session eines bereits offenen
/// Menschen-Tabs, s. `test_mcp_action_on_shared_human_session_writes_
/// no_persisted_history` oben) hängt sich eine MCP-Aktion deshalb an
/// die LAUFENDE Menschen-Runde an, statt eine eigene zu bilden. Eine
/// rundenweise Verdichtung ("irgendeine Nachricht ist MCP ⇒ ganze
/// Runde raus") würde in genau diesem Fall echten Chat-Inhalt
/// derselben Runde mit aus der Faltung reißen — der
/// Kontinuitätsverlust, den Etappe 3 gerade verhindern soll. Dieser
/// Test bildet exakt diese Mischung nach und beweist die
/// nachrichtenweise (nicht rundenweise) Filterung in
/// `group_mcp_flags_by_round`/`compact_rounds_with_summary`.
#[tokio::test]
async fn test_mcp_action_within_existing_chat_round_only_excludes_the_mcp_message() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]), // unten sofort ersetzt
        MockSshTransport::default()
            .with_response("cat mcp_secret.log", output("MCP_GEHEIM_KENNUNG_77")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    session.parts_mut_for_tests().ai_provider = Box::new(MockAiProvider {
        rounds: StdMutex::new(
            vec![vec![
                AiEvent::TextDelta("Zusammenfassung.".to_string()),
                AiEvent::Done,
            ]]
            .into(),
        ),
        received_contexts: received_contexts.clone(),
    });

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    // Runde 0: erst eine ECHTE Chat-Runde (Nutzerfrage + Kommando-
    // ergebnis, beide nicht-MCP) — ein eindeutiger Text, NICHT über
    // `push_synthetic_rounds` (das unten für die weiteren Runden
    // erneut ab "Frage 0" zählt und den Text sonst kollidieren ließe,
    // wodurch die Kernaussage unten selbst dann grün liefe, wenn
    // Runde 0 fälschlich komplett ausgeschlossen würde).
    {
        let mut ctx = session.context.lock().await;
        let mut flags = session.mcp_origin_flags.lock().unwrap();
        ctx.history.push(user_msg("Ursprüngliche Chat-Frage"));
        flags.push(false);
        ctx.history
            .push(command_result_msg("echte-chat-runde", "ok"));
        flags.push(false);
    }
    // ... dann, OHNE neue `Role::User`-Nachricht dazwischen, eine
    // MCP-Aktion auf DERSELBEN Runde — genau der geteilte-Session-
    // Regelfall aus Teil 0 der Aufgabenstellung.
    let action_future = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "cat mcp_secret.log".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    // Weitere, echte Chat-Runden drängen Runde 0 aus dem erhaltenen
    // Fenster und lösen die Kompaktierung/Faltung aus.
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.context.lock().await.clone();
    let parts = session.system_context_parts.lock().await.clone();
    crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.model_context_window_tokens,
    )
    .await;

    let summarization_requests = received_contexts.lock().unwrap().clone();
    assert!(
        !summarization_requests.is_empty(),
        "die Kompaktierung muss tatsächlich einen Zusammenfassungs-Aufruf ausgelöst haben"
    );
    for request in &summarization_requests {
        let sent_text = format!("{request:?}");
        assert!(
            !sent_text.contains("MCP_GEHEIM_KENNUNG_77") && !sent_text.contains("mcp_secret.log"),
            "MCP-Content darf nicht in den Zusammenfassungs-Request eingehen: {sent_text}"
        );
        // Kernaussage: der ECHTE Chat-Inhalt DERSELBEN Runde (Runde 0,
        // durch die MCP-Aktion nur ERGÄNZT, nicht ersetzt) muss trotzdem
        // in der Zusammenfassung landen — eine rundenweise Verdichtung
        // hätte ihn fälschlich mit ausgeschlossen.
        assert!(
            sent_text.contains("Ursprüngliche Chat-Frage"),
            "der echte Chat-Inhalt derselben Runde darf NICHT mitausgeschlossen werden, \
             nur weil dieselbe Runde auch eine MCP-Nachricht enthält: {sent_text}"
        );
    }
}

// --- Spec 0040, Abschnitt 5: nur-additive Re-Redaction vor `send()` ----

/// Simuliert genau den Fall, der Abschnitt 5 motiviert: eine Nachricht,
/// die entstand, BEVOR ein bestimmtes Redaction-Muster existierte (hier
/// über `DefaultOutputRedactor::with_extra_patterns` als "nachträglich
/// hinzugefügte Regel" simuliert), landet unredigiert in der Historie.
/// `reapply_redaction_for_send` muss sie beim nächsten `send()` trotzdem
/// redigieren — sowohl im freien `Text`- als auch im
/// `CommandResult.output`-Feld.
#[test]
fn test_reapply_redaction_for_send_redacts_with_retroactively_added_pattern() {
    let retroactive_pattern =
        regex::Regex::new(r"sudo_geheim_[a-z0-9]+").expect("Testmuster ist gültig");
    let redactor = DefaultOutputRedactor::with_extra_patterns(vec![retroactive_pattern]);

    let history = vec![
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text(
                "das sudo-Passwort ist sudo_geheim_abc123, bitte merken".to_string(),
            ),
        },
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: "cat notes.txt".to_string(),
                output: output("Notiz: sudo_geheim_abc123 verwenden"),
                cancelled: false,
            },
        },
    ];

    let redacted = reapply_redaction_for_send(history, &redactor);

    match &redacted[0].content {
        MessageContent::Text(t) => {
            assert!(
                !t.contains("sudo_geheim_abc123"),
                "das nachträglich hinzugefügte Muster muss auch Text-Inhalte redigieren: {t}"
            );
            assert!(t.contains("[REDACTED]"));
        }
        other => panic!("Text-Variante erwartet, war: {other:?}"),
    }
    match &redacted[1].content {
        MessageContent::CommandResult {
            command, output, ..
        } => {
            assert_eq!(command, "cat notes.txt", "Kommandotext bleibt unangetastet");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout.contains("sudo_geheim_abc123"),
                "CommandResult.output muss ebenfalls redigiert werden: {stdout}"
            );
        }
        other => panic!("CommandResult-Variante erwartet, war: {other:?}"),
    }
}

/// Kritischer Gegentest zur "nur additiv"-Anforderung: Inhalt, der
/// bereits redigiert ist (enthält schon den `[REDACTED]`-Platzhalter),
/// darf durch einen erneuten Redaction-Durchlauf niemals wieder
/// sichtbar/verändert werden — der Durchlauf darf ausschließlich NEU
/// erkannte Treffer ersetzen, nie etwas rückgängig machen oder anders
/// transformieren.
#[test]
fn test_reapply_redaction_for_send_never_reverses_existing_redaction() {
    let redactor = DefaultOutputRedactor::new();
    // Bewusst OHNE ein "password="/"token="-artiges Schlüsselwort direkt
    // vor dem Platzhalter — sonst würde das eingebaute Credential-Muster
    // den Platzhalter selbst (erneut, aber weiterhin sicher) treffen und
    // "password=[REDACTED]" zu "[REDACTED]" zusammenziehen; das ist kein
    // Bug (es wird nichts sichtbar, nur zusätzlich redigiert), würde
    // hier aber nur die eigentliche Testaussage verwässern.
    let already_redacted =
        "Kommentar: das Secret wurde bereits entfernt ([REDACTED]), alles gut.".to_string();

    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(already_redacted.clone()),
    }];

    let redacted = reapply_redaction_for_send(history, &redactor);

    match &redacted[0].content {
        MessageContent::Text(t) => assert_eq!(
            t, &already_redacted,
            "bereits redigierter Inhalt muss durch einen erneuten Durchlauf unverändert \
             bleiben — insbesondere darf der Platzhalter selbst nie wieder aufgelöst werden"
        ),
        other => panic!("Text-Variante erwartet, war: {other:?}"),
    }
}

/// Ergänzung zum Test oben: selbst wenn der Platzhalter unmittelbar auf
/// ein Schlüsselwort wie `password=` folgt (das eingebaute
/// Credential-Muster matcht dann erneut, s. Kommentar oben), bleibt das
/// Ergebnis sicher — der Platzhalter bleibt bestehen, es wird nirgendwo
/// ursprünglicher Klartext sichtbar, nur ggf. noch etwas kompakter
/// redigiert.
#[test]
fn test_reapply_redaction_for_send_stays_safe_even_when_pattern_matches_placeholder_again() {
    let redactor = DefaultOutputRedactor::new();
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(
            "Verbindung ok, password=[REDACTED], danach normal weitergemacht".to_string(),
        ),
    }];

    let redacted = reapply_redaction_for_send(history, &redactor);

    match &redacted[0].content {
        MessageContent::Text(t) => {
            assert!(
                t.contains("[REDACTED]"),
                "der Platzhalter darf nie verschwinden: {t}"
            );
            assert!(
                !t.to_lowercase().contains("hunter") && !t.contains("password=hunter"),
                "kein Klartext-Secret darf jemals sichtbar werden: {t}"
            );
        }
        other => panic!("Text-Variante erwartet, war: {other:?}"),
    }
}

/// Unabhängiger Review-Pass zu Spec 0040, Abschnitt 5: ein bereits
/// gefencter `<remote_file>`-Eintrag (wie ihn `execute_read_remote_
/// file` in `session.context`/der DB ablegt), dessen Inhalt einen
/// abgeschnittenen Private-Key-Block OHNE `END`-Marker enthält, würde
/// mit dem gierigen Rückfallmuster (`(?s)-----BEGIN ... PRIVATE
/// KEY-----.*`, matcht bis zum Ende der Zeichenkette) auch das
/// schließende `</remote_file>`-Tag mitfressen — die Fencing-Garantie
/// aus Spec 0039 wäre damit verletzt (kaputter Fence = genau die
/// Lücke, durch die eingeschleuste Anweisungen wieder als
/// vertrauenswürdiger Kontext durchrutschen könnten), obwohl die
/// Redaction-Richtung selbst sicher bleibt (nur Löschung, keine
/// Enthüllung). Nach dem Fix muss der schließende Tag intakt bleiben.
#[test]
fn test_reapply_redaction_for_send_never_breaks_an_already_fenced_closing_tag() {
    let redactor = DefaultOutputRedactor::new();
    let truncated_key = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqh...(abgeschnitten)";
    let fenced = fence_untrusted(
        UntrustedKind::RemoteFile,
        "/home/deploy/id_rsa",
        truncated_key,
    );
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(format!("Inhalt von 'id_rsa':\n\n{fenced}")),
    }];

    let redacted = reapply_redaction_for_send(history, &redactor);

    match &redacted[0].content {
        MessageContent::Text(t) => {
            assert_eq!(
                t.matches("</remote_file>").count(),
                1,
                "der schließende Fence-Tag muss erhalten bleiben, tatsächlicher Text: {t}"
            );
            assert!(
                t.trim_end().ends_with("</remote_file>"),
                "der Fence muss korrekt geschlossen sein, tatsächlicher Text: {t}"
            );
            assert!(
                !t.contains("MIIEvQIBADANBgkqh"),
                "der Key-Inhalt selbst muss weiterhin redigiert sein: {t}"
            );
        }
        other => panic!("Text-Variante erwartet, war: {other:?}"),
    }
}

/// Ergänzung zum Test oben: ein tatsächliches Secret innerhalb bereits
/// gefencten Inhalts wird weiterhin redigiert — der Fix schwächt die
/// Redaction nicht ab, er ordnet sie nur relativ zu den Fence-Grenzen.
#[test]
fn test_reapply_redaction_for_send_still_redacts_a_real_secret_inside_fenced_content() {
    let redactor = DefaultOutputRedactor::new();
    let fenced = fence_untrusted(
        UntrustedKind::RemoteFile,
        "/etc/app.conf",
        "host=localhost\npassword=hunter2geheim\n",
    );
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(fenced),
    }];

    let redacted = reapply_redaction_for_send(history, &redactor);

    match &redacted[0].content {
        MessageContent::Text(t) => {
            assert!(
                !t.contains("hunter2geheim"),
                "das Secret muss redigiert sein: {t}"
            );
            assert!(t.contains("[REDACTED]"));
            assert_eq!(t.matches("</remote_file>").count(), 1);
            assert!(t.trim_end().ends_with("</remote_file>"));
        }
        other => panic!("Text-Variante erwartet, war: {other:?}"),
    }
}

/// Realistischerer End-to-End-Aufbau der beiden Tests oben
/// (unabhängiger Review-Pass zum Fencing-Fix selbst: die eingebaute
/// Private-Key-Fail-safe-Regel ist immer aktiv, ein wörtlicher
/// abgeschnittener Key wäre also schon bei `execute_read_remote_file`
/// redigiert worden — die obigen Tests konstruieren die Historie
/// deshalb direkt, statt den echten Lese-Pfad zu durchlaufen). Dieser
/// Test geht stattdessen exakt den Weg, auf dem der Bug tatsächlich
/// auftreten kann (Spec 0040, Abschnitt 5s eigenes Szenario:
/// "nachträglich hinzugefügtes Muster"): eine Datei mit einem beim
/// Lesen NOCH nicht erkannten Secret-Format wird gelesen und gefenct
/// (Redactor ohne das Muster), danach wird ein Redactor MIT dem
/// (gierigen, unterminierten) Zusatzmuster für den Versand verwendet.
#[tokio::test]
async fn test_read_remote_file_then_send_with_retroactive_greedy_pattern_keeps_fence_intact() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::ReadRemoteFile {
                path: "/home/deploy/legacy_secret.pem".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    // Beim Lesen/Fencen noch unbekanntes Secret-Format — kein Muster
    // im (Standard-)Redactor dieser Session erkennt es, es landet
    // deshalb unredigiert im gefencten `MessageContent::Text`.
    let mock_sftp = MockSftpSession::new().with_file(
        "/home/deploy/legacy_secret.pem",
        b"-----BEGIN LEGACY SECRET-----\nunbekanntesFormatOhneEndemarker...".to_vec(),
    );
    session
        .set_sftp_for_tests(Box::new(mock_sftp.clone()))
        .await;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    // Spec 0068, Teil 2: ein Secret-Pfad verlangt jetzt immer eine
    // Bestätigung (auch mit Allow-Regel) — den Dialog beantworten.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            resolve_first_confirm(&emitter, &confirmations, ActionUserDecision::Approve),
        )
    })
    .await
    .expect("Turn muss nach der Bestätigung enden");

    let history = session.context.lock().await.history.clone();
    let fenced_entry = history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::Text(t) if t.contains("<remote_file>") => Some(t.clone()),
            _ => None,
        })
        .expect("erwartet: ein gefenceter <remote_file>-Eintrag im Kontext");
    assert!(
        fenced_entry.contains("unbekanntesFormatOhneEndemarker"),
        "zum Lesezeitpunkt noch unbekanntes Format darf noch nicht redigiert sein: \
         {fenced_entry}"
    );
    assert!(fenced_entry.trim_end().ends_with("</remote_file>"));

    // Zeit vergeht, ein neues, gieriges (unterminiertes) Muster für
    // genau dieses Format wird ergänzt — Spec 0040, Abschnitt 5s
    // eigenes "nachträglich hinzugefügtes Muster"-Szenario.
    let retroactive_pattern =
        regex::Regex::new(r"(?s)-----BEGIN LEGACY SECRET-----.*").expect("Testmuster ist gültig");
    let stronger_redactor = DefaultOutputRedactor::with_extra_patterns(vec![retroactive_pattern]);

    let redacted = reapply_redaction_for_send(history, &stronger_redactor);
    let redacted_entry = redacted
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::Text(t) if t.contains("<remote_file>") => Some(t.clone()),
            _ => None,
        })
        .expect("gefenceter Eintrag muss weiterhin vorhanden sein");

    assert!(
        !redacted_entry.contains("unbekanntesFormatOhneEndemarker"),
        "das nachträglich erkannte Secret muss jetzt redigiert sein: {redacted_entry}"
    );
    assert_eq!(
        redacted_entry.matches("</remote_file>").count(),
        1,
        "der schließende Fence-Tag muss trotz des gierigen Musters erhalten bleiben: \
         {redacted_entry}"
    );
    assert!(redacted_entry.trim_end().ends_with("</remote_file>"));
}

/// End-to-End: `session.context` (die für Persistenz/UI maßgebliche
/// Historie) bleibt exakt so, wie sie war — nur die tatsächlich an
/// `AiProvider::send()` übergebene Kopie ist zusätzlich redigiert. Deckt
/// damit beide Hälften von Abschnitt 5 gleichzeitig ab: die Re-Redaction
/// wirkt (dank `received_contexts_handle`, s. `MockAiProvider`
/// sichtbar), und sie verändert nirgendwo außerhalb dieser einen Kopie
/// etwas.
#[tokio::test]
async fn test_send_to_ai_provider_is_redacted_without_altering_persisted_context() {
    // Spec 0078 (T-A13): zusätzlich eine Verbindungs-URL, deren
    // BENUTZERNAME ein `@` enthält — bis Spec 0078 ging `pw123` hier
    // vollständig im Klartext an den KI-Anbieter.
    let raw_secret_text =
        "Notiz: password=hunter2geheim nicht vergessen, postgres://svc@tenant:pw123@db/x"
            .to_string();
    let ai_provider = MockAiProvider::new(vec![AiEvent::Done]);
    let received_contexts = ai_provider.received_contexts_handle();
    let mut session = session_with_ai_provider(ai_provider, MockSshTransport::default());
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    // Simuliert eine Nachricht, die (aus welchem Grund auch immer, z. B.
    // eine ältere Redactor-Version) unredigiert in der Historie
    // gelandet ist — direkt in den In-Memory-Kontext geschrieben, ohne
    // über `push_history` bzw. dessen normalen Redaction-Pfad zu laufen.
    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text(raw_secret_text.clone()),
    });
    session.mcp_origin_flags.lock().unwrap().push(false);

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let sent = received_contexts.lock().unwrap().clone();
    assert!(
        !sent.is_empty(),
        "der Provider muss mindestens einmal aufgerufen worden sein"
    );
    let sent_text = sent[0]
        .history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::Text(t) if t.contains("Notiz:") => Some(t.clone()),
            _ => None,
        })
        .expect("die Nachricht muss (redigiert) beim Provider ankommen");
    assert!(
        !sent_text.contains("hunter2geheim"),
        "der an die KI gesendete Text muss redigiert sein: {sent_text}"
    );
    // Spec 0078 (T-A13): auch das Passwort hinter einem Benutzernamen
    // mit `@` darf den Anbieter nie im Klartext erreichen.
    assert!(
        !sent_text.contains("pw123"),
        "das URL-Passwort muss im Request an die KI redigiert sein: {sent_text}"
    );

    let context_after = session.context.lock().await.history.clone();
    assert!(
        context_after.iter().any(|m| matches!(
            &m.content,
            MessageContent::Text(t) if t == &raw_secret_text
        )),
        "der In-Memory-Kontext/die persistierte Historie muss unverändert (roh) bleiben, \
         nur die an den Provider gesendete Kopie wird redigiert: {context_after:?}"
    );
}

/// Spec 0057, §3: "vor jedem `AiProvider::send()`-Aufruf" wird nur die
/// an den Provider gesendete Kopie kompaktiert — die gespeicherte
/// Historie in der DB bleibt vollständig. Ein absichtlich winziges
/// `model_context_window_tokens` stellt sicher, dass die Kompaktierung
/// beim nächsten `send()`-Aufruf greifen MUSS, und prüft, dass
/// `load_session` danach trotzdem noch alle ursprünglichen Nachrichten
/// liefert.
#[tokio::test]
async fn test_context_truncation_for_provider_request_does_not_affect_persisted_history() {
    let (mut session, chat_store, chat_session_id, _tmp_dir) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().model_context_window_tokens = 1_000;

    // Zwei Nachrichten weit über dem winzigen Kontextfenster oben,
    // direkt über `push_history` (nicht über einen echten Turn) —
    // reicht, um die Vorbedingung für Kompaktierung zu erfüllen, ohne
    // den gesamten Turn-Mechanismus dafür zu bemühen.
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("a".repeat(30_000)),
        },
    )
    .await;
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("b".repeat(30_000)),
        },
    )
    .await;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let loaded = chat_store.load_session(chat_session_id).await.unwrap();
    let text_lengths: Vec<usize> = loaded
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::Text(t) => Some(t.chars().count()),
            _ => None,
        })
        .collect();
    assert!(
        text_lengths.contains(&30_000) && text_lengths.iter().filter(|&&l| l == 30_000).count() == 2,
        "beide 30.000-Zeichen-Nachrichten müssen weiterhin vollständig in der DB stehen: {text_lengths:?}"
    );
}

/// Spec 0088, T7 (A4.1, gegen `33d0501` rot): Ist die Sperre von
/// `mcp_origin_flags` vergiftet, panicken weder das Schreiben in
/// `push_history_scoped` noch das Lesen in der Kompaktierung.
///
/// Das Lesen ist die teurere Stelle: Es passiert **nach** dem `mem::take`
/// des Verlaufs aus dem Kontext (`compact_rounds_with_summary`). Ein Panic
/// dort liesse den Verlauf der Sitzung leer zurueck — der Chat waere weg.
#[tokio::test]
async fn test_a_poisoned_mcp_origin_flags_lock_breaks_neither_push_nor_compaction() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default(),
    );
    crate::poison::poison_for_test(&session.mcp_origin_flags);

    // Ein Verlauf, der die Kompaktierung sicher ausloest.
    for round in 0..6 {
        let text = format!("Runde {round}: {}", "x".repeat(4_000));
        push_history_scoped(&session, user_msg(&text), false).await;
    }

    let history_len = session.context.lock().await.history.len();
    let flags_len = crate::poison::lock_tolerating_poison(&session.mcp_origin_flags).len();
    assert_eq!(
        history_len, flags_len,
        "Verlauf und Flags muessen trotz vergifteter Sperre gleich lang bleiben"
    );
    assert_eq!(history_len, 6);

    let context = session.context.lock().await.clone();
    let parts = crate::compaction::SystemContextParts::default();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        context,
        &parts,
        1_000,
    )
    .await;

    assert!(
        !result.history.is_empty(),
        "die Kompaktierung darf den Verlauf nicht leer zuruecklassen"
    );
}
