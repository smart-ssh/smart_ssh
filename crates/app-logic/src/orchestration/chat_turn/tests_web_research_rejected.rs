//! Issue #169 (Spec 0105 §7): lehnt das Provider-Konto die Web-Werkzeuge
//! ab, wiederholt der Haupt-Chat dieselbe Anfrage genau einmal ohne
//! Web-Werkzeuge und zeigt einen Hinweis — für keinen anderen Fehler.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use uuid::Uuid;

use ssh_manager_core::ai::{AiError, AiEvent, AiProvider, SessionContext};

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

/// Ein `send()`-Aufruf, wie ihn der Provider gesehen hat.
#[derive(Debug, Clone)]
struct SendRecord {
    /// `disable_web_research` war vor diesem Aufruf schon gerufen.
    web_research_disabled: bool,
    at: tokio::time::Instant,
    history_len: usize,
}

/// Liefert je `send()` die nächste vorgegebene Ereignisfolge und merkt sich
/// jeden Aufruf samt Web-Zustand.
struct ScriptedProvider {
    rounds: StdMutex<std::collections::VecDeque<Vec<AiEvent>>>,
    sends: Arc<StdMutex<Vec<SendRecord>>>,
    disabled: AtomicBool,
    disable_calls: Arc<AtomicUsize>,
}

