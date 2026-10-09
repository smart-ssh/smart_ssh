//! Issue #102: eine ausgefallene KI-Zweitmeinung bzw. ein nicht gelaufener
//! Injection-Check ist sichtbar (Hinweis, Log) und schließt fail closed pro
//! Aktion — und schwächt nie etwas ab.

use std::sync::Arc;

use ssh_manager_core::ai::{AiError, AiEvent, AiProvider, SessionContext};
use ssh_manager_core::filter::FilterEngine;

use crate::events::TestEmitter;
use crate::test_support::log_capture;

use super::super::test_support::*;
use super::*;

const PLAIN: &str = "ls -la";
/// Kommt weder im Log noch im Hinweis vor, wenn alles richtig läuft.
const SECRET_FRAGMENT: &str = "S3cretPassw0rd-in-body";

struct FailingProvider;
impl AiProvider for FailingProvider {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![AiEvent::Error(
            AiError::ProviderUnavailable(format!("HTTP 500 body: {SECRET_FRAGMENT}")),
        )]))
    }
}

struct TextProvider(&'static str);
impl AiProvider for TextProvider {
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

fn attach_second_opinion(session: &mut Session, provider: impl AiProvider + 'static) {
    session.parts_mut_for_tests().risk_second_opinion_provider = Some(Box::new(provider));
    session.parts_mut_for_tests().risk_second_opinion_budget =
        Some(Arc::new(ai_providers::ProviderBudgetGuard::new()));
}

fn find<'a>(
    events: &'a [(String, serde_json::Value)],
    name: &str,
) -> Option<&'a serde_json::Value> {
    events.iter().find(|(n, _)| n == name).map(|(_, p)| p)
}

async fn resolve_first_escalated(
    emitter: &TestEmitter,
    confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) {
    loop {
        let events = emitter.events.lock().unwrap().clone();
        // Entweder nachträglich eskaliert, oder von Anfang an `Confirm`.
        let id = find(&events, "action-decision-escalated")
            .or_else(|| {
                find(&events, "chat-action-proposed")
                    .filter(|p| p["decision"].get("Confirm").is_some())
            })
            .map(|p| p["actionId"].as_str().unwrap().to_string());
        if let Some(id) = id {
            if confirmations
                .resolve(&id.parse().unwrap(), ActionUserDecision::Deny)
                .is_ok()
            {
                return;
            }
        }
        tokio::task::yield_now().await;
    }
}

/// Fährt eine `AutoExec`-Aktion mit der gegebenen Zweitmeinung durch und
/// liefert die Events (Dialog wird, falls er kommt, abgelehnt).
async fn propose_with_second_opinion(
    provider: impl AiProvider + 'static,
    command: &str,
) -> Vec<(String, serde_json::Value)> {
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(command, output("ok")),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    attach_second_opinion(&mut session, provider);
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: command.to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    // Wartet entweder auf die Eskalation (dann ablehnen) oder auf das Ende.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::select! {
            _ = handled => {}
            _ = resolve_first_escalated(&emitter, &confirmations) => {
                // nach dem Ablehnen läuft `handled` nicht weiter in diesem
                // Zweig — der Test prüft nur die bis hierher gesendeten Events.
            }
        }
    })
    .await
    .expect("must finish");
    let events = emitter.events.lock().unwrap().clone();
    events
}

