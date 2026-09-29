//! Spec 0092, A3: Hebt die KI-Zweitmeinung das Daten-Risiko **nachträglich**
//! auf Rot, wird die schon als `AutoExec` angekündigte Aktion nicht
//! ausgeführt, sondern zu einer Bestätigung — gemeldet über
//! `action-decision-escalated`.
//!
//! Alle Fälle hier laufen mit einer Allow-Regel und einem Kommando, das der
//! regelbasierte Klassifizierer **nicht** rot einstuft (sonst hätte das Glied
//! aus A2 längst eskaliert und A3 wäre unbeobachtbar).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ssh_manager_core::ai::{AiEvent, AiProvider, SessionContext};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent};
use ssh_manager_core::filter::FilterEngine;

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

/// Regelbasiert nur **Gelb** (Server-Achse), Daten-Achse `None` — die
/// Zweitmeinung ist damit die einzige Quelle für ein Rot.
const YELLOW_ONLY: &str = "systemctl restart nginx";
/// Regelbasiert völlig unauffällig.
const PLAIN: &str = "ls -la";

fn escalated_event(events: &[(String, serde_json::Value)]) -> Option<&serde_json::Value> {
    events
        .iter()
        .find(|(name, _)| name == "action-decision-escalated")
        .map(|(_, payload)| payload)
}

fn event_index(events: &[(String, serde_json::Value)], name: &str) -> Option<usize> {
    events.iter().position(|(n, _)| n == name)
}

fn action_result_count(events: &[(String, serde_json::Value)]) -> usize {
    events
        .iter()
        .filter(|(name, _)| name == "chat-action-result")
        .count()
}

fn attach_second_opinion(session: &mut Session, provider: impl AiProvider + 'static) {
    session.parts_mut_for_tests().risk_second_opinion_provider = Some(Box::new(provider));
    session.parts_mut_for_tests().risk_second_opinion_budget =
        Some(Arc::new(ai_providers::ProviderBudgetGuard::new()));
}

/// Wartet, bis die Eskalation gemeldet ist, und beantwortet dann den Dialog
/// — anders als [`respond_to_first_proposed_action`], das auf
/// `chat-action-proposed` reagiert: Dessen Entscheidung ist hier noch
/// `AutoExec`, ein Klick zu diesem Zeitpunkt träfe noch keinen registrierten
/// Empfänger.
async fn resolve_first_escalated_action(
    emitter: &TestEmitter,
    confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    decision: ActionUserDecision,
) {
    loop {
        let action_id = escalated_event(&emitter.events.lock().unwrap().clone())
            .map(|payload| payload["actionId"].as_str().unwrap().to_string());
        if let Some(action_id) = action_id {
            let action_id: ActionId = action_id.parse().unwrap();
            confirmations
                .resolve(&action_id, decision)
                .expect("die Registrierung muss VOR dem Ereignis liegen (A3.2)");
            return;
        }
        tokio::task::yield_now().await;
    }
}

/// Eine Zweitmeinung, die genau ein Urteil liefert.
struct VerdictProvider(&'static str);
impl AiProvider for VerdictProvider {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![
            AiEvent::TextDelta(self.0.to_string()),
            AiEvent::Done,
        ]))
    }
}

// --- T10: Anhebung auf Rot, Bestätigen ---------------------------------

