//! Issue #843: a profile-store failure while reading the server tags never
//! lets an `AutoExec` through; "server not stored" keeps today's decision.

use ssh_manager_core::ai::AiEvent;

use crate::events::TestEmitter;
use crate::test_support::log_capture;

use super::super::test_support::*;
use super::*;

const CMD: &str = "ls -la";

async fn propose(store: &InMemoryProfileStore) -> Vec<(String, serde_json::Value)> {
    let session = test_session_allowing_everything(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(CMD, output("ok")),
    );
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let handled = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: CMD.to_string(),
        },
        &emitter,
        store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    // A `Confirm` waits for a click: reject it so the call returns.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::select! {
            _ = handled => {}
            _ = async {
                loop {
                    let id = emitter
                        .events
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|(n, _)| n == "chat-action-proposed")
                        .map(|(_, p)| p["actionId"].as_str().unwrap().to_string());
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
            } => {}
        }
    })
    .await
    .expect("must finish");
    let events = emitter.events.lock().unwrap().clone();
    events
}

fn proposed_decision(events: &[(String, serde_json::Value)]) -> serde_json::Value {
    events
        .iter()
        .find(|(n, _)| n == "chat-action-proposed")
        .map(|(_, p)| p["decision"].clone())
        .expect("proposal event")
}

#[tokio::test]
async fn test_store_failure_reading_tags_turns_allow_into_confirm() {
    log_capture::start_recording();
    let store = InMemoryProfileStore::default();
    store
        .fail_get_server
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let events = propose(&store).await;
    let decision = proposed_decision(&events);
    assert_eq!(
        decision["Confirm"]["code"], "FILTER_SERVER_TAGS_UNAVAILABLE",
        "{decision}"
    );
    assert!(events.iter().all(|(n, _)| n != "chat-action-result"));
    let log = log_capture::recorded_text();
    assert!(
        !log.contains("disk I/O error"),
        "error text must not be logged: {log}"
    );
}

/// Counter-check: `ServerNotFound` keeps today's decision (auto run).
#[tokio::test]
async fn test_server_not_found_keeps_auto_exec() {
    let store = InMemoryProfileStore::default();
    let events = propose(&store).await;
    assert_eq!(proposed_decision(&events), serde_json::json!("AutoExec"));
}

/// Issue #843: the redaction notice is sent once per flag.
#[test]
fn test_redaction_incomplete_notice_is_sent_once() {
    let session = test_session_allowing_everything(vec![], MockSshTransport::default());
    let emitter = TestEmitter::default();
    let count = |emitter: &TestEmitter| {
        emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p)| p["code"] == "REDACTION_INCOMPLETE")
            .count()
    };
    announce_second_opinion_setup_failure(&session, Uuid::new_v4(), &emitter);
    assert_eq!(count(&emitter), 0, "no notice without the flag");
    session
        .safety
        .redaction_incomplete_notice_pending
        .store(true, std::sync::atomic::Ordering::SeqCst);
    announce_second_opinion_setup_failure(&session, Uuid::new_v4(), &emitter);
    announce_second_opinion_setup_failure(&session, Uuid::new_v4(), &emitter);
    assert_eq!(count(&emitter), 1);
}
