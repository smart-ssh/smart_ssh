//! Issue #833: every provider request is built through `RedactedContext`. A
//! secret known to the session's redactor that sits in the history (or in the
//! content handed to a side call) never reaches the recording provider.

use uuid::Uuid;

use ssh_manager_core::ai::AiEvent;

use crate::events::TestEmitter;
use crate::orchestration::action_exec::check_for_injected_instructions;
use crate::orchestration::{
    generate_session_title_on_disconnect, run_chat_turn, suggest_note_update_on_disconnect,
};

use super::super::test_support::*;
use super::*;

const SECRET_TEXT: &str = "password=hunter2geheim";
const SECRET: &str = "hunter2geheim";

fn secret_message() -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: MessageContent::Text(format!("my login is {SECRET_TEXT}")),
    }
}

fn secret_command_result() -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::CommandResult {
            command: "cat creds".to_string(),
            output: output(&format!("{SECRET_TEXT}\n")),
            cancelled: false,
            waiting_for_input: false,
        },
    }
}

fn assert_secret_free(contexts: &std::sync::Mutex<Vec<SessionContext>>) {
    let sent = contexts.lock().unwrap();
    assert!(!sent.is_empty(), "the provider must have been called");
    for context in sent.iter() {
        let dump = format!("{:?}", context.history);
        assert!(
            !dump.contains(SECRET),
            "secret reached the provider: {dump}"
        );
        assert!(dump.contains("REDACTED"), "placeholder expected: {dump}");
    }
}

#[tokio::test]
async fn test_chat_turn_request_is_redacted() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    push_history(&session, secret_message()).await;
    push_history(&session, secret_command_result()).await;

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;

    assert_secret_free(&contexts);
}

#[tokio::test]
async fn test_title_request_is_redacted() {
    let (mut session, _chat_store, _chat_session_id, _tmp_dir) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    let provider =
        MockAiProvider::new(vec![AiEvent::TextDelta("Title".to_string()), AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    session.set_ai_provider_for_tests(Box::new(provider));
    push_history(&session, secret_message()).await;
    push_history(&session, secret_command_result()).await;

    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;

    assert_secret_free(&contexts);
}

#[tokio::test]
async fn test_disconnect_note_request_is_redacted() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    push_history(&session, secret_message()).await;
    push_history(&session, secret_command_result()).await;

    suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;

    assert_secret_free(&contexts);
}

#[tokio::test]
async fn test_injection_check_request_is_redacted() {
    let mut session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    let provider = MockAiProvider::new(vec![AiEvent::TextDelta("VERDICT: no".to_string())]);
    let contexts = provider.received_contexts_handle();
    session.set_injection_check_provider_for_tests(Some(Box::new(provider)));

    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        Uuid::new_v4(),
        &format!("output: {SECRET_TEXT}"),
        "test",
    )
    .await;

    assert_secret_free(&contexts);
}