impl ScriptedProvider {
    fn new(rounds: Vec<Vec<AiEvent>>) -> Self {
        Self {
            rounds: StdMutex::new(rounds.into()),
            sends: Arc::new(StdMutex::new(Vec::new())),
            disabled: AtomicBool::new(false),
            disable_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl AiProvider for ScriptedProvider {
    fn send(
        &self,
        context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        self.sends.lock().unwrap().push(SendRecord {
            web_research_disabled: self.disabled.load(Ordering::SeqCst),
            at: tokio::time::Instant::now(),
            history_len: context.history.len(),
        });
        let events = self
            .rounds
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| vec![AiEvent::Done]);
        Box::pin(futures::stream::iter(events))
    }

    fn disable_web_research(&self) {
        self.disabled.store(true, Ordering::SeqCst);
        self.disable_calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn rejected() -> AiEvent {
    AiEvent::Error(AiError::WebResearchRejected(
        r#"HTTP 400 Bad Request: {"type":"error","error":{"type":"invalid_request_error","message":"Web search is not enabled for this organization."}}"#
            .to_string(),
    ))
}

struct Run {
    sends: Vec<SendRecord>,
    disable_calls: usize,
    events: Vec<(String, serde_json::Value)>,
}

impl Run {
    fn names(&self) -> Vec<&str> {
        self.events.iter().map(|(n, _)| n.as_str()).collect()
    }
    fn count(&self, name: &str) -> usize {
        self.events.iter().filter(|(n, _)| n == name).count()
    }
}

async fn run_turn(rounds: Vec<Vec<AiEvent>>) -> Run {
    let provider = ScriptedProvider::new(rounds);
    let sends = provider.sends.clone();
    let disable_calls = provider.disable_calls.clone();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text("Was ist neu in nginx 1.29?".to_string()),
    });
    let emitter = TestEmitter::default();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    let sends = sends.lock().unwrap().clone();
    Run {
        sends,
        disable_calls: disable_calls.load(Ordering::SeqCst),
        events,
    }
}

/// AC 1: genau ein Wiederholversuch, erst nach dem Abschalten der
/// Web-Werkzeuge, mit demselben Kontext; die Antwort erscheint normal,
/// zusammen mit dem Hinweis, ohne Fehlerkarte.
#[tokio::test]
async fn test_web_tool_rejection_retries_once_without_web_tools_and_shows_hint() {
    let run = run_turn(vec![
        vec![rejected()],
        vec![
            AiEvent::TextDelta("Antwort ohne Web.".to_string()),
            AiEvent::Done,
        ],
    ])
    .await;

    assert_eq!(run.sends.len(), 2, "{:?}", run.sends);
    assert!(!run.sends[0].web_research_disabled);
    assert!(run.sends[1].web_research_disabled);
    assert_eq!(run.sends[0].history_len, run.sends[1].history_len);
    assert_eq!(run.disable_calls, 1);
    assert_eq!(
        run.count("chat-web-research-unavailable"),
        1,
        "{:?}",
        run.names()
    );
    assert_eq!(run.count("chat-error"), 0, "{:?}", run.names());
    let names = run.names();
    let hint = names
        .iter()
        .position(|n| *n == "chat-web-research-unavailable")
        .unwrap();
    let delta = names.iter().position(|n| *n == "chat-text-delta").unwrap();
    assert!(hint < delta, "{names:?}");
    // AC 5: der Hinweis trägt nur die Sitzung, nie den Fehlertext des
    // Providers.
    let (_, payload) = run
        .events
        .iter()
        .find(|(n, _)| n == "chat-web-research-unavailable")
        .unwrap();
    let payload_text = payload.to_string();
    assert!(!payload_text.contains("not enabled"), "{payload_text}");
    assert!(
        !payload_text.contains("invalid_request_error"),
        "{payload_text}"
    );
    assert_eq!(payload.as_object().unwrap().len(), 1, "{payload_text}");
}

/// AC 4: scheitert auch der Wiederholversuch, wird sein Fehler gezeigt, und
/// es folgt kein weiterer Versuch — auch dann nicht, wenn er (gegen die
/// Zusage des Providers) erneut als Web-Ablehnung gemeldet wird.
#[tokio::test]
async fn test_failed_retry_shows_its_error_and_is_not_retried_again() {
    for second in [
        AiEvent::Error(AiError::ProviderUnavailable("HTTP 500".to_string())),
        rejected(),
    ] {
        let run = run_turn(vec![
            vec![rejected()],
            vec![second.clone()],
            vec![rejected()],
        ])
        .await;

        assert_eq!(run.sends.len(), 2, "{second:?}: {:?}", run.sends);
        assert_eq!(run.disable_calls, 1);
        assert_eq!(run.count("chat-web-research-unavailable"), 1);
        assert_eq!(run.count("chat-error"), 1, "{second:?}: {:?}", run.names());
        let (_, payload) = run.events.iter().find(|(n, _)| n == "chat-error").unwrap();
        let AiEvent::Error(err) = &second else {
            unreachable!()
        };
        assert_eq!(payload["code"], err.code());
    }
}

/// AC 2/3: jeder andere Fehler wird wie bisher angezeigt und nicht
/// wiederholt, und die Web-Werkzeuge bleiben an.
#[tokio::test]
async fn test_other_errors_are_not_retried_by_this_mechanism() {
    for err in [
        AiError::ProviderUnavailable(
            r#"HTTP 400 Bad Request: {"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: too large"}}"#
                .to_string(),
        ),
        AiError::AuthenticationFailed,
        AiError::RateLimited,
        AiError::ProviderUnavailable("HTTP 529: overloaded_error".to_string()),
        AiError::ContextTooLarge,
        AiError::Timeout { secs: 90 },
        AiError::NetworkError("reset".to_string()),
    ] {
        let run = run_turn(vec![
            vec![AiEvent::Error(err.clone())],
            vec![AiEvent::TextDelta("darf nie kommen".to_string()), AiEvent::Done],
        ])
        .await;

        assert_eq!(run.sends.len(), 1, "{err:?}");
        assert_eq!(run.disable_calls, 0, "{err:?}");
        assert_eq!(run.count("chat-web-research-unavailable"), 0, "{err:?}");
        assert_eq!(run.count("chat-error"), 1, "{err:?}");
        assert_eq!(run.count("chat-text-delta"), 0, "{err:?}");
    }
}

/// Der Wiederholversuch läuft durch dieselbe Taktung wie jede Anfrage
/// (Mindestabstand vor dem Senden), statt sofort hinterherzuschießen.
#[tokio::test(start_paused = true)]
async fn test_retry_goes_through_the_same_request_pacing() {
    let run = run_turn(vec![
        vec![rejected()],
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done],
    ])
    .await;

    assert_eq!(run.sends.len(), 2);
    let gap = run.sends[1].at - run.sends[0].at;
    assert!(gap >= MIN_AI_REQUEST_SPACING, "{gap:?}");
}

/// Der Wiederholversuch läuft durch dasselbe Rate-Limit-Gate: meldet der
/// Budget-Wächter eine nötige Wartezeit, erscheint der Warte-Hinweis auch
/// vor dem zweiten Senden.
#[tokio::test(start_paused = true)]
async fn test_retry_goes_through_the_same_rate_limit_budget_gate() {
    let provider = ScriptedProvider::new(vec![
        vec![rejected()],
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done],
    ]);
    let sends = provider.sends.clone();
    let mut session = session_with_ai_provider(provider, MockSshTransport::default());
    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text("hallo".to_string()),
    });
    let budget = Arc::new(ai_providers::ProviderBudgetGuard::new());
    budget.record_headers(exhausted_budget_snapshot());
    session.parts_mut_for_tests().ai_provider_budget = budget;
    let emitter = TestEmitter::default();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;

    assert_eq!(sends.lock().unwrap().len(), 2);
    let names: Vec<String> = emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let waits: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| *n == "ai-budget-waiting")
        .map(|(i, _)| i)
        .collect();
    let hint = names
        .iter()
        .position(|n| n == "chat-web-research-unavailable")
        .unwrap();
    assert!(waits.iter().any(|&i| i > hint), "{names:?}");
}

/// Restbudget klar unter der 15 %-Schwelle (wie `low_budget_snapshot` in
/// `tests_continuation`) — der Wächter verlangt vor jedem Senden eine
/// Wartezeit.
fn exhausted_budget_snapshot() -> ai_providers::RateLimitHeaderSnapshot {
    ai_providers::RateLimitHeaderSnapshot {
        input_tokens: ai_providers::RawCounter {
            limit: Some(1_000),
            remaining: Some(50),
            reset_at: Some(std::time::Instant::now() + std::time::Duration::from_secs(30)),
        },
        ..Default::default()
    }
}
