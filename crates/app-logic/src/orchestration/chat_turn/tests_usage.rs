//! Issue #610: observers of completed AI requests (Spec 0038, section 2).

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use ssh_manager_core::ai::{AiUsage, ProviderType};
use ssh_manager_core::filter::FilterEngine;
use ssh_manager_core::profiles::AiAction;

use crate::ai_usage::{
    AiUsageObserver, AiUsagePurpose, AiUsageRecord, AiUsageReporting, AiUsageSource,
};
use crate::events::TestEmitter;
use crate::orchestration::action_exec::check_for_injected_instructions;
use crate::orchestration::{
    generate_session_title_on_disconnect, suggest_note_update_on_disconnect,
};

use super::super::test_support::*;
use super::tests_web::HangingProvider;
use super::*;

struct Recorder(std::sync::Mutex<mpsc::Sender<AiUsageRecord>>);

impl AiUsageObserver for Recorder {
    fn on_usage(&self, record: &AiUsageRecord) {
        let _ = self.0.lock().unwrap().send(record.clone());
    }
}

struct Panicker;

impl AiUsageObserver for Panicker {
    fn on_usage(&self, _record: &AiUsageRecord) {
        panic!("observer bug");
    }
}

fn recorder() -> (Arc<dyn AiUsageObserver>, mpsc::Receiver<AiUsageRecord>) {
    let (tx, rx) = mpsc::channel();
    (Arc::new(Recorder(std::sync::Mutex::new(tx))), rx)
}

fn reporting(observers: Vec<Arc<dyn AiUsageObserver>>) -> AiUsageReporting {
    AiUsageReporting {
        observers: Arc::new(observers),
        runtime_session_id: "tab-1".to_string(),
        main: AiUsageSource {
            provider_config_id: "main-config".to_string(),
            provider_type: ProviderType::Anthropic,
            configured_model: "main-model".to_string(),
        },
        side_call: Some(AiUsageSource {
            provider_config_id: "side-config".to_string(),
            provider_type: ProviderType::OpenAi,
            configured_model: "side-model".to_string(),
        }),
    }
}

fn usage(n: u64) -> AiUsage {
    AiUsage {
        input_tokens: Some(n),
        output_tokens: Some(1),
        ..AiUsage::default()
    }
}

fn next(rx: &mpsc::Receiver<AiUsageRecord>) -> AiUsageRecord {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("a usage record")
}

fn assert_no_more(rx: &mpsc::Receiver<AiUsageRecord>) {
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "no further record expected"
    );
}

async fn run_turn(session: &Session) -> TestEmitter {
    let emitter = TestEmitter::default();
    run_chat_turn(
        session,
        Uuid::new_v4(),
        &emitter,
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
    emitter
}

#[tokio::test]
async fn test_chat_turn_reports_one_record_with_ids_and_usage() {
    let (observer, rx) = recorder();
    let session = test_session(
        vec![
            AiEvent::TextDelta("hi".to_string()),
            AiEvent::RequestUsage(usage(42)),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    )
    .with_usage_reporting(Some(reporting(vec![observer])));
    let chat_id = Uuid::new_v4();
    *session.chat_session_id.lock().await = Some(chat_id);

    run_turn(&session).await;

    let record = next(&rx);
    assert_eq!(record.chat_session_id, Some(chat_id.to_string()));
    assert_eq!(record.server_id, Some(session.server_id.0.to_string()));
    assert_eq!(record.runtime_session_id, "tab-1");
    assert_eq!(record.provider_config_id, "main-config");
    assert_eq!(record.provider_type, ProviderType::Anthropic);
    assert_eq!(record.configured_model, "main-model");
    assert_eq!(record.purpose, AiUsagePurpose::ChatTurn);
    assert_eq!(record.usage, usage(42));
    assert_no_more(&rx);
}

#[tokio::test]
async fn test_follow_up_round_and_next_turn_each_report_their_own_record() {
    let (observer, rx) = recorder();
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::RequestUsage(usage(1)),
            AiEvent::Done,
        ],
        vec![AiEvent::RequestUsage(usage(2)), AiEvent::Done],
        // a second turn (the user continues)
        vec![AiEvent::RequestUsage(usage(3)), AiEvent::Done],
    ]);
    let session = session_with_ai_provider_allowing_everything(
        provider,
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    )
    .with_usage_reporting(Some(reporting(vec![observer])));

    run_turn(&session).await;
    let first = next(&rx);
    let second = next(&rx);
    assert_eq!(
        (first.usage.input_tokens, second.usage.input_tokens),
        (Some(1), Some(2))
    );
    assert_no_more(&rx);

    run_turn(&session).await;
    assert_eq!(next(&rx).usage.input_tokens, Some(3));
}

#[tokio::test]
async fn test_panicking_observer_does_not_break_turn_or_other_observers() {
    let (observer, rx) = recorder();
    let provider = MockAiProvider::with_rounds(vec![
        vec![AiEvent::RequestUsage(usage(1)), AiEvent::Done],
        vec![AiEvent::RequestUsage(usage(2)), AiEvent::Done],
    ]);
    let session = session_with_ai_provider(provider, MockSshTransport::default())
        .with_usage_reporting(Some(reporting(vec![Arc::new(Panicker), observer])));

    let first = run_turn(&session).await;
    assert_eq!(next(&rx).usage.input_tokens, Some(1));
    let second = run_turn(&session).await;
    assert_eq!(next(&rx).usage.input_tokens, Some(2));
    for emitter in [first, second] {
        assert!(!emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "chat-error"));
    }
}