/// Spec 0092, T10: keine Ausführung vor dem Klick;
/// `action-decision-escalated` **nach** `risk-assessment-updated`;
/// Bestätigen führt **genau einmal** aus; Ledger-Eintrag `Confirmed` mit dem
/// neuen Code.
#[tokio::test]
async fn test_t10_second_opinion_raising_to_red_turns_autoexec_into_confirmation() {
    let (mut session, _chat_store, chat_session_id, _tmp_dir, ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done],
            MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")),
        )
        .await;
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(
        &mut session,
        VerdictProvider("red: gibt Zugangsdaten des Dienstes aus"),
    );

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: YELLOW_ONLY.to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    let responder = async {
        resolve_first_escalated_action(&emitter, &confirmations, ActionUserDecision::Approve).await;
        assert_eq!(
            action_result_count(&emitter.events.lock().unwrap().clone()),
            0,
            "vor dem Klick darf nichts gelaufen sein"
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(handled, responder)
    })
    .await
    .expect("Dialog muss enden");

    let events = emitter.events.lock().unwrap().clone();
    let escalated = escalated_event(&events).expect("action-decision-escalated muss kommen");
    assert_eq!(escalated["code"], "FILTER_RED_RISK_REQUIRES_CONFIRM");
    assert!(
        escalated["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("gibt Zugangsdaten des Dienstes aus"),
        "der Grund nennt die Begründung der Zweitmeinung: {escalated}"
    );
    assert_eq!(escalated["actionId"], events[0].1["actionId"]);

    // §5: Reihenfolge — das Badge ist rot, bevor der Dialog erscheint.
    let updated_at = event_index(&events, "risk-assessment-updated")
        .expect("risk-assessment-updated muss kommen");
    let escalated_at = event_index(&events, "action-decision-escalated").unwrap();
    assert!(
        updated_at < escalated_at,
        "action-decision-escalated muss NACH risk-assessment-updated kommen: {events:?}"
    );

    assert_eq!(
        action_result_count(&events),
        1,
        "genau eine Ausführung nach dem Bestätigen: {events:?}"
    );

    let entries = ledger_store.load_entries(chat_session_id).await.unwrap();
    let decision = entries
        .iter()
        .find_map(|entry| match &entry.content {
            LedgerEntryContent::Decision { outcome, code, .. } => Some((outcome, code)),
            _ => None,
        })
        .expect("ein Decision-Eintrag muss geschrieben werden");
    assert!(matches!(decision.0, LedgerDecisionOutcome::Confirmed));
    assert_eq!(
        decision.1.as_deref(),
        Some("FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "der Ledger-Eintrag trägt den Eskalationsgrund, wie bei jedem anderen Confirm"
    );
}

// --- T11: Anhebung auf Rot, Ablehnen ----------------------------------

/// Spec 0092, T11/A3.4: Ablehnen führt nichts aus, setzt
/// `earlier_rejection` und schreibt `Rejected` mit dem neuen Code.
#[tokio::test]
async fn test_t11_rejecting_the_escalated_action_blocks_it_and_marks_the_rejection() {
    let (mut session, _chat_store, chat_session_id, _tmp_dir, ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done],
            MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")),
        )
        .await;
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(&mut session, VerdictProvider("red: heikel"));

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let earlier_rejection = AtomicBool::new(false);

    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: YELLOW_ONLY.to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        &earlier_rejection,
    );
    let responder =
        resolve_first_escalated_action(&emitter, &confirmations, ActionUserDecision::Deny);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(handled, responder)
    })
    .await
    .expect("Dialog muss enden");

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(action_result_count(&events), 0, "{events:?}");
    assert!(
        earlier_rejection.load(Ordering::SeqCst),
        "eine Ablehnung hier zählt wie jede andere für die Nachbar-Aktionen"
    );

    let entries = ledger_store.load_entries(chat_session_id).await.unwrap();
    let decision = entries
        .iter()
        .find_map(|entry| match &entry.content {
            LedgerEntryContent::Decision { outcome, code, .. } => Some((outcome, code)),
            _ => None,
        })
        .expect("ein Decision-Eintrag muss geschrieben werden");
    assert!(matches!(decision.0, LedgerDecisionOutcome::Rejected));
    assert_eq!(
        decision.1.as_deref(),
        Some("FILTER_RED_RISK_REQUIRES_CONFIRM")
    );
}

// --- T12/T13: die Fälle, in denen nichts passieren darf ----------------

/// Spec 0092, T12/A3.5: Einstellung aus → Ausführung wie vor dieser Spec,
/// kein neues Ereignis.
#[tokio::test]
async fn test_t12_setting_off_lets_the_action_run_as_before() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().red_risk_always_confirm = false;
    attach_second_opinion(&mut session, VerdictProvider("red: heikel"));

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handle_action_proposed(
            &session,
            Uuid::new_v4(),
            AiAction::SuggestCommand {
                command: YELLOW_ONLY.to_string(),
            },
            &emitter,
            &profile_store,
            &confirmations,
            ActionOrigin::Internal,
            test_fresh_rejection_flag(),
        ),
    )
    .await
    .expect("ohne Eskalation läuft die Aktion ohne Dialog durch");

    let events = emitter.events.lock().unwrap().clone();
    assert!(escalated_event(&events).is_none(), "{events:?}");
    assert_eq!(action_result_count(&events), 1, "{events:?}");
    // Das Badge-Update kommt trotzdem — nur die Entscheidung bleibt.
    assert!(event_index(&events, "risk-assessment-updated").is_some());
}

/// Spec 0092, T13: Eine Anhebung auf **Gelb** eskaliert nicht — die Schwelle
/// ist Rot, nicht „irgendeine Anhebung".
#[tokio::test]
async fn test_t13_second_opinion_raising_only_to_yellow_does_not_escalate() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(PLAIN, output("total 0")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(
        &mut session,
        VerdictProvider("yellow: könnte interne Pfade zeigen"),
    );

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handle_action_proposed(
            &session,
            Uuid::new_v4(),
            AiAction::SuggestCommand {
                command: PLAIN.to_string(),
            },
            &emitter,
            &profile_store,
            &confirmations,
            ActionOrigin::Internal,
            test_fresh_rejection_flag(),
        ),
    )
    .await
    .expect("Gelb eskaliert nicht, die Aktion läuft ohne Dialog durch");

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(
        events
            .iter()
            .find(|(name, _)| name == "risk-assessment-updated")
            .expect("Badge-Update muss kommen")
            .1["dataRisk"],
        serde_json::json!("yellow")
    );
    assert!(escalated_event(&events).is_none(), "{events:?}");
    assert_eq!(action_result_count(&events), 1, "{events:?}");
}

// --- T14: Stopp hat Vorrang -------------------------------------------