/// Akzeptanzkriterium 1 + fail closed: Provider-Fehler → Hinweis an der
/// Karte, Stufe bleibt regelbasiert, Aktion wird bestätigungspflichtig.
/// Gegenbeweis: vor der Änderung gab es weder das Feld noch das Event.
#[tokio::test]
async fn test_provider_error_marks_card_and_forces_confirmation() {
    log_capture::start_recording();
    let events = propose_with_second_opinion(FailingProvider, PLAIN).await;

    let updated = find(&events, "risk-assessment-updated").expect("badge update must be sent");
    assert_eq!(updated["secondOpinionUnavailable"], serde_json::json!(true));
    assert_eq!(
        updated["dataRisk"],
        serde_json::json!("none"),
        "the rule-based level stays"
    );
    let escalated = find(&events, "action-decision-escalated")
        .expect("an unavailable second opinion must force a confirmation");
    assert_eq!(
        escalated["code"],
        "FILTER_SECOND_OPINION_UNAVAILABLE_REQUIRES_CONFIRM"
    );
    // Reihenfolge: Badge/Hinweis vor dem Dialog.
    let pos = |n: &str| events.iter().position(|(e, _)| e == n).unwrap();
    assert!(pos("risk-assessment-updated") < pos("action-decision-escalated"));
    assert!(
        find(&events, "chat-action-result").is_none(),
        "nothing may run before the click"
    );

    // Kein Fehlertext/Body im Log oder in den Events.
    let log = log_capture::recorded_text();
    assert!(
        log.contains("\"code\":\"AI_PROVIDER_UNAVAILABLE\""),
        "the error kind must be logged: {log}"
    );
    assert!(!log.contains(SECRET_FRAGMENT), "{log}");
    // From `info` up no command text (the filter's `debug` log is Spec 0094).
    let lines = log_capture::recorded_lines_at_info_or_above();
    assert!(
        !lines.iter().any(|l| l.contains(PLAIN)),
        "command text must not be logged: {lines:?}"
    );
    let all = serde_json::to_string(&events).unwrap();
    assert!(!all.contains(SECRET_FRAGMENT), "{all}");
}

/// Eine Antwort ohne erkennbares Urteil gilt ebenfalls als "nicht geprüft".
#[tokio::test]
async fn test_answer_without_verdict_is_not_treated_as_unremarkable() {
    let events = propose_with_second_opinion(TextProvider("I do not know."), PLAIN).await;
    let updated = find(&events, "risk-assessment-updated").unwrap();
    assert_eq!(updated["secondOpinionUnavailable"], serde_json::json!(true));
    assert!(find(&events, "action-decision-escalated").is_some());
}

/// Geprüftes, unauffälliges Ergebnis: kein Hinweis, keine Eskalation.
#[tokio::test]
async fn test_successful_second_opinion_has_no_notice_and_no_extra_confirm() {
    let events = propose_with_second_opinion(TextProvider("VERDICT: none\nfine"), PLAIN).await;
    let updated = find(&events, "risk-assessment-updated").unwrap();
    assert_eq!(
        updated["secondOpinionUnavailable"],
        serde_json::json!(false)
    );
    assert!(find(&events, "action-decision-escalated").is_none());
    assert!(find(&events, "chat-action-result").is_some());
}

/// Eskalation nur in eine Richtung: ein regelbasiertes Rot bleibt Rot, auch
/// wenn die Zweitmeinung ausfällt.
#[tokio::test]
async fn test_unavailable_second_opinion_never_lowers_rule_based_red() {
    let events = propose_with_second_opinion(FailingProvider, "cat ~/.ssh/id_rsa").await;
    let updated = find(&events, "risk-assessment-updated").unwrap();
    assert_eq!(updated["dataRisk"], serde_json::json!("red"));
    assert_eq!(updated["secondOpinionUnavailable"], serde_json::json!(true));
}

// --- Injection-Check -------------------------------------------------------

fn session_with_injection_check(provider: impl AiProvider + 'static) -> Session {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.parts_mut_for_tests().injection_check_provider = Some(Box::new(provider));
    session
}

/// Provider-Fehler: Hinweis in der Sitzung, `warn`-Log ohne Inhalt,
/// `injection_suspected` unverändert, nächste Aktion braucht Bestätigung.
#[tokio::test]
async fn test_injection_check_provider_error_notifies_and_forces_next_confirmation() {
    log_capture::start_recording();
    let session = session_with_injection_check(FailingProvider);
    let emitter = TestEmitter::default();

    check_for_injected_instructions(&session, Uuid::new_v4(), &emitter, "file body cat-me").await;

    let events = emitter.events.lock().unwrap().clone();
    let notice = find(&events, "chat-error").expect("a session notice must be shown");
    assert_eq!(notice["code"], "AI_INJECTION_CHECK_UNAVAILABLE");
    let all = serde_json::to_string(&events).unwrap();
    assert!(
        !all.contains(SECRET_FRAGMENT) && !all.contains("cat-me"),
        "{all}"
    );
    let log = log_capture::recorded_text();
    assert!(log.contains("\"level\":\"WARN\""), "{log}");
    assert!(
        !log.contains(SECRET_FRAGMENT) && !log.contains("cat-me"),
        "{log}"
    );
    assert!(
        !session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "a failed check must not invent a suspicion"
    );

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;
    assert!(matches!(decision, Decision::Confirm { .. }), "{payload}");
    assert_eq!(
        payload["decision"]["Confirm"]["code"],
        serde_json::json!("FILTER_INJECTION_CHECK_UNAVAILABLE_REQUIRES_CONFIRM")
    );

    // Pro Aktion: die übernächste läuft wieder normal.
    let (decision, _) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;
    assert!(matches!(decision, Decision::AutoExec));
}

