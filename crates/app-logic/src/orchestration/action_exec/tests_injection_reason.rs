//! Issue #693: progress/verdict events of the injection check and the
//! suspicion reason carried into the confirmation.

use ssh_manager_core::ai::AiEvent;

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

const SECRET: &str = "AKIAIOSFODNN7EXAMPLE";

fn session_answering(answer: String) -> Session {
    let mut session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    session.set_injection_check_provider_for_tests(Some(Box::new(MockAiProvider::new(vec![
        AiEvent::TextDelta(answer),
        AiEvent::Done,
    ]))));
    session
}

fn check_events(emitter: &TestEmitter) -> Vec<serde_json::Value> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "chat-injection-check")
        .map(|(_, p)| p.clone())
        .collect()
}

#[tokio::test]
async fn test_suspected_emits_running_then_redacted_truncated_reason() {
    let long = format!("{SECRET} {}", "x".repeat(600));
    let session = session_answering(format!("ja - {long}"));
    let emitter = TestEmitter::default();
    let item = Uuid::new_v4();
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &emitter,
        item,
        "body",
        "file_read",
    )
    .await;

    let evs = check_events(&emitter);
    assert_eq!(evs.len(), 2, "{evs:?}");
    assert_eq!(evs[0]["state"], "running");
    assert_eq!(evs[0]["itemId"], item.to_string());
    assert_eq!(evs[1]["state"], "suspected");
    let reason = evs[1]["reason"].as_str().unwrap();
    assert!(!reason.contains(SECRET), "{reason}");
    assert!(reason.chars().count() <= 300, "{}", reason.chars().count());

    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;
    let Decision::Confirm { reason: text, code } = decision else {
        panic!("expected Confirm: {payload}");
    };
    assert_eq!(code, "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM");
    assert!(
        text.ends_with(&format!("{INJECTION_REASON_MARKER}{reason}")),
        "{text}"
    );
    assert!(!text.contains(SECRET), "{text}");

    // The reason was consumed with the flag: a later, unrelated escalation
    // must not show it.
    assert!(session.injection_suspected_reason.lock().unwrap().is_none());
    session
        .injection_check_unavailable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (_, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc/hosts".to_string(),
        },
    )
    .await;
    assert!(
        !payload.to_string().contains("Reason:"),
        "old reason leaked: {payload}"
    );
}

#[tokio::test]
async fn test_reason_is_not_in_ai_context() {
    let session = session_answering("ja - unique-reason-marker".to_string());
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        Uuid::new_v4(),
        "body",
        "file_read",
    )
    .await;
    let history = format!("{:?}", session.context.lock().await.history);
    assert!(!history.contains("unique-reason-marker"), "{history}");
}

#[tokio::test]
async fn test_clean_emits_running_then_clean() {
    let session = session_answering("nein - fine".to_string());
    let emitter = TestEmitter::default();
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &emitter,
        Uuid::new_v4(),
        "b",
        "file_read",
    )
    .await;
    let evs = check_events(&emitter);
    let states: Vec<_> = evs.iter().map(|e| e["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["running", "clean"]);
}

#[tokio::test]
async fn test_no_events_without_provider() {
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    let emitter = TestEmitter::default();
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &emitter,
        Uuid::new_v4(),
        "b",
        "file_read",
    )
    .await;
    assert!(check_events(&emitter).is_empty());
}

#[tokio::test]
async fn test_combined_variant_carries_reason() {
    let session = session_answering("ja - looks injected".to_string());
    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        Uuid::new_v4(),
        "b",
        "file_read",
    )
    .await;
    let (decision, payload) = proposed_decision_code(
        &session,
        AiAction::SuggestCommand {
            command: "cat ~/.ssh/id_rsa".to_string(),
        },
    )
    .await;
    let Decision::Confirm { reason, code } = decision else {
        panic!("{payload}");
    };
    assert_eq!(code, "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM");
    assert!(reason.ends_with("\nReason: looks injected"), "{reason}");
}