/// Eine Zweitmeinung, die erst antwortet, wenn der Test sie freigibt —
/// damit der Stopp beweisbar **während** der Zweitmeinung eintrifft und
/// nicht vorher oder nachher.
struct GatedVerdictProvider {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl AiProvider for GatedVerdictProvider {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(futures::stream::once(async move {
            entered.notify_one();
            release.notified().await;
            AiEvent::TextDelta("red: heikel".to_string())
        }))
    }
}

/// Spec 0092, T14/A3.3: Trifft ein Stopp ein, während die Zweitmeinung
/// läuft, wird die Aktion wie heute übersprungen — **kein** Dialog. Sonst
/// bekäme der Nutzer nach einem ausdrücklichen „Stopp" noch eine Rückfrage
/// zu einer Aktion, die er gerade abgebrochen hat.
#[tokio::test]
async fn test_t14_a_stop_during_the_second_opinion_wins_over_the_escalation() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(
        &mut session,
        GatedVerdictProvider {
            entered: entered.clone(),
            release: release.clone(),
        },
    );

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: YELLOW_ONLY.to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    let stopper = async {
        entered.notified().await;
        session.request_auto_continue_stop();
        release.notify_one();
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(handled, stopper)
    })
    .await
    .expect("die gestoppte Aktion muss ohne Dialog enden");

    let events = emitter.events.lock().unwrap().clone();
    assert!(
        escalated_event(&events).is_none(),
        "eine gestoppte Aktion bekommt keinen Dialog: {events:?}"
    );
    assert!(
        session.pending_action.lock().unwrap().is_none(),
        "kein Tab-Indikator für eine übersprungene Aktion"
    );
    // Spec 0066: übersprungen wird als abgebrochenes Kommando gemeldet.
    let (_, result) = events
        .iter()
        .find(|(name, _)| name == "chat-action-result")
        .expect("die übersprungene Aktion wird als abgebrochen gemeldet");
    assert_eq!(
        result["result"]["cancelled"],
        serde_json::json!(true),
        "{result}"
    );
    assert_eq!(
        result["result"]["exitCode"],
        serde_json::Value::Null,
        "{result}"
    );

    // **Gegenprobe im selben Test**: Ohne den Stopp eskaliert genau dieser
    // Aufbau. Ohne sie wäre die Zusicherung oben auch dann grün, wenn A3
    // überhaupt nichts täte — sie prüfte dann nichts.
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(
        &mut session,
        GatedVerdictProvider {
            entered: entered.clone(),
            release: release.clone(),
        },
    );
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: YELLOW_ONLY.to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    let releaser = async {
        entered.notified().await;
        release.notify_one();
        resolve_first_escalated_action(&emitter, &confirmations, ActionUserDecision::Deny).await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(handled, releaser)
    })
    .await
    .expect("Dialog muss enden");
    assert!(
        escalated_event(&emitter.events.lock().unwrap().clone()).is_some(),
        "ohne Stopp muss derselbe Aufbau eskalieren — sonst prüft die Zusicherung oben nichts"
    );
}

// --- T15: dieselbe Zeitgrenze wie jede Bestätigung --------------------

/// Spec 0092, T15/A3.1: Der eskalierte Wartepfad ist **derselbe** — also
/// gilt auch `PENDING_ACTION_CONFIRM_TIMEOUT`. Bleibt der Klick aus, wird
/// nichts ausgeführt und der Tab-Indikator fällt wieder, statt für immer zu
/// hängen.
#[tokio::test(start_paused = true)]
async fn test_t15_an_escalated_confirmation_still_times_out_without_executing() {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(&mut session, VerdictProvider("red: heikel"));

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: YELLOW_ONLY.to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    // Niemand klickt. Erst warten, bis die Eskalation wirklich wartet, dann
    // die (virtuelle) Uhr über die Zeitgrenze spulen.
    // **Begrenzt statt endlos**: Tritt die Eskalation nicht ein (der Fall,
    // gegen den dieser Test schützt), wird nie etwas wartend registriert.
    // Eine endlose `yield_now`-Schleife hielte dann die virtuelle Uhr an, und
    // auch ein äußeres `tokio::time::timeout` könnte nie ablaufen — der Test
    // hinge, statt zu scheitern. Nach der Schranke endet die Schleife
    // einfach; die Zusicherungen unten scheitern dann sauber.
    let advancer = async {
        for _ in 0..10_000 {
            if session.pending_action.lock().unwrap().is_some() {
                tokio::time::advance(
                    PENDING_ACTION_CONFIRM_TIMEOUT + std::time::Duration::from_secs(1),
                )
                .await;
                return;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::join!(handled, advancer);

    let events = emitter.events.lock().unwrap().clone();
    assert!(
        escalated_event(&events).is_some(),
        "Vorbedingung: es wurde tatsächlich eskaliert"
    );
    assert_eq!(
        action_result_count(&events),
        0,
        "ein ausgebliebener Klick darf nie als Freigabe gelten: {events:?}"
    );
    assert!(
        session.pending_action.lock().unwrap().is_none(),
        "der Tab-Indikator muss nach der Zeitgrenze wieder fallen"
    );
}
