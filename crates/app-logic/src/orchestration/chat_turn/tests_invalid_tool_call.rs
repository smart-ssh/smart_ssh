//! Issue #604: unknown or invalid tool calls are non-fatal — feedback to
//! the model, valid calls of the same response still run, a loop guard ends
//! the turn after two responses with only invalid calls.

use uuid::Uuid;

use ssh_manager_core::ai::{AiEvent, InvalidToolCallReason};
use ssh_manager_core::profiles::AiAction;

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

fn invalid(name: &str, reason: InvalidToolCallReason) -> AiEvent {
    AiEvent::InvalidToolCall {
        tool_name: name.to_string(),
        reason,
    }
}

fn suggest(command: &str) -> AiEvent {
    AiEvent::ActionProposed(AiAction::SuggestCommand {
        command: command.to_string(),
    })
}

async fn run_turn(session: &Session, emitter: &TestEmitter) {
    run_chat_turn(
        session,
        Uuid::new_v4(),
        emitter,
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
}

fn event_names(emitter: &TestEmitter) -> Vec<String> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|(n, _)| n.clone())
        .collect()
}

fn feedback_for(history: &[ChatMessage], tool: &str) -> Option<String> {
    history.iter().find_map(|m| match &m.content {
        MessageContent::ActionRejected {
            command,
            reason: RejectionReason::Blocked(text),
        } if command == tool => Some(text.clone()),
        _ => None,
    })
}

#[tokio::test]
async fn test_unknown_tool_does_not_drop_valid_call_of_same_response() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            invalid("date", InvalidToolCallReason::UnknownTool),
            suggest("ls"),
            AiEvent::Done,
        ],
        vec![AiEvent::TextDelta("done".into()), AiEvent::Done],
    ]);
    let sent = provider.received_contexts_handle();
    let transport = MockSshTransport::default().with_response("ls", output("a b"));
    let executed = transport.executed_handle();
    let session = session_with_ai_provider_allowing_everything(provider, transport);
    let emitter = TestEmitter::default();

    run_turn(&session, &emitter).await;

    assert_eq!(*executed.lock().unwrap(), vec!["ls".to_string()]);
    assert!(!event_names(&emitter).contains(&"chat-error".to_string()));
    let contexts = sent.lock().unwrap().clone();
    assert_eq!(contexts.len(), 2);
    let text = feedback_for(&contexts[1].history, "date").expect("feedback for `date`");
    assert!(text.contains("unknown tool"), "{text}");
    assert!(text.contains("suggest_command"), "{text}");
}

#[tokio::test]
async fn test_only_invalid_call_triggers_exactly_one_followup_with_feedback() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            invalid(
                "suggest_command",
                InvalidToolCallReason::MissingField("command".into()),
            ),
            AiEvent::Done,
        ],
        vec![AiEvent::TextDelta("ok".into()), AiEvent::Done],
    ]);
    let sent = provider.received_contexts_handle();
    let transport = MockSshTransport::default();
    let executed = transport.executed_handle();
    let session = session_with_ai_provider_allowing_everything(provider, transport);
    let emitter = TestEmitter::default();

    run_turn(&session, &emitter).await;

    let contexts = sent.lock().unwrap().clone();
    assert_eq!(contexts.len(), 2);
    let text = feedback_for(&contexts[1].history, "suggest_command").expect("feedback");
    assert!(text.contains("field `command` missing"), "{text}");
    assert!(executed.lock().unwrap().is_empty());
    let names = event_names(&emitter);
    assert!(!names.contains(&"chat-error".to_string()));
    assert!(!names.contains(&"chat-action-proposed".to_string()));
}

#[tokio::test]
async fn test_two_only_invalid_responses_end_the_turn_without_third_request() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            invalid("date", InvalidToolCallReason::UnknownTool),
            AiEvent::Done,
        ],
        vec![
            invalid("date", InvalidToolCallReason::UnknownTool),
            AiEvent::Done,
        ],
        vec![AiEvent::TextDelta("never".into()), AiEvent::Done],
    ]);
    let sent = provider.received_contexts_handle();
    let session =
        session_with_ai_provider_allowing_everything(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();

    run_turn(&session, &emitter).await;

    assert_eq!(sent.lock().unwrap().len(), 2, "no third request");
    let events = emitter.events.lock().unwrap().clone();
    let (_, payload) = events
        .iter()
        .find(|(n, _)| n == "chat-error")
        .expect("chat-error");
    assert_eq!(payload["code"], "AI_INVALID_TOOL_CALL");
}

#[tokio::test]
async fn test_valid_call_resets_the_invalid_streak() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            invalid("date", InvalidToolCallReason::UnknownTool),
            AiEvent::Done,
        ],
        vec![suggest("ls"), AiEvent::Done],
        vec![
            invalid("date", InvalidToolCallReason::UnknownTool),
            AiEvent::Done,
        ],
        vec![AiEvent::TextDelta("fine".into()), AiEvent::Done],
    ]);
    let sent = provider.received_contexts_handle();
    let transport = MockSshTransport::default().with_response("ls", output("x"));
    let session = session_with_ai_provider_allowing_everything(provider, transport);
    let emitter = TestEmitter::default();

    run_turn(&session, &emitter).await;

    assert_eq!(sent.lock().unwrap().len(), 4);
    assert!(!event_names(&emitter).contains(&"chat-error".to_string()));
}
