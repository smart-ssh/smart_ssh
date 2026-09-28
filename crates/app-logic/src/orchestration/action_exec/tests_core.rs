//! Tests für den Aktions-Lebenszyklus (Vorschlag, Filter-Auswertung,
//! Risiko/Injection-Eskalation, Ausführung) — Spec 0083: reine Verschiebung
//! aus `orchestration::tests`, keine Verhaltensänderung.

use std::sync::Arc;

use async_trait::async_trait;
use ssh_manager_core::ai::{AiError, AiEvent, AiProvider, SessionContext};
use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyStore, Rule};

use crate::events::TestEmitter;
use crate::orchestration::run_chat_turn;
use crate::session::SessionManager;

use super::super::test_support::*;
use super::*;

#[tokio::test]
async fn test_autoexec_path_runs_command_and_records_result() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"]
    );
    let (_, proposed_payload) = &events[0];
    assert_eq!(proposed_payload["decision"], serde_json::json!("AutoExec"));

    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1);
    assert!(matches!(
        history[0].content,
        MessageContent::CommandResult { .. }
    ));
}

/// Spec 0065, Teil 2 (Regressionstest): eine Antwort, die mit
/// `AiEvent::TextTruncated` statt `AiEvent::Done` endet, muss (a) den
/// bis dahin gestreamten Text trotzdem in die Historie/den Ledger
/// übernehmen (genau wie bei `Done` — "bleibt sichtbar, ist gültig, nur
/// unvollständig") und (b) ein `chat-response-truncated`-Event für die
/// richtige Session auslösen, DAMIT das Frontend den Hinweis + „Weiter"
/// anzeigen kann — kein Text-Hinweis im Inhalt selbst (Lehre aus Spec
/// 0057, s. `emit_chat_response_truncated`-Doc-Kommentar).
#[tokio::test]
async fn test_text_truncated_event_keeps_partial_text_and_emits_notice() {
    let session = test_session(
        vec![
            AiEvent::TextDelta("Teil".to_string()),
            AiEvent::TextDelta("antwort".to_string()),
            AiEvent::TextTruncated,
        ],
        MockSshTransport::default(),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert!(
        event_names.contains(&"chat-response-truncated"),
        "erwartet ein chat-response-truncated-Event, bekam: {event_names:?}"
    );
    let (_, payload) = events
        .iter()
        .find(|(name, _)| name == "chat-response-truncated")
        .expect("chat-response-truncated fehlt");
    assert_eq!(payload["sessionId"], serde_json::json!(session_id));

    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].content,
        MessageContent::Text("Teilantwort".to_string())
    );
    assert!(matches!(history[0].role, Role::Assistant));
}

/// T11 (Spec 0080, A2): eine Runde, die nur mit `Done` endet (Runde 1
/// eines Turns — beantwortet direkt die Nutzer-Nachricht) → genau ein
/// `chat-response-empty`, keine Ledger-/Historie-Zeile (`flush_text_
/// buffer` schreibt bei leerem Puffer nichts, s. dortige Prüfung).
#[tokio::test]
async fn test_bare_done_round_emits_chat_response_empty_once_with_no_history_entry() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(event_names, vec!["chat-response-empty"]);
    let (_, payload) = events
        .iter()
        .find(|(name, _)| name == "chat-response-empty")
        .expect("chat-response-empty fehlt");
    assert_eq!(payload["sessionId"], serde_json::json!(session_id));

    let history = session.context.lock().await.history.clone();
    assert!(
        history.is_empty(),
        "eine leere Runde darf keine Historien-/Ledger-Zeile hinterlassen: {history:?}"
    );
}

/// T12 (Spec 0080, Wächter): Text + `Done` → kein `chat-response-empty`.
#[tokio::test]
async fn test_text_then_done_does_not_emit_chat_response_empty() {
    let session = test_session(
        vec![AiEvent::TextDelta("Hallo.".to_string()), AiEvent::Done],
        MockSshTransport::default(),
    );
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert!(
        !event_names.contains(&"chat-response-empty"),
        "eine Antwort mit Text darf nicht als leer gelten: {event_names:?}"
    );
}

/// T13 (Spec 0080, Wächter): eine Aktion wird in DERSELBEN Runde
/// vorgeschlagen, in der Text ausbleibt — kein `chat-response-empty`,
/// unabhängig vom späteren Ausgang der Aktion. Hier: AutoExec (wie
/// `test_autoexec_path_runs_command_and_records_result`), damit die
/// Runde ohne Warten auf eine nie eintreffende Bestätigung sofort mit
/// `Done` endet — `round_had_content` wird schon beim `ActionProposed`-
/// Event gesetzt, VOR jedem Warten auf `handle_action_proposed`, das
/// macht die genaue Art der Aktions-Auflösung für diesen Test
/// irrelevant.
#[tokio::test]
async fn test_action_proposed_without_text_in_the_same_round_does_not_emit_chat_response_empty() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert!(
        !event_names.contains(&"chat-response-empty"),
        "eine vorgeschlagene Aktion zählt als \"nicht leer\", auch ohne Text: {event_names:?}"
    );
}

/// T14 (Spec 0080, Wächter): `Error` → kein `chat-response-empty`.
#[tokio::test]
async fn test_error_round_does_not_emit_chat_response_empty() {
    let session = test_session(
        vec![AiEvent::Error(AiError::RateLimited)],
        MockSshTransport::default(),
    );
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert!(
        !event_names.contains(&"chat-response-empty"),
        "auf einen Fehler darf kein chat-response-empty folgen: {event_names:?}"
    );
    assert!(event_names.contains(&"chat-error"));
}

/// Klarstellung Q-BL-0259-01 (Spec 0080, Variante b), erster
/// Wächter-Test aus der Entscheidung: eine in Runde 1 AUSGEFÜHRTE
/// Aktion löst eine automatische Folgerunde aus (Spec 0021, Abschnitt
/// 3); endet die ohne Text nur mit `Done`, bleibt das still — kein
/// `chat-response-empty`. Scheitert gegen eine wörtliche "jede
/// Runde"-Umsetzung von A2 — genau die Alternative, die die
/// Klarstellung in Spec 0080 §8 zugunsten von Variante (b) verwirft.
#[tokio::test]
async fn test_silent_followup_round_after_executed_action_does_not_emit_chat_response_empty() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.ai_provider = Box::new(MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        // Runde 2 (automatische Folgerunde nach der ausgeführten
        // Aktion) — bleibt ohne jeden Text, wie ein Modell, das der
        // Aktion nichts mehr hinzuzufügen hat.
        vec![AiEvent::Done],
    ]));
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"],
        "eine stille Folgerunde nach einer ausgeführten Aktion darf kein \
         chat-response-empty auslösen: {event_names:?}"
    );
}

