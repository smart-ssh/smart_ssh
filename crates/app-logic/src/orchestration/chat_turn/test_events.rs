//! Gemeinsame Warte-Helfer für die `chat_turn`-Tests (Issue #2): statt
//! eines nackten `tokio::time::timeout(5 s, …).expect("…")`, das im
//! Fehlerfall nur sagt, *dass* etwas fehlt, scheitern diese Helfer mit
//! einer Meldung, die das erwartete Ereignis nennt und alle bis dahin
//! tatsächlich gesendeten Events auflistet.

use std::future::Future;
use std::time::Duration;

use ssh_manager_core::filter::Decision;

use super::super::test_support::proposed_decision_code_with_emitter;
use super::{ActionOrigin, AiAction, Session};
use crate::events::TestEmitter;

/// Obergrenze, nach der ein Test als hängend gilt — derselbe Wert wie
/// zuvor an jeder einzelnen `tokio::time::timeout`-Stelle.
pub(super) const EVENT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Längste Payload-Darstellung je Event in der Fehlermeldung; längere
/// Payloads werden gekürzt, damit die Meldung lesbar bleibt.
const MAX_PAYLOAD_CHARS: usize = 300;

/// Wartet höchstens [`EVENT_WAIT_TIMEOUT`] auf `fut`. Läuft die Zeit ab,
/// scheitert der Test mit einer Meldung, die `expected` (das erwartete
/// Ereignis, z. B. `"chat-action-proposed"` oder „Turn endet nach
/// Ablehnung“) nennt und alle bis dahin in `emitter` gesendeten Events
/// samt Payload auflistet.
pub(super) async fn expect_event_within<F: Future>(
    emitter: &TestEmitter,
    expected: &str,
    fut: F,
) -> F::Output {
    match tokio::time::timeout(EVENT_WAIT_TIMEOUT, fut).await {
        Ok(output) => output,
        Err(_) => panic!("{}", timeout_message(emitter, expected)),
    }
}

/// Wartet, bis ein Event namens `name` gesendet wurde, und gibt dessen
/// (erste) Payload zurück. Scheitert nach [`EVENT_WAIT_TIMEOUT`] mit der
/// Liste der stattdessen empfangenen Events.
///
/// Steht dieser Aufruf (wie in den Stopp-Tests) innerhalb eines äußeren
/// [`expect_event_within`] mit derselben Obergrenze, hängt es vom Timing
/// ab, welche der beiden Meldungen erscheint. Beide nennen ein erwartetes
/// Ereignis und listen alle empfangenen Events auf, die Diagnose ist also
/// in beiden Fällen vollständig.
pub(super) async fn wait_for_event(emitter: &TestEmitter, name: &str) -> serde_json::Value {
    expect_event_within(emitter, name, async {
        loop {
            let found = emitter
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|(event, _)| event == name)
                .map(|(_, payload)| payload.clone());
            if let Some(payload) = found {
                return payload;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
}

/// [`proposed_decision_code_with_emitter`] unter [`expect_event_within`]:
/// wartet auf das `chat-action-proposed`-Event der Aktion samt Abschluss
/// des Dialogs und listet im Fehlerfall die empfangenen Events auf.
/// `expected` beschreibt, was der Test an dieser Stelle erwartet.
pub(super) async fn expect_proposed_decision(
    session: &Session,
    action: AiAction,
    origin: ActionOrigin,
    expected: &str,
) -> (Decision, serde_json::Value) {
    let emitter = TestEmitter::default();
    expect_event_within(
        &emitter,
        &format!("chat-action-proposed — {expected}"),
        proposed_decision_code_with_emitter(session, action, origin, &emitter),
    )
    .await
}

/// Baut die Fehlermeldung: erwartetes Ereignis plus alle empfangenen
/// Events in Sende-Reihenfolge.
fn timeout_message(emitter: &TestEmitter, expected: &str) -> String {
    // `unwrap_or_else(into_inner)`: ist der Mutex vergiftet (ein anderer
    // Test-Task ist mit gehaltenem Lock gepanickt), soll trotzdem die
    // Liste erscheinen statt einer zweiten, nichtssagenden Panik.
    let events = emitter
        .events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut message = format!(
        "Zeitüberschreitung nach {} s beim Warten auf erwartetes Ereignis: {expected}\n\
         Empfangene Events ({}):",
        EVENT_WAIT_TIMEOUT.as_secs(),
        events.len()
    );
    if events.is_empty() {
        message.push_str("\n  (keine)");
    }
    for (index, (name, payload)) in events.iter().enumerate() {
        let mut rendered = payload.to_string();
        if rendered.chars().count() > MAX_PAYLOAD_CHARS {
            rendered = rendered.chars().take(MAX_PAYLOAD_CHARS).collect::<String>() + "…";
        }
        message.push_str(&format!("\n  {}. {name}: {rendered}", index + 1));
    }
    message
}

#[tokio::test]
async fn test_expect_event_within_returns_output_of_completed_future() {
    let emitter = TestEmitter::default();
    let value = expect_event_within(&emitter, "sofort fertig", async { 42 }).await;
    assert_eq!(value, 42);
}

#[tokio::test]
async fn test_wait_for_event_returns_payload_of_matching_event() {
    use crate::events::EventEmitter;
    let emitter = TestEmitter::default();
    emitter.emit_event("chat-text-delta", serde_json::json!({"text": "a"}));
    emitter.emit_event("chat-action-proposed", serde_json::json!({"actionId": "x"}));
    let payload = wait_for_event(&emitter, "chat-action-proposed").await;
    assert_eq!(payload["actionId"], "x");
}

/// Der Kern des Issues: die Meldung nennt das erwartete Ereignis und
/// listet die tatsächlich empfangenen Events auf. `start_paused`, damit
/// das Timeout ohne echte 5 s Wartezeit abläuft.
#[tokio::test(start_paused = true)]
async fn test_timeout_message_names_expected_event_and_lists_received_events() {
    use crate::events::EventEmitter;
    let emitter = TestEmitter::default();
    emitter.emit_event("chat-text-delta", serde_json::json!({"text": "hallo"}));
    emitter.emit_event("chat-response-cancelled", serde_json::json!({}));

    let outcome = tokio::spawn(async move {
        expect_event_within(
            &emitter,
            "chat-action-proposed",
            std::future::pending::<()>(),
        )
        .await;
    })
    .await;

    let panic = outcome.expect_err("das Warten muss am Timeout scheitern");
    let message = panic
        .into_panic()
        .downcast::<String>()
        .map(|message| *message)
        .expect("panic!-Meldung mit Format-Argumenten ist ein String");
    assert!(message.contains("chat-action-proposed"), "{message}");
    assert!(message.contains("Empfangene Events (2)"), "{message}");
    assert!(
        message.contains(r#"1. chat-text-delta: {"text":"hallo"}"#),
        "{message}"
    );
    assert!(
        message.contains("2. chat-response-cancelled: {}"),
        "{message}"
    );
}

#[test]
fn test_timeout_message_without_events_says_none_and_truncates_long_payloads() {
    use crate::events::EventEmitter;
    let empty = TestEmitter::default();
    assert!(timeout_message(&empty, "x").contains("(keine)"));

    let long = TestEmitter::default();
    long.emit_event("chat-text-delta", serde_json::json!("a".repeat(1000)));
    let message = timeout_message(&long, "x");
    assert!(message.ends_with('…'), "{message}");
    assert!(message.len() < 600, "{message}");
}