#[tokio::test]
async fn test_slow_observer_does_not_delay_the_turn() {
    struct Slow;
    impl AiUsageObserver for Slow {
        fn on_usage(&self, _record: &AiUsageRecord) {
            std::thread::sleep(Duration::from_secs(3));
        }
    }
    let session = test_session(
        vec![AiEvent::RequestUsage(usage(1)), AiEvent::Done],
        MockSshTransport::default(),
    )
    .with_usage_reporting(Some(reporting(vec![Arc::new(Slow)])));
    let started = std::time::Instant::now();
    run_turn(&session).await;
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn test_second_opinion_reports_with_side_call_provider() {
    let (observer, rx) = recorder();
    let mut session = session_with_second_opinion(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("total 0")),
        MockAiProvider::new(vec![
            AiEvent::TextDelta("none".to_string()),
            AiEvent::RequestUsage(usage(7)),
            AiEvent::Done,
        ]),
    )
    .with_usage_reporting(Some(reporting(vec![observer])));
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));

    run_turn(&session).await;

    let record = next(&rx);
    assert_eq!(record.purpose, AiUsagePurpose::RiskSecondOpinion);
    assert_eq!(record.provider_config_id, "side-config");
    assert_eq!(record.usage, usage(7));
    assert_no_more(&rx);
}

#[tokio::test]
async fn test_injection_check_reports_its_purpose() {
    let (observer, rx) = recorder();
    let mut session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default())
            .with_usage_reporting(Some(reporting(vec![observer])));
    session.set_injection_check_provider_for_tests(Some(Box::new(MockAiProvider::new(vec![
        AiEvent::TextDelta("nein".to_string()),
        AiEvent::RequestUsage(usage(5)),
        AiEvent::Done,
    ]))));

    check_for_injected_instructions(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        Uuid::new_v4(),
        "x",
        "command_output",
    )
    .await;

    let record = next(&rx);
    assert_eq!(record.purpose, AiUsagePurpose::InjectionCheck);
    assert_eq!(record.provider_config_id, "side-config");
    assert_no_more(&rx);
}

#[tokio::test]
async fn test_title_reports_its_purpose() {
    let (observer, rx) = recorder();
    let (session, _store, _id, _tmp) = session_with_real_chat_persistence(
        vec![
            AiEvent::TextDelta("Festplatte".to_string()),
            AiEvent::RequestUsage(usage(9)),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    )
    .await;
    let session = session.with_usage_reporting(Some(reporting(vec![observer])));
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("räum auf".to_string()),
        },
    )
    .await;

    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;

    let record = next(&rx);
    assert_eq!(record.purpose, AiUsagePurpose::SessionTitle);
    assert_eq!(record.provider_config_id, "main-config");
    assert_no_more(&rx);
}

fn note_history() -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::CommandResult {
            command: "uptime".to_string(),
            output: output("up 3 days"),
            cancelled: false,
            waiting_for_input: false,
        },
    }
}

#[tokio::test]
async fn test_note_suggestion_reports_its_purpose() {
    let (observer, rx) = recorder();
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::RequestUsage(usage(4)), AiEvent::Done]),
        MockSshTransport::default(),
    )
    .with_usage_reporting(Some(reporting(vec![observer])));
    session.context.lock().await.history.push(note_history());

    suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;

    assert_eq!(next(&rx).purpose, AiUsagePurpose::NoteSuggestion);
    assert_no_more(&rx);
}

/// A stream that never ends after its stop event: the note suggestion and the
/// chat turn still finish, and usage arriving later is not reported.
#[tokio::test]
async fn test_usage_after_the_stop_event_is_not_awaited_or_reported() {
    let (observer, rx) = recorder();
    // Not a note proposal of this server: the suggestion ends right after
    // the stop event instead of waiting for the user.
    let proposal = AiAction::SuggestCommand {
        command: "ls".to_string(),
    };
    let session = session_with_ai_provider(
        HangingProvider(vec![
            AiEvent::ActionProposed(proposal),
            AiEvent::RequestUsage(usage(1)),
        ]),
        MockSshTransport::default(),
    )
    .with_usage_reporting(Some(reporting(vec![observer.clone()])));
    session.context.lock().await.history.push(note_history());
    tokio::time::timeout(
        Duration::from_secs(20),
        suggest_note_update_on_disconnect(
            &session,
            Uuid::new_v4(),
            &TestEmitter::default(),
            &InMemoryProfileStore::default(),
            &ConfirmationRegistry::new(),
        ),
    )
    .await
    .expect("note suggestion must finish");
    assert_no_more(&rx);

    let session = session_with_ai_provider(
        HangingProvider(vec![
            AiEvent::TextDelta("x".to_string()),
            AiEvent::Done,
            AiEvent::RequestUsage(usage(1)),
        ]),
        MockSshTransport::default(),
    )
    .with_usage_reporting(Some(reporting(vec![observer])));
    tokio::time::timeout(Duration::from_secs(20), run_turn(&session))
        .await
        .expect("chat turn must finish");
    assert_no_more(&rx);
}