/// Klarstellung Q-BL-0259-01 (Spec 0080, Variante b), zweiter
/// Wächter-Test aus der Entscheidung: eine WÄHREND der ersten Runde
/// eingereihte Nutzer-Nachricht (Spec 0066, §2) wird in Runde 2
/// beantwortet — endet die ohne Text nur mit `Done`, gilt sie trotzdem
/// als "beantwortet eine Nutzer-Nachricht" und löst
/// `chat-response-empty` aus (anders als eine gewöhnliche automatische
/// Folgerunde, s. Test oben).
#[tokio::test]
async fn test_followup_round_answering_a_queued_message_emits_chat_response_empty() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.ai_provider = Box::new(MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        // Runde 2 beantwortet die eingereihte Nachricht unten, bleibt
        // aber ohne jeden eigenen Text.
        vec![AiEvent::Done],
    ]));
    // Spec 0066, §2: identisch zu `commands::send_chat_message_impl`s
    // "es läuft schon ein Turn" -Zweig — hier direkt gesetzt, weil
    // dieser Test `run_chat_turn` ohne den umschließenden Tauri-Command
    // aufruft.
    session
        .chat_turn
        .lock()
        .unwrap()
        .queued
        .push("Und was ist mit /var/log?".to_string());
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec![
            "chat-action-proposed",
            "chat-action-result",
            "chat-queued-messages-sent",
            "chat-response-empty",
        ],
        "eine Runde, die eine eingereihte Nachricht beantwortet, zählt wie Runde 1: {event_names:?}"
    );
}

/// Spec 0028, Abschnitt 5 (Regressionstest, s. `ActionOrigin::Mcp`):
/// dieselbe Allow-Regel, die im Test oben (`ActionOrigin::Internal`) zu
/// `AutoExec` führt, muss bei `ActionOrigin::Mcp` trotzdem eine
/// Bestätigung erzwingen — ein externer MCP-Client darf interne
/// Allow-Regeln nie automatisch ausnutzen.
#[tokio::test]
async fn test_mcp_origin_downgrades_autoexec_to_confirm_despite_allow_rule() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let action_future = handle_action_proposed(
        &session,
        session_id,
        AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    let responder = deny_first_proposed_action(&emitter, &confirmations);
    // Rückgabewert ignoriert: `true` bedeutet hier "Folgerunde nötig"
    // (Spec 0021, Abschnitt 3, Fall 3 — auch eine Ablehnung löst das
    // aus), nicht "wurde ausgeführt". Ob tatsächlich ausgeführt wurde,
    // zeigt sich an `decision`/`history` unten, nicht am Rückgabewert.
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-proposed")
        .expect("chat-action-proposed muss gesendet worden sein");
    assert_eq!(
        proposed_payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_MCP_ORIGIN_REQUIRES_CONFIRM"),
        "eine Allow-Regel darf bei MCP-Ursprung nie zu AutoExec führen — war: {proposed_payload}"
    );

    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1);
    assert!(matches!(
        &history[0].content,
        MessageContent::ActionRejected {
            reason: RejectionReason::User,
            ..
        }
    ));
}

// --- Spec 0027: Abbruch lang laufender Kommandos ------------------------

/// Kernszenario: ein nie von selbst endendes Kommando (`journalctl -f`)
/// wird über die Registry abgebrochen — `execute_suggested_command`
/// muss zurückkehren (statt für immer zu hängen) und dabei die bereits
/// eingetroffene Teil-Ausgabe mit `cancelled: true` sowohl im
/// `chat-action-result`-Event als auch im Chat-Kontext-Eintrag tragen.
#[tokio::test]
async fn test_execute_suggested_command_cancellation_returns_partial_output() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_never_completing("journalctl -f"),
    );
    let emitter = TestEmitter::default();
    let action_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();

    let exec_future = execute_suggested_command(
        &session,
        session_id,
        action_id,
        "journalctl -f".to_string(),
        &emitter,
        true,
        LedgerSource::Ai,
    );
    let cancel_future = async {
        // Kleine Verzögerung, damit `exec_future` sicher schon
        // registriert hat und im `cancel.await` des Mocks steckt,
        // bevor hier aufgelöst wird — analog zu
        // `test_slow_session_does_not_block_concurrent_session_via_shared_manager`s
        // festen Verzögerungen oben.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        session
            .running_command_cancellations
            .resolve(&action_id, ())
            .expect("sollte eine wartende Abbruch-Registrierung finden");
    };

    let (executed, ()) = tokio::join!(exec_future, cancel_future);
    assert!(
        executed,
        "ein abgebrochenes Kommando zählt als \"ausgeführt\" (Ergebnis liegt vor)"
    );

    let events = emitter.events.lock().unwrap().clone();
    let (_, result_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-result")
        .expect("chat-action-result sollte gesendet worden sein");
    assert_eq!(
        result_payload["result"]["cancelled"],
        serde_json::json!(true)
    );
    assert_eq!(
        result_payload["result"]["stdout"],
        serde_json::json!("partial output before cancel")
    );
    assert_eq!(
        result_payload["result"]["exitCode"],
        serde_json::Value::Null
    );

    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1);
    let MessageContent::CommandResult { cancelled, .. } = &history[0].content else {
        panic!(
            "erwartete MessageContent::CommandResult, bekam {:?}",
            history[0].content
        );
    };
    assert!(
        cancelled,
        "der Kontext-Eintrag für die KI muss den Abbruch tragen"
    );
}

/// Ohne Abbruch darf in der Registry kein Eintrag zurückbleiben — sonst
/// würde jedes regulär beendete Kommando die Registry unbegrenzt
/// wachsen lassen.
#[tokio::test]
async fn test_execute_suggested_command_without_cancellation_leaves_no_registry_entry() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    let emitter = TestEmitter::default();
    let action_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();

    let executed = execute_suggested_command(
        &session,
        session_id,
        action_id,
        "ls -la".to_string(),
        &emitter,
        true,
        LedgerSource::Ai,
    )
    .await;
    assert!(executed);

    let result = session
        .running_command_cancellations
        .resolve(&action_id, ());
    assert!(
        result.is_err(),
        "nach regulärer Beendigung darf kein Registry-Eintrag mehr existieren, sonst ein Leck pro Kommando"
    );
}

