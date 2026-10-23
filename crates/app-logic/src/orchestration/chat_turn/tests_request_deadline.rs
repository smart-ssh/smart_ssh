//! Issue #694: total request deadline, wait-state event and request log
//! lines (first content, finish with outcome; no content in the log).

use std::time::Duration;

use uuid::Uuid;

use ssh_manager_core::ai::{AiError, AiEvent, AiProvider, SessionContext};

use crate::events::TestEmitter;
use crate::test_support::log_capture;

use super::super::test_support::*;
use super::*;

const SECRET_TEXT: &str = "SECRET-ANSWER-TEXT";

/// Streams `SECRET_TEXT` deltas every `every` forever.
struct SlowDeltas {
    every: Duration,
}

impl AiProvider for SlowDeltas {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let every = self.every;
        Box::pin(futures::stream::unfold((), move |()| async move {
            tokio::time::sleep(every).await;
            Some((AiEvent::TextDelta(SECRET_TEXT.to_string()), ()))
        }))
    }
}

/// Never yields anything.
struct Silent;

impl AiProvider for Silent {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::pending())
    }
}

async fn session_for(provider: impl AiProvider + 'static) -> Session {
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text("hello".to_string()),
    });
    session
}

fn names(emitter: &TestEmitter) -> Vec<(String, serde_json::Value)> {
    emitter.events.lock().unwrap().clone()
}

async fn run(session: &Session, emitter: &TestEmitter) {
    run_chat_turn(
        session,
        Uuid::new_v4(),
        emitter,
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
}

/// Deltas every 30 s for 11 minutes: the request ends at 10 minutes with the
/// total-deadline error; the text received so far stays (flushed).
#[tokio::test(start_paused = true)]
async fn test_total_deadline_ends_a_request_that_keeps_streaming() {
    log_capture::start_recording();
    let session = session_for(SlowDeltas {
        every: Duration::from_secs(30),
    })
    .await;
    let emitter = TestEmitter::default();
    let start = tokio::time::Instant::now();

    run(&session, &emitter).await;

    assert_eq!(start.elapsed(), TOTAL_REQUEST_DEADLINE);
    let events = names(&emitter);
    let deltas = events
        .iter()
        .filter(|(n, _)| n == "chat-text-delta")
        .count();
    assert_eq!(
        deltas, 19,
        "text received before the deadline stays visible"
    );
    let (_, err) = events.iter().find(|(n, _)| n == "chat-error").unwrap();
    assert_eq!(err["code"], "AI_TOTAL_DEADLINE");
    assert_eq!(err["timeoutSecs"], 600);
    assert_eq!(err["providerName"], session.ai_provider_label.as_str());
    // The history keeps the partial answer.
    let history = session.context.lock().await.history.clone();
    assert!(history.iter().any(|m| matches!(
        &m.content,
        MessageContent::Text(t) if t.contains(SECRET_TEXT)
    )));

    let log = log_capture::recorded_text();
    assert!(log.contains("\"outcome\":\"total_deadline\""), "{log}");
    assert!(log.contains("time_to_first_content_ms"), "{log}");
    let lines = log_capture::recorded_lines_at_info_or_above();
    assert!(
        !lines.iter().any(|l| l.contains(SECRET_TEXT)),
        "answer text must not be logged: {lines:?}"
    );
}

/// The wait state is announced when the request is sent.
#[tokio::test(start_paused = true)]
async fn test_request_started_event_names_the_provider() {
    let session = session_for(SlowDeltas {
        every: Duration::from_secs(30),
    })
    .await;
    let emitter = TestEmitter::recording_request_started();
    run(&session, &emitter).await;
    let events = names(&emitter);
    let (_, started) = events
        .iter()
        .find(|(n, _)| n == "chat-ai-request-started")
        .expect("request-started event");
    assert_eq!(started["providerName"], session.ai_provider_label.as_str());
}

/// A provider error is logged with outcome `error`, a content timeout with
/// `content_timeout`.
#[tokio::test]
async fn test_error_outcomes_are_logged() {
    for (err, outcome) in [
        (AiError::Timeout { secs: 120 }, "content_timeout"),
        (AiError::AuthenticationFailed, "error"),
    ] {
        log_capture::start_recording();
        let session =
            session_for(MockAiProvider::with_rounds(vec![vec![AiEvent::Error(err)]])).await;
        let emitter = TestEmitter::default();
        run(&session, &emitter).await;
        let log = log_capture::recorded_text();
        assert!(log.contains(&format!("\"outcome\":\"{outcome}\"")), "{log}");
    }
}

/// Stop during the wait ends the request and logs `stopped_by_user`.
#[tokio::test(start_paused = true)]
async fn test_stop_during_wait_logs_stopped_by_user() {
    log_capture::start_recording();
    let session = session_for(Silent).await;
    let emitter = TestEmitter::default();
    let stopper = async {
        tokio::time::sleep(Duration::from_secs(7)).await;
        session.request_auto_continue_stop();
    };
    tokio::join!(run(&session, &emitter), stopper);

    let log = log_capture::recorded_text();
    assert!(log.contains("\"outcome\":\"stopped_by_user\""), "{log}");
    assert!(log.contains("\"duration_ms\":7000"), "{log}");
}
