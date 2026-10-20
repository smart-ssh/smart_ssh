//! Issue #545: `ask_user` in the chat turn — event, storage, redaction,
//! invalid calls, no follow-up round, no filter/confirmation path.

use uuid::Uuid;

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::profiles::{AiAction, AskUserOption};

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::tests_web::HangingProvider;
use super::*;

const PLANTED_KEY: &str = "AKIAABCDEFGHIJKLMNOP";

fn opt(label: &str) -> AskUserOption {
    AskUserOption {
        label: label.to_string(),
        description: Some("short".to_string()),
    }
}

fn ask(question: &str, options: Vec<AskUserOption>, multi: bool) -> AiEvent {
    AiEvent::ActionProposed(AiAction::AskUser {
        question: question.to_string(),
        options,
        multi_select: multi,
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

fn question_events(emitter: &TestEmitter) -> Vec<serde_json::Value> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(n, _)| n == "chat-question-asked")
        .map(|(_, p)| p.clone())
        .collect()
}

#[tokio::test]
async fn test_valid_question_emits_one_event_stores_it_and_ends_turn() {
    let provider = MockAiProvider::with_rounds(vec![vec![
        ask("Which fix?", vec![opt("A"), opt("B")], true),
        AiEvent::Done,
    ]]);
    let sent = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();

    run_turn(&session, &emitter).await;

    assert_eq!(sent.lock().unwrap().len(), 1, "no follow-up round");
    let events = question_events(&emitter);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["question"], "Which fix?");
    assert_eq!(events[0]["multiSelect"], true);
    assert_eq!(events[0]["options"][1]["label"], "B");
    assert_eq!(events[0]["options"][0]["description"], "short");
    let all = emitter.events.lock().unwrap().clone();
    assert!(!all.iter().any(|(n, _)| n == "chat-action-proposed"
        || n == "chat-error"
        || n == "chat-response-empty"));

    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1, "{history:?}");
    assert_eq!(history[0].role, Role::Assistant);
    let MessageContent::Question {
        question_id,
        question,
        options,
        multi_select,
    } = &history[0].content
    else {
        panic!("{history:?}");
    };
    assert_eq!(question_id.to_string(), events[0]["questionId"]);
    assert_eq!(question, "Which fix?");
    assert_eq!(options.len(), 2);
    assert!(*multi_select);
}

#[tokio::test]
async fn test_invalid_questions_are_shown_as_text_without_event() {
    let long = "x".repeat(81);
    let cases = vec![
        vec![opt("only")],
        vec![opt("1"), opt("2"), opt("3"), opt("4"), opt("5")],
        vec![opt("fine"), opt("")],
        vec![opt("fine"), opt(&long)],
        vec![
            opt("fine"),
            AskUserOption {
                label: "d".into(),
                description: Some("y".repeat(201)),
            },
        ],
    ];
    for options in cases {
        let provider = MockAiProvider::with_rounds(vec![vec![
            ask("Which fix?", options, false),
            AiEvent::Done,
        ]]);
        let session = session_with_ai_provider(provider, MockSshTransport::default());
        let emitter = TestEmitter::default();
        run_turn(&session, &emitter).await;

        assert!(question_events(&emitter).is_empty());
        let history = session.context.lock().await.history.clone();
        assert_eq!(history.len(), 1, "{history:?}");
        let MessageContent::Text(text) = &history[0].content else {
            panic!("{history:?}");
        };
        assert!(text.starts_with("Which fix?"), "{text}");
        assert!(text.contains("\n- "), "{text}");
    }
}

#[tokio::test]
async fn test_overlong_question_is_shown_as_text() {
    let provider = MockAiProvider::with_rounds(vec![vec![
        ask(&"q".repeat(501), vec![opt("A"), opt("B")], false),
        AiEvent::Done,
    ]]);
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();
    run_turn(&session, &emitter).await;
    assert!(question_events(&emitter).is_empty());
    let history = session.context.lock().await.history.clone();
    assert!(matches!(history[0].content, MessageContent::Text(_)));
}

#[tokio::test]
async fn test_question_texts_are_redacted_before_emit_and_storage() {
    let provider = MockAiProvider::with_rounds(vec![vec![
        ask(
            &format!("Use key {PLANTED_KEY}?"),
            vec![opt(&format!("key {PLANTED_KEY}")), opt("no")],
            false,
        ),
        AiEvent::Done,
    ]]);
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();
    run_turn(&session, &emitter).await;

    let events = question_events(&emitter);
    assert_eq!(events.len(), 1);
    assert!(
        !events[0].to_string().contains(PLANTED_KEY),
        "{}",
        events[0]
    );
    let history = session.context.lock().await.history.clone();
    assert!(!format!("{history:?}").contains(PLANTED_KEY), "{history:?}");
}

#[tokio::test]
async fn test_stopped_turn_emits_no_question() {
    let provider = HangingProvider(vec![AiEvent::TextDelta("Let me ask: ".to_string())]);
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();
    let stopper = async {
        loop {
            if !session.context.lock().await.history.is_empty()
                || emitter
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(n, _)| n == "chat-text-delta")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        session.request_auto_continue_stop();
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(run_turn(&session, &emitter), stopper)
    })
    .await
    .expect("stop must end the turn");
    assert!(question_events(&emitter).is_empty());
    let history = session.context.lock().await.history.clone();
    assert!(!history
        .iter()
        .any(|m| matches!(m.content, MessageContent::Question { .. })));
}

#[tokio::test]
async fn test_truncated_response_emits_no_question() {
    // The providers drop a truncated tool call (invariant 5); the turn then
    // sees only `TextTruncated`.
    let provider = MockAiProvider::with_rounds(vec![vec![
        AiEvent::TextDelta("Which".to_string()),
        AiEvent::TextTruncated,
    ]]);
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();
    run_turn(&session, &emitter).await;
    assert!(question_events(&emitter).is_empty());
}