/// `cancel_running_command` (Tauri-Command) delegiert nur an
/// `resolve()` und ignoriert dessen Fehler — hier direkt gegen die
/// Registry geprüft (kein `AppState` nötig, um denselben Effekt zu
/// testen): ein Abbruchversuch für eine unbekannte/bereits beendete
/// `action_id` darf nicht fehlschlagen/abstürzen.
#[test]
fn test_cancel_unknown_action_id_is_silently_ignored() {
    let registry: ConfirmationRegistry<ActionId, ()> = ConfirmationRegistry::new();
    let result = registry.resolve(&Uuid::new_v4(), ());
    assert!(result.is_err());
}

// --- Spec 0026: Risiko-Indikatoren --------------------------------------

#[test]
fn test_escalate_data_risk_none_to_yellow_via_ai() {
    let (level, reason) = escalate_data_risk(
        RiskLevel::None,
        None,
        Some((
            RiskLevel::Yellow,
            "könnte interne Hostnamen enthalten".to_string(),
        )),
    );
    assert_eq!(level, RiskLevel::Yellow);
    assert_eq!(
        reason.as_deref(),
        Some("könnte interne Hostnamen enthalten")
    );
}

#[test]
fn test_escalate_data_risk_yellow_to_red_via_ai() {
    let (level, reason) = escalate_data_risk(
        RiskLevel::Yellow,
        Some("listet .ssh auf".to_string()),
        Some((
            RiskLevel::Red,
            "enthält vermutlich einen privaten Schlüssel".to_string(),
        )),
    );
    assert_eq!(level, RiskLevel::Red);
    assert_eq!(
        reason.as_deref(),
        Some("enthält vermutlich einen privaten Schlüssel")
    );
}

/// Spec 0026, Abschnitt 3: "Nur Eskalation, nie Abschwächung" — der
/// zentrale, explizit verlangte Test: ein regelbasiertes `Red` darf
/// durch KEIN KI-Ergebnis mehr abgeschwächt werden, auch nicht durch
/// ein KI-Ergebnis von `none`.
#[test]
fn test_escalate_data_risk_rule_based_red_survives_ai_none() {
    let (level, reason) = escalate_data_risk(
        RiskLevel::Red,
        Some("Zugriff auf eine SSH-Private-Key-Datei (id_rsa)".to_string()),
        Some((RiskLevel::None, "looks harmless to me".to_string())),
    );
    assert_eq!(level, RiskLevel::Red);
    assert_eq!(
        reason.as_deref(),
        Some("Zugriff auf eine SSH-Private-Key-Datei (id_rsa)"),
        "die ursprüngliche regelbasierte Begründung darf nicht durch die KI-Begründung ersetzt werden"
    );
}

#[test]
fn test_escalate_data_risk_rule_based_red_survives_ai_yellow() {
    let (level, _) = escalate_data_risk(
        RiskLevel::Red,
        Some("...".to_string()),
        Some((RiskLevel::Yellow, "...".to_string())),
    );
    assert_eq!(level, RiskLevel::Red);
}

#[test]
fn test_escalate_data_risk_no_second_opinion_keeps_rule_based_result() {
    let (level, reason) = escalate_data_risk(RiskLevel::Yellow, Some("x".to_string()), None);
    assert_eq!(level, RiskLevel::Yellow);
    assert_eq!(reason.as_deref(), Some("x"));
}

fn risk_assessment_updated_payload(
    events: &[(String, serde_json::Value)],
) -> Option<&serde_json::Value> {
    events
        .iter()
        .find(|(name, _)| name == "risk-assessment-updated")
        .map(|(_, payload)| payload)
}

/// End-to-end (nicht nur die reine Funktion): eine aktivierte
/// Zweitmeinung hebt ein regelbasiertes `None` auf `Yellow` an und das
/// Ergebnis kommt tatsächlich als `risk-assessment-updated`-Event beim
/// `TestEmitter` an.
#[tokio::test]
async fn test_second_opinion_escalates_none_to_yellow_end_to_end() {
    let mut session = session_with_second_opinion(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                // Unauffällig laut Regel-Klassifizierer (kein Muster
                // trifft) — die KI-Zweitmeinung ist hier die einzige
                // Quelle für ein Risiko ungleich `None`.
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
        MockAiProvider::new(vec![
            AiEvent::TextDelta("yellow: könnte interne Pfade offenlegen".to_string()),
            AiEvent::Done,
        ]),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let payload = risk_assessment_updated_payload(&events)
        .expect("erwartet: risk-assessment-updated wurde gesendet");
    assert_eq!(payload["dataRisk"], serde_json::json!("yellow"));
    assert_eq!(
        payload["reason"],
        serde_json::json!("könnte interne Pfade offenlegen")
    );
}

/// Spec 0026, Abschnitt 3: das zentrale Sicherheitsversprechen auch
/// end-to-end geprüft — ein bereits regelbasiert als `Red`
/// eingestuftes Kommando bleibt `Red`, selbst wenn die aktivierte
/// KI-Zweitmeinung `none` zurückmeldet.
#[tokio::test]
async fn test_second_opinion_cannot_downgrade_rule_based_red_end_to_end() {
    let mut session = session_with_second_opinion(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("cat ~/.ssh/id_rsa", output("")),
        MockAiProvider::new(vec![
            AiEvent::TextDelta("none, this looks like a routine read".to_string()),
            AiEvent::Done,
        ]),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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
            resolve_first_confirm(&emitter, &confirmations, ActionUserDecision::Deny),
        )
    })
    .await
    .expect("Turn muss nach der Bestätigung enden");

    let events = emitter.events.lock().unwrap().clone();
    let payload = risk_assessment_updated_payload(&events)
        .expect("erwartet: risk-assessment-updated wurde gesendet");
    assert_eq!(
        payload["dataRisk"],
        serde_json::json!("red"),
        "ein regelbasiertes Red darf durch keine KI-Zweitmeinung abgeschwächt werden"
    );
}