/// Kein erkennbares Urteil → wie "nicht gelaufen" (Spec 0074).
#[tokio::test]
async fn test_injection_check_without_verdict_counts_as_unavailable() {
    let session = session_with_injection_check(TextProvider("hmm"));
    let emitter = TestEmitter::default();
    check_for_injected_instructions(&session, Uuid::new_v4(), &emitter, "content").await;
    assert!(find(&emitter.events.lock().unwrap().clone(), "chat-error").is_some());
    assert!(session
        .injection_check_unavailable
        .load(std::sync::atomic::Ordering::SeqCst));
}

/// Ein früherer Verdacht wird von einem ausgefallenen Check nie gelöscht.
#[tokio::test]
async fn test_failed_injection_check_never_clears_earlier_suspicion() {
    let session = session_with_injection_check(FailingProvider);
    session
        .injection_suspected
        .store(true, std::sync::atomic::Ordering::SeqCst);
    check_for_injected_instructions(&session, Uuid::new_v4(), &TestEmitter::default(), "x").await;
    assert!(session
        .injection_suspected
        .load(std::sync::atomic::Ordering::SeqCst));
}

/// Ein sauberer Check ("nein") erzeugt weder Hinweis noch Flag.
#[tokio::test]
async fn test_successful_injection_check_has_no_notice() {
    let session = session_with_injection_check(TextProvider("VERDICT: no\nfine"));
    let emitter = TestEmitter::default();
    check_for_injected_instructions(&session, Uuid::new_v4(), &emitter, "content").await;
    assert!(find(&emitter.events.lock().unwrap().clone(), "chat-error").is_none());
    assert!(!session
        .injection_check_unavailable
        .load(std::sync::atomic::Ordering::SeqCst));
}

/// Ein `Deny` verbraucht das Flag nicht (wie beim Verdacht, Spec 0039).
#[tokio::test]
async fn test_unavailable_flag_survives_a_denied_action() {
    let session = session_with_injection_check(FailingProvider);
    check_for_injected_instructions(&session, Uuid::new_v4(), &TestEmitter::default(), "x").await;
    let (decision, _) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "rm -rf /".to_string(),
        },
    )
    .await;
    assert!(!matches!(decision, Decision::AutoExec));
    assert!(session
        .injection_check_unavailable
        .load(std::sync::atomic::Ordering::SeqCst));
}

// --- Sitzungs-Hinweis beim Verbinden ---------------------------------------

#[tokio::test]
async fn test_setup_failure_notice_is_sent_once_per_session() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session
        .second_opinion_setup_notice_pending
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let emitter = TestEmitter::default();
    let sid = Uuid::new_v4();
    announce_second_opinion_setup_failure(&session, sid, &emitter);
    announce_second_opinion_setup_failure(&session, sid, &emitter);
    let events = emitter.events.lock().unwrap().clone();
    let notices: Vec<_> = events.iter().filter(|(n, _)| n == "chat-error").collect();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].1["code"], "AI_SECOND_OPINION_SETUP_FAILED");
}

#[tokio::test]
async fn test_no_setup_failure_means_no_notice() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let emitter = TestEmitter::default();
    announce_second_opinion_setup_failure(&session, Uuid::new_v4(), &emitter);
    assert!(emitter.events.lock().unwrap().is_empty());
}