/// Deaktivierte Zweitmeinung (Default: `risk_second_opinion_provider:
/// None`, s. `test_session`) darf keinen zusätzlichen API-Call auslösen
/// — strukturell garantiert, da `handle_action_proposed` den
/// Zweitmeinungs-Zweig nur betritt, wenn `Session::
/// risk_second_opinion_provider` `Some` ist. Dieser Test macht die
/// beobachtbare Konsequenz explizit: kein `risk-assessment-updated`-
/// Event, also auch kein Lade-Indikator, der je aufgelöst werden müsste.
#[tokio::test]
async fn test_disabled_second_opinion_yields_no_update_event() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("cat ~/.ssh/id_rsa", output("")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    assert!(session.risk_second_opinion_provider.is_none());
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
            resolve_first_confirm(&emitter, &confirmations, ActionUserDecision::Deny),
        )
    })
    .await
    .expect("Turn muss nach der Bestätigung enden");

    let events = emitter.events.lock().unwrap().clone();
    assert!(
        risk_assessment_updated_payload(&events).is_none(),
        "bei deaktivierter Zweitmeinung darf kein risk-assessment-updated-Event gesendet werden"
    );
    // Die regelbasierte Ersteinschätzung (Red, da id_rsa) bleibt davon
    // unberührt im `chat-action-proposed`-Event sichtbar.
    let (_, proposed_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-proposed")
        .expect("erwartet: chat-action-proposed wurde gesendet");
    assert_eq!(
        proposed_payload["riskAssessment"]["dataRisk"],
        serde_json::json!("red")
    );
}

// --- Spec 0018: Sudo-Passwort -----------------------------------------

#[test]
fn test_detect_elevation_prefix_matches_leading_sudo_and_doas() {
    assert_eq!(detect_elevation_prefix("sudo apt update"), Some("sudo"));
    assert_eq!(detect_elevation_prefix("  sudo apt update"), Some("sudo"));
    assert_eq!(detect_elevation_prefix("doas apt update"), Some("doas"));
    assert_eq!(detect_elevation_prefix("sudo"), Some("sudo"));
}

#[test]
fn test_detect_elevation_prefix_rejects_partial_word_match() {
    // "sudoku" darf nicht als "sudo"-Präfix erkannt werden.
    assert_eq!(detect_elevation_prefix("sudoku --help"), None);
}

#[test]
fn test_detect_elevation_prefix_ignores_sudo_mid_chain() {
    // Spec 0018, Abschnitt 3: bewusst keine Erkennung mitten in einer
    // Kommandokette.
    assert_eq!(
        detect_elevation_prefix("cd /var/log && sudo tail -f x"),
        None
    );
}

#[test]
fn test_command_with_stdin_password_flag_inserts_dash_s() {
    assert_eq!(
        command_with_stdin_password_flag("sudo systemctl restart nginx"),
        Some("sudo -S systemctl restart nginx".to_string())
    );
}

#[test]
fn test_command_with_stdin_password_flag_none_for_non_elevated_command() {
    assert_eq!(command_with_stdin_password_flag("ls -la"), None);
}

#[test]
fn test_command_with_stdin_password_flag_leaves_existing_dash_s_untouched() {
    // KI/Nutzer hat die Passworteingabe bereits selbst vorgesehen —
    // nicht gegensteuern (s. Doc-Kommentar).
    assert_eq!(command_with_stdin_password_flag("sudo -S apt update"), None);
}

/// Kern von Spec 0018, Abschnitt 5: ein hinterlegtes Sudo-Passwort wird
/// über `execute_with_stdin` eingespeist, das Kommando dabei um `-S`
/// ergänzt — sowohl im tatsächlichen Transport-Aufruf als auch im
/// `chat-action-result`/Kontext-Eintrag (volle Transparenz, s.
/// Spec-Dokument Abschnitt 5, letzter Absatz).
#[tokio::test]
async fn test_sudo_command_with_stored_password_uses_stdin_and_rewritten_command() {
    let transport = MockSshTransport::default()
        .with_response("sudo -S systemctl restart nginx", output("done"));
    // Handle vor dem Verschieben von `transport` in die Session ziehen
    // (analog zu `MockAiProvider::received_contexts_handle`).
    let stdin_calls = transport.stdin_calls_handle();

    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "sudo systemctl restart nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        transport,
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.sudo_password = Some(secrecy::SecretString::from("hunter2".to_string()));

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    // Unabhängiger Review-Pass (Spec 0018): ein hinterlegtes
    // Sudo-Passwort erzwingt jetzt immer Confirm — ohne diese
    // Genehmigung würde `run_chat_turn` ewig auf die nie eintreffende
    // Bestätigung warten.
    let turn = run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"]
    );
    let (_, result_payload) = &events[1];
    assert_eq!(
        result_payload["result"]["command"], "sudo -S systemctl restart nginx",
        "das tatsächlich ausgeführte Kommando (mit -S) muss im Ergebnis-Event stehen"
    );

    let history = session.context.lock().await.history.clone();
    assert!(matches!(
        &history[0].content,
        MessageContent::CommandResult { command, .. } if command == "sudo -S systemctl restart nginx"
    ));

    let calls = stdin_calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        1,
        "execute_with_stdin muss genau einmal aufgerufen werden"
    );
    assert_eq!(calls[0].0, "sudo -S systemctl restart nginx");
    assert_eq!(
        calls[0].1,
        b"hunter2\n".to_vec(),
        "das Passwort muss gefolgt von einem Zeilenumbruch als Stdin ankommen"
    );
}

#[tokio::test]
async fn test_sudo_command_without_stored_password_runs_unchanged_via_plain_execute() {
    // Kein `session.sudo_password` gesetzt (Default) — Regression: das
    // bekannte, unveränderte Fehlverhalten von `sudo` ohne TTY bleibt
    // erhalten (kein automatisches Umschreiben ohne hinterlegtes
    // Passwort), s. Spec 0018, Abschnitt 5, Punkt 2.
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "sudo systemctl restart nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("sudo systemctl restart nginx", output("done")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let (_, result_payload) = &events[1];
    assert_eq!(
        result_payload["result"]["command"],
        "sudo systemctl restart nginx"
    );
}

#[tokio::test]
async fn test_chat_action_proposed_flags_uses_stored_sudo_password() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "sudo apt update".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("sudo -S apt update", output("")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.sudo_password = Some(secrecy::SecretString::from("hunter2".to_string()));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    // Unabhängiger Review-Pass (Spec 0018): die Verwendung eines
    // hinterlegten Sudo-Passworts erzwingt jetzt immer Confirm (s.
    // `test_uses_stored_sudo_password_downgrades_autoexec_to_confirm`
    // unten) — ohne diese Genehmigung würde `run_chat_turn` ewig auf
    // die nie eintreffende Bestätigung warten.
    let turn = run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = &events[0];
    assert_eq!(proposed_payload["usesStoredSudoPassword"], true);
}

/// **Der eigentliche Fix des unabhängigen Review-Passes** (Spec 0018):
/// ein hinterlegtes Sudo-Passwort darf nie ohne Bestätigung verbraucht
/// werden — auch nicht, wenn eine (typischerweise für den
/// unprivilegierten Fall angelegte) Allow-Regel die `sudo`-Variante per
/// Dual-Text-Matching (ADR 0002) zufällig mit abdeckt.
#[tokio::test]
async fn test_uses_stored_sudo_password_downgrades_autoexec_to_confirm() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.sudo_password = Some(secrecy::SecretString::from("hunter2".to_string()));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let action_future = handle_action_proposed(
        &session,
        session_id,
        AiAction::SuggestCommand {
            command: "sudo ls -la".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    let responder = deny_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-proposed")
        .expect("chat-action-proposed muss gesendet worden sein");
    assert_eq!(
        proposed_payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM"),
        "ein hinterlegtes Sudo-Passwort darf nie ohne Bestätigung verbraucht werden — war: {proposed_payload}"
    );
}

/// Spec 0039, Abschnitt 7: `Strict` — nach einer gelesenen Ausgabe wird
/// auch eine reine Leseaktion, die per Allow-Regel `AutoExec` wäre, zu
/// `Confirm` eskaliert.
#[tokio::test]
async fn test_post_ingest_policy_strict_escalates_even_a_pure_read_action() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Strict;
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::Confirm { .. }),
        "Strict muss auch eine reine Leseaktion eskalieren, war: {payload}"
    );
    assert_eq!(
        payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_POST_INGEST_REQUIRES_CONFIRM")
    );
}

/// Spec 0039, Abschnitt 7: `Balanced` — nach einer gelesenen Ausgabe
/// bleibt eine reine Leseaktion `AutoExec`.
#[tokio::test]
async fn test_post_ingest_policy_balanced_leaves_pure_read_action_autoexec() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Balanced;
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::AutoExec),
        "Balanced darf eine reine Leseaktion (Server-Risiko None) nicht eskalieren, war: {payload}"
    );
}

/// Spec 0039, Abschnitt 7: `Balanced` — eine verändernde Aktion
/// (Server-Risiko ≠ `None`) wird zu `Confirm` eskaliert.
#[tokio::test]
async fn test_post_ingest_policy_balanced_escalates_modifying_action() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Balanced;
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            // Server-Risiko Rot laut `server_risk_patterns()` (`*rm*-rf*`).
            // Server-Risiko Gelb laut `server_risk_patterns()`
            // (`systemctl*restart*`) — bewusst NICHT hart geblacklistet
            // (anders als z. B. `rm -rf`), damit dieser Test wirklich
            // die neue PostIngestPolicy-Eskalation prüft und nicht
            // zufällig durch `FILTER_HARD_BLACKLIST` verdeckt wird.
            command: "systemctl restart nginx".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::Confirm { .. }),
        "Balanced muss eine verändernde Aktion (Server-Risiko ≠ None) eskalieren, war: {payload}"
    );
    assert_eq!(
        payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_POST_INGEST_REQUIRES_CONFIRM")
    );
}

/// Spec 0039, Abschnitt 7: `Standard` — keine zusätzliche Eskalation
/// nach dem Einlesen, auch nicht für eine verändernde Aktion; Regeln
/// greifen unverändert.
#[tokio::test]
async fn test_post_ingest_policy_standard_does_not_escalate() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Standard;
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            // Server-Risiko Gelb laut `server_risk_patterns()`
            // (`systemctl*restart*`) — bewusst NICHT hart geblacklistet
            // (anders als z. B. `rm -rf`), damit dieser Test wirklich
            // die neue PostIngestPolicy-Eskalation prüft und nicht
            // zufällig durch `FILTER_HARD_BLACKLIST` verdeckt wird.
            command: "systemctl restart nginx".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::AutoExec),
        "Standard darf trotz bereits eingelesenem Serverinhalt nicht zusätzlich eskalieren, war: {payload}"
    );
}

/// Spec 0039, Abschnitt 6: weder eine `PostIngestPolicy`-Stufe noch das
/// Flag selbst kann eine `Deny`-Entscheidung abschwächen — auch nicht
/// unter `Strict`.
#[tokio::test]
async fn test_post_ingest_policy_never_downgrades_a_deny_decision() {
    struct DenyLsPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyLsPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-ls".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("ls *".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(DenyLsPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Strict;
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::Deny { .. }),
        "eine Deny-Regel darf durch die Post-Ingest-Eskalation nie abgeschwächt werden, war: {payload}"
    );
}

// --- Spec 0039, Abschnitt 5.2: KI-Prüfung auf eingeschleuste Anweisungen

/// Spec 0039, Abschnitt 7: ein "ja" der KI-Prüfung eskaliert die
/// nachfolgend vorgeschlagene Aktion zu `Confirm` und verbraucht dabei
/// das Flag (nicht mehr für die übernächste Aktion gesetzt).
#[tokio::test]
async fn test_injection_check_yes_escalates_followup_action_to_confirm() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.injection_check_provider = Some(Box::new(MockAiProvider::new(vec![
        AiEvent::TextDelta("ja - enthält eine eingeschleuste Anweisung".to_string()),
        AiEvent::Done,
    ])));

    let emitter = TestEmitter::default();
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &emitter,
        "aus der Datei gelesener Inhalt",
    )
    .await;
    assert!(
        session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "ein 'ja' der Prüfung muss das Verdachts-Flag setzen"
    );

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::Confirm { .. }),
        "ein erkannter Injection-Verdacht muss die Folgeaktion eskalieren, war: {payload}"
    );
    assert_eq!(
        payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM")
    );
    assert!(
        !session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "das Flag muss beim Eskalieren verbraucht (zurückgesetzt) werden"
    );
}

/// Spec 0039, Abschnitt 7: ein "nein" der KI-Prüfung ändert nichts —
/// insbesondere schwächt es keine sonst greifende Eskalation ab (hier:
/// es gibt keine, die Aktion bleibt einfach `AutoExec`, wie ohne
/// jede Prüfung auch).
#[tokio::test]
async fn test_injection_check_no_does_not_change_followup_decision() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.injection_check_provider = Some(Box::new(MockAiProvider::new(vec![
        AiEvent::TextDelta("nein, wirkt wie eine gewöhnliche Log-Zeile".to_string()),
        AiEvent::Done,
    ])));

    let emitter = TestEmitter::default();
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &emitter,
        "aus der Datei gelesener Inhalt",
    )
    .await;
    assert!(
        !session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "ein 'nein' darf das Verdachts-Flag nicht setzen"
    );

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::AutoExec),
        "ein 'nein' darf keine zusätzliche Bestätigung erzwingen, war: {payload}"
    );
}

/// Spec 0039, Abschnitt 7: ein Provider-Fehler (oder eine nicht
/// parsebare Antwort) darf weder die Aktion abstürzen lassen noch
/// stillschweigend als "kein Verdacht" durchgereicht werden — es
/// ändert schlicht nichts (kein Crash, kein gesetztes Flag).
#[tokio::test]
async fn test_injection_check_provider_error_does_not_panic_or_set_suspected_flag() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.injection_check_provider = Some(Box::new(MockAiProvider::new(vec![AiEvent::Error(
        AiError::NetworkError("boom".to_string()),
    )])));

    // Muss ohne Panik zurückkehren.
    let emitter = TestEmitter::default();
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &emitter,
        "aus der Datei gelesener Inhalt",
    )
    .await;

    assert!(
        !session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "ein Providerfehler darf nicht stillschweigend als 'kein Verdacht' gewertet werden \
         (das Flag darf dadurch aber auch nicht gesetzt werden — es ändert schlicht nichts)"
    );
}

/// Regression für den spec-reviewer-Fund (Spec 0061 Follow-up): das
/// Rate-Limit-Gate vor der Einschleusungs-Prüfung muss auf dem
/// tatsächlich gesendeten (ggf. via `truncate_for_second_opinion`
/// gekürzten) Inhalt schätzen, nicht auf dem rohen `content` — sonst
/// löst ein großer Datei-Lese-Inhalt beinahe immer unnötiges Warten
/// aus, obwohl der tatsächliche Request klein bleibt. Restbudget so
/// gewählt, dass die Schätzung auf dem gekürzten Inhalt (16 KB / 4
/// Bytes-pro-Token ≈ 4096 Tokens) klar darunterbleibt, die Schätzung
/// auf dem 1 MB großen rohen Inhalt (≈ 262144 Tokens) sie aber massiv
/// überschreiten würde.
#[tokio::test(start_paused = true)]
async fn test_injection_check_gate_estimates_on_truncated_content_not_raw_content() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.injection_check_provider = Some(Box::new(MockAiProvider::new(vec![
        AiEvent::TextDelta("nein".to_string()),
        AiEvent::Done,
    ])));
    let budget = Arc::new(ai_providers::ProviderBudgetGuard::new());
    budget.record_headers(ai_providers::RateLimitHeaderSnapshot {
        input_tokens: ai_providers::RawCounter {
            limit: Some(100_000),
            remaining: Some(50_000), // 50%, über der 15%-Schwelle
            reset_at: Some(std::time::Instant::now() + std::time::Duration::from_secs(30)),
        },
        ..Default::default()
    });
    session.injection_check_budget = Some(budget);

    // Deutlich über dem 16-KB-Kürzungslimit (~4096 geschätzte Tokens),
    // aber deutlich unter den 50.000 verbleibenden Tokens des Budgets —
    // die geschätzten ~262144 Tokens des ROHEN Inhalts würden das
    // Budget dagegen weit überschreiten. Großzügiger Abstand zu beiden
    // Seiten, damit der Test nicht bei kleinen Änderungen an
    // BYTES_PER_TOKEN_ESTIMATE/DEFAULT_SECOND_OPINION_MAX_LEN kippt.
    let huge_content = "a".repeat(1_000_000);
    let emitter = TestEmitter::default();

    let before = tokio::time::Instant::now();
    check_for_injected_instructions(&session, Uuid::new_v4(), &emitter, &huge_content).await;
    let elapsed = before.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "muss auf dem gekürzten Inhalt schätzen und darf deshalb nicht warten, wartete {elapsed:?}"
    );
    assert!(
        emitter.events.lock().unwrap().is_empty(),
        "kein Warte-Event, wenn (korrekt) gar nicht gewartet wurde"
    );
}

/// Spec 0039, Abschnitt 6 (analog zur PostIngestPolicy-Eskalation):
/// ein erkannter Injection-Verdacht kann eine bereits per Regel
/// getroffene `Deny`-Entscheidung nie abschwächen.
#[tokio::test]
async fn test_injection_suspected_never_downgrades_a_deny_decision() {
    struct DenyLsPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyLsPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-ls".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("ls *".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(DenyLsPolicyStore));
    session
        .injection_suspected
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        },
    )
    .await;

    assert!(
        matches!(decision, Decision::Deny { .. }),
        "ein Injection-Verdacht darf eine Deny-Regel nie abschwächen, war: {payload}"
    );
}

/// Unabhängiger Review-Pass (Spec 0039): eine Aktion, die ohnehin schon
/// (unabhängig vom Verdacht) `Deny` bekommt, darf das Verdachts-Flag
/// nicht "verbrauchen" — sonst könnte der eingeschleuste Inhalt selbst
/// gezielt zuerst ein hart geblacklistetes Kommando vorschlagen lassen
/// (das ohnehin scheitert), um damit das Flag zu verbrennen, bevor die
/// eigentlich gemeinte Folgeaktion vorgeschlagen wird — die liefe dann
/// trotz erkanntem Verdacht ungebremst als `AutoExec`. Regressionstest
/// für genau diesen Umgehungspfad.
#[tokio::test]
async fn test_injection_suspected_survives_an_unrelated_denied_action() {
    struct DenyLsPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyLsPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-ls".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("ls *".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(DenyLsPolicyStore));
    session
        .injection_suspected
        .store(true, std::sync::atomic::Ordering::SeqCst);

    // Erste (vom Payload vorgeschobene) Aktion: per Regel `Deny`,
    // unabhängig vom Verdachts-Flag.
    let (first_decision, _) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        },
    )
    .await;
    assert!(matches!(first_decision, Decision::Deny { .. }));
    assert!(
        session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "eine ohnehin `Deny`-Aktion darf das Verdachts-Flag nicht verbrauchen"
    );

    // Zweite, eigentlich gemeinte Aktion: wäre ohne den Verdacht
    // `AutoExec` (andere Policy: alles erlaubt) — muss trotzdem
    // eskaliert werden, und zwar sichtbar über den
    // Injection-spezifischen Code, nicht zufällig über
    // `FILTER_NO_RULE_MATCHED` o. ä.
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    let (second_decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;
    assert!(
        matches!(second_decision, Decision::Confirm { .. }),
        "das Flag muss für die tatsächlich vorgeschlagene Folgeaktion erhalten bleiben, war: {payload}"
    );
    assert_eq!(
        payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM")
    );
}

/// Spec 0039, Abschnitt 5: das Flag ist innerhalb einer Sitzung monoton
/// — einmal durch eine Kommando-Ausführung gesetzt, bleibt es auch für
/// eine völlig neue, danach vorgeschlagene Aktion gesetzt (simuliert
/// "nächste Nutzer-Nachricht", ohne dass irgendein Rundenzähler
/// zurückgesetzt wird).
#[tokio::test]
async fn test_untrusted_content_ingested_flag_is_monotonic_across_actions() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response("uptime", output("up 3 days")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Strict;
    assert!(!session
        .untrusted_content_ingested
        .load(std::sync::atomic::Ordering::SeqCst));

    // Erste Aktion: AutoExec (Flag noch nicht gesetzt), Ausführung
    // setzt das Flag als Nebeneffekt (execute_suggested_command).
    let (first_decision, _) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "uptime".to_string(),
        },
    )
    .await;
    assert!(matches!(first_decision, Decision::AutoExec));
    assert!(
        session
            .untrusted_content_ingested
            .load(std::sync::atomic::Ordering::SeqCst),
        "Ausführung eines Kommandos muss das Flag setzen"
    );

    // Zweite, unabhängige Aktion auf derselben Session: Flag ist immer
    // noch gesetzt, Strict eskaliert entsprechend.
    let (second_decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;
    assert!(
        matches!(second_decision, Decision::Confirm { .. }),
        "das Flag darf zwischen zwei Aktionen nicht verloren gehen, war: {payload}"
    );
}

#[tokio::test]
async fn test_chat_action_proposed_does_not_flag_sudo_usage_without_stored_password() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "sudo apt update".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("sudo apt update", output("")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = &events[0];
    assert_eq!(proposed_payload["usesStoredSudoPassword"], false);
}

#[tokio::test]
async fn test_confirm_path_waits_for_respond_to_action_before_executing() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                // Kein Mock-Response konfiguriert für den *originalen*
                // Befehl — nur für den editierten (s. unten). Würde die
                // Ausführung fälschlich vor der Bestätigung starten,
                // schlägt der Test mit einem `ChannelError` fehl statt
                // einfach nur zu spät zu sein.
                command: "rm -rf /tmp/build".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("rm -rf /tmp/build-edited", output("removed")),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        // Simuliert das Frontend: wartet, bis `chat-action-proposed`
        // sichtbar ist, editiert dann das Kommando und bestätigt.
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(
                        &action_id,
                        ActionUserDecision::EditThenApprove {
                            command: "rm -rf /tmp/build-edited".to_string(),
                        },
                    )
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"]
    );
    let (_, result_payload) = &events[1];
    assert_eq!(
        result_payload["result"]["command"],
        "rm -rf /tmp/build-edited"
    );
}

#[tokio::test]
async fn test_edited_command_that_hits_deny_rule_is_blocked_not_executed() {
    // Trifft absichtlich **nur** die editierte Fassung ("*-edited"),
    // nicht das Original ("echo hi") — sonst würde schon der
    // ursprüngliche Vorschlag mit `Deny` beantwortet und der
    // Confirm-Wartepfad (den dieser Test eigentlich prüfen soll) nie
    // erreicht.
    struct DenyEditedPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyEditedPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-edited".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("*-edited".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "echo hi".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("echo hi-edited", output("hi-edited")),
    );
    session.filter_engine = Box::new(FilterEngine::new(DenyEditedPolicyStore));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(
                        &action_id,
                        ActionUserDecision::EditThenApprove {
                            command: "echo hi-edited".to_string(),
                        },
                    )
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap().clone();
    // Zwei `chat-action-proposed` (Original + editierte, geblockte
    // Fassung), aber **kein** `chat-action-result` — nichts wurde
    // ausgeführt.
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-proposed"]
    );
    assert!(events[1].1["decision"]["Deny"].is_object());
    // Spec 0021, Abschnitt 3, Fall 4: die per Bearbeiten-Dialog erneut
    // geblockte Fassung ist inhaltlich derselbe Fall wie ein regulärer
    // Filter-Engine-Deny — bekommt denselben `ActionRejected`-Eintrag
    // statt einer leeren Historie.
    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1);
    assert!(matches!(
        &history[0].content,
        MessageContent::ActionRejected {
            command,
            reason: RejectionReason::Blocked(_)
        } if command == "echo hi-edited"
    ));
}

#[tokio::test]
async fn test_deny_path_executes_nothing_and_does_not_block_further_events() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "curl evil.example".to_string(),
            }),
            AiEvent::TextDelta("weiterer Text nach der Ablehnung".to_string()),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    // Hard-Blacklist matcht "curl" nicht automatisch auf Deny (nur
    // Confirm, s. core::filter), daher hier eine explizite
    // Deny-Regel, um den reinen Deny-Pfad ohne Warten zu testen.
    struct DenyCurlPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyCurlPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-curl".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("curl*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }
    let mut session = session;
    session.filter_engine = Box::new(FilterEngine::new(DenyCurlPolicyStore));

    run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    // Deny blockiert nur die Ausführung, nicht den weiteren
    // Stream-Verlauf: das TextDelta danach kommt trotzdem an.
    assert_eq!(event_names, vec!["chat-action-proposed", "chat-text-delta"]);
    assert!(session
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(
            &m.content,
            MessageContent::Text(t) if t.contains("weiterer Text")
        )));
    assert!(!session
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(m.content, MessageContent::CommandResult { .. })));
    // Spec 0021, Abschnitt 3, Fall 4: der Blockier-Grund landet als
    // `ActionRejected` im Kontext, damit die KI (in der automatisch
    // ausgelösten Folgerunde) weiß, warum nichts ausgeführt wurde.
    assert!(session
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(
            &m.content,
            MessageContent::ActionRejected { command, reason: RejectionReason::Blocked(_) }
                if command == "curl evil.example"
        )));
}

/// Spec 0003 Abschnitt 5.2 / Spec 0007 Abschnitt 6, letzter Punkt:
/// `ProposeNoteUpdate` wartet **immer** auf Bestätigung, unabhängig von
/// der Filter-Engine — hier absichtlich mit `AllowEverythingPolicyStore`
/// (die für ein `SuggestCommand` sofort `AutoExec` ergäbe), um zu
/// zeigen, dass die Filter-Engine für diesen Aktionstyp gar nicht erst
/// gefragt wird.
#[tokio::test]
async fn test_propose_note_update_always_waits_for_confirmation_and_persists() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "neuer Kontext".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    // Spec 0016, Abschnitt 6: die KI liefert keine ID mehr — das Backend
    // löst `CurrentServer` selbst auf `session.server_id` auf.
    let expected_server_id = session.server_id;
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"]
    );
    let (_, proposed_payload) = &events[0];
    assert!(
        proposed_payload["decision"]["Confirm"].is_object(),
        "ProposeNoteUpdate muss immer Confirm sein, nie AutoExec"
    );

    let revisions = profile_store.note_revisions.lock().unwrap().clone();
    assert_eq!(revisions.len(), 1);
    assert_eq!(
        revisions[0].target,
        ssh_manager_core::profiles::NoteTarget::Server(expected_server_id),
        "CurrentServer muss auf session.server_id auflösen, nie auf eine von der KI \
         gelieferte ID (die KI kennt hier gar keine)"
    );
    assert!(session
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("aktualisiert"))));
}

/// Kern des ADR-Vorschlags in diesem Modul-Doc: nach einem
/// tatsächlich ausgeführten Kommando bekommt die KI automatisch eine
/// Folgerunde, um dessen Ergebnis in eine Antwort zu fassen — vorher
/// endete `run_chat_turn` stattdessen wortlos nach dem
/// `chat-action-result`.
#[tokio::test]
async fn test_executed_action_triggers_automatic_followup_round_with_final_answer() {
    let mut session = session_with_ai_provider(
        MockAiProvider::with_rounds(vec![
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "uptime".to_string(),
                }),
                AiEvent::Done,
            ],
            vec![
                AiEvent::TextDelta("Der Server läuft seit 3 Tagen.".to_string()),
                AiEvent::Done,
            ],
        ]),
        MockSshTransport::default().with_response("uptime", output("up 3 days")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec![
            "chat-action-proposed",
            "chat-action-result",
            "chat-text-delta"
        ]
    );

    let history = session.context.lock().await.history.clone();
    assert!(history
        .iter()
        .any(|m| matches!(&m.content, MessageContent::CommandResult { .. })));
    assert!(history
        .iter()
        .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("3 Tagen"))));
}

/// Aufgabenstellung Teil 1, Punkt 2/5 (Spec 0017, Abschnitt 2, letzter
/// Absatz): eine langsame KI-Antwort in einer Session darf einen
/// zeitnahen Befehl in einer anderen Session nicht ausbremsen. Session A
/// bekommt einen `AiProvider`, dessen `send()`-Stream erst nach 300ms
/// überhaupt das erste Element liefert (simuliert einen langsamen/
/// hängenden KI-Stream) — währenddessen muss `run_chat_turn` für Session
/// B (über denselben `SessionManager`, wie es zwei parallele
/// `send_chat_message`-Aufrufe für zwei offene Tabs täten) deutlich unter
/// dieser Zeit fertig werden. Schlägt fehl, falls `SessionManager` doch
/// einen Lock über die gesamte Map hinweg über einen Await-Punkt hält
/// (die Regression, vor der Spec 0017 warnt) oder falls `Session`s
/// `context`/`transport`-Mutexe session-übergreifend geteilt würden statt
/// pro Session zu existieren.
struct SlowAiProvider {
    delay: std::time::Duration,
}

impl AiProvider for SlowAiProvider {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let delay = self.delay;
        Box::pin(futures::stream::once(async move {
            tokio::time::sleep(delay).await;
            AiEvent::Done
        }))
    }
}

#[tokio::test]
async fn test_slow_session_does_not_block_concurrent_session_via_shared_manager() {
    let manager = SessionManager::new();
    let id_slow = Uuid::new_v4();
    let id_fast = Uuid::new_v4();

    manager.insert(
        id_slow,
        Arc::new(session_with_ai_provider(
            SlowAiProvider {
                delay: std::time::Duration::from_millis(300),
            },
            MockSshTransport::default(),
        )),
    );
    manager.insert(
        id_fast,
        Arc::new(test_session(
            vec![AiEvent::Done],
            MockSshTransport::default(),
        )),
    );

    let session_slow = manager.get(id_slow).unwrap();
    let session_fast = manager.get(id_fast).unwrap();
    let emitter_slow = TestEmitter::default();
    let emitter_fast = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let slow_turn = run_chat_turn(
        &session_slow,
        id_slow,
        &emitter_slow,
        &profile_store,
        &confirmations,
    );

    // `SessionManager::get` für Session B während Session A noch mitten
    // in ihrem (langsamen) Turn steckt — genau das, was ein zweiter,
    // gleichzeitiger `send_chat_message`-Aufruf für einen anderen Tab
    // täte.
    let fast_turn = async {
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            run_chat_turn(
                &session_fast,
                id_fast,
                &emitter_fast,
                &profile_store,
                &confirmations,
            ),
        )
        .await
        .expect(
            "Session B wurde durch die langsame Session A blockiert — \
             SessionManager/Session-Locks sperren offenbar über Sessions hinweg",
        )
    };

    tokio::join!(slow_turn, fast_turn);

    // Spec 0080, A2: Session B bekommt Runde 1 eines frischen Turns nur
    // mit `Done` — seit A2 (Klarstellung Q-BL-0259-01) also GENAU das
    // `chat-response-empty`-Event, nicht mehr gar keins. Diese Zeile
    // prüft nicht den Kern dieses Tests (Nebenläufigkeit, s. oben) —
    // sie hält nur fest, dass sich außer dieser einen, erwarteten
    // Ergänzung nichts an Session B geändert hat.
    let events_fast = emitter_fast.events.lock().unwrap().clone();
    let event_names: Vec<&str> = events_fast.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(event_names, vec!["chat-response-empty"]);
}
