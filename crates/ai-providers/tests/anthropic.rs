//! Wiremock-basierte Tests für `AnthropicProvider` (Spec 0006, Abschnitt 7,
//! zweiter Block).

use std::sync::Arc;

use ai_providers::{AnthropicProvider, ProviderBudgetGuard};
use futures::StreamExt;
use ssh_manager_core::ai::{default_action_schemas, AiError, AiEvent, AiProvider, SessionContext};
use ssh_manager_core::profiles::AiAction;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Spec 0061: jeder Test baut seinen eigenen, unabhängigen Wächter — kein
/// geteilter Zustand zwischen Tests nötig, die Identität/Registry-Logik
/// wird eigenständig in `rate_limit_budget`s Unit-Tests geprüft.
fn test_budget() -> Arc<ProviderBudgetGuard> {
    Arc::new(ProviderBudgetGuard::new())
}

fn empty_context() -> SessionContext {
    SessionContext {
        system_context: "Testkontext".to_string(),
        history: Vec::new(),
        available_actions: default_action_schemas(),
        max_tokens_hint: None,
    }
}

async fn mock_server_with_sse_body(sse_body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body.to_string()),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn test_native_tool_calling_success_yields_action_proposed() {
    let sse_body = "\
event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Klar,\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: content_block_start\ndata: {\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"suggest_command\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\": \\\"ls -la\\\"}\"}}\n\n\
event: content_block_stop\ndata: {\"index\":1}\n\n\
event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n\
event: message_stop\ndata: {}\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events[0], AiEvent::TextDelta("Klar,".to_string()));
    assert_eq!(
        events[1],
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: "ls -la".to_string()
        })
    );
    assert_eq!(events[2], AiEvent::Done);
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn test_fallback_mode_parses_action_block_after_stream_completes() {
    let sse_body = "\
event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Sicher. \"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"<!--ACTION-->{\\\"action\\\": \\\"suggest_command\\\", \\\"parameters\\\": {\\\"command\\\": \\\"df -h\\\"}}<!--/ACTION-->\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" Fertig.\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
event: message_stop\ndata: {}\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        false,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events.len(), 3);
    let AiEvent::TextDelta(text) = &events[0] else {
        panic!("erwartetes TextDelta, bekam {:?}", events[0]);
    };
    assert!(!text.contains("<!--ACTION-->"));
    assert!(text.contains("Sicher."));
    assert!(text.contains("Fertig."));
    assert_eq!(
        events[1],
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: "df -h".to_string()
        })
    );
    assert_eq!(events[2], AiEvent::Done);
}

#[tokio::test]
async fn test_fallback_mode_treats_malformed_action_block_as_plain_text() {
    let full_text = "Text vor <!--ACTION-->{invalid<!--/ACTION--> Text danach";
    let sse_body = format!(
        "event: content_block_start\ndata: {{\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
event: content_block_delta\ndata: {{\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{}}}}}\n\n\
event: content_block_stop\ndata: {{\"index\":0}}\n\n\
event: message_stop\ndata: {{}}\n\n",
        serde_json::to_string(full_text).unwrap()
    );
    let server = mock_server_with_sse_body(&sse_body).await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        false,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta(full_text.to_string()), AiEvent::Done]
    );
}

#[tokio::test]
async fn test_authentication_failure_maps_401_to_ai_error() {
    let server = MockServer::start().await;
    // Spec-Reviewer-Fund (Spec 0051, Review dieses Schritts): `.expect(1)`
    // beweist, dass nur 429 automatisch wiederholt wird — ohne diese
    // Zählung würde eine Regression zu "jeder Nicht-Erfolgs-Status wird
    // wiederholt" unbemerkt bleiben (die reine Endzustands-Assertion unten
    // wäre in beiden Fällen identisch grün).
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .expect(1)
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "bad-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::AuthenticationFailed)]);
}

/// Spec 0051, Teil 1: ein 429 mit `Retry-After` wird automatisch wiederholt
/// und, sobald der Provider wieder antwortet, kehrt `send()` zu einem
/// normalen erfolgreichen Stream zurück — kein Abbruch, kein Hängen. Der
/// Wechsel von "immer 429" auf "danach 200" wird über die
/// Priorität+`up_to_n_times`-Kombination von wiremock simuliert (s.
/// `Mock::with_priority`-Doku: die höher priorisierte, `up_to_n_times(1)`
/// begrenzte 429-Mock hört nach dem ersten Treffer auf zu matchen, danach
/// greift die niedriger priorisierte 200-Mock).
#[tokio::test]
async fn test_429_with_retry_after_retries_and_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    let sse_body = "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: message_stop\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body.to_string()),
        )
        .with_priority(2)
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0051, Teil 1: bleibt der Provider dauerhaft bei 429, gibt `send()`
/// nach der harten Obergrenze an Versuchen auf — statt endlos zu warten
/// oder zu hängen — und liefert `AiEvent::Error(AiError::RateLimited)`.
/// `Retry-After: 0` hält den Test schnell, ohne die Backoff-Logik selbst
/// zu berühren (die ist in `crate::retry` isoliert unit-getestet).
#[tokio::test]
async fn test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error() {
    let server = MockServer::start().await;
    // `.expect(4)` (nicht nur der Endzustand `Error(RateLimited)`, den auch
    // der ungefixte Stand ohne jeden Retry sofort liefern würde) ist hier
    // der eigentliche Regressionstest: `MockServer` panickt beim Shutdown,
    // falls nicht exakt `crate::retry::MAX_ATTEMPTS` (4) Requests
    // ankamen — ohne diese Zählung würde dieser Test tautologisch
    // sowohl gegen den gefixten als auch den ungefixten Stand grün sein.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .expect(4)
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}

/// Spec-Reviewer-Fund (Spec 0051, Review dieses Schritts): ein
/// `Retry-After`, das länger ist als das verbleibende Gesamtzeit-Budget,
/// muss sofort aufgeben statt trotzdem noch einen (zu frühen) Request zu
/// schicken — `.expect(1)` beweist, dass tatsächlich nur der Erstversuch
/// stattfindet, nicht erst nach einem (in einem echten Test unpraktikablen)
/// stundenlangen Warten.
#[tokio::test]
async fn test_retry_after_longer_than_total_budget_gives_up_without_extra_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "3600")
                .set_body_string("rate limited"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}

/// Spec 0061, Abschnitt 1: die `anthropic-ratelimit-*`-Header werden auf
/// einer ERFOLGREICHEN Antwort gelesen — Integrationstest über den echten
/// `AnthropicProvider`, nicht nur den isolierten Header-Parser (s.
/// `rate_limit_budget`-Unit-Tests). Der Test hält denselben `Arc`, den der
/// Provider intern aktualisiert, und prüft danach dessen Zustand über das
/// öffentliche `wait_duration`-Verhalten (kein direkter Feldzugriff nötig).
#[tokio::test]
async fn test_rate_limit_headers_are_recorded_from_a_successful_response() {
    let server = MockServer::start().await;
    let sse_body = "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: message_stop\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .insert_header("anthropic-ratelimit-input-tokens-limit", "1000")
                .insert_header("anthropic-ratelimit-input-tokens-remaining", "50")
                .set_body_string(sse_body.to_string()),
        )
        .mount(&server)
        .await;
    let budget = test_budget();
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        budget.clone(),
        None,
    );

    let _events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert!(
        budget.wait_duration(0).is_some(),
        "5% Restbudget aus den gelesenen Headern muss den Wächter zum Warten veranlassen"
    );
}

/// Spec 0061, Abschnitt 1: dieselben Header werden AUCH auf einer 429-
/// Antwort gelesen — genau der Fall, den `map_http_status` bisher (vor
/// Spec 0061) unwiederbringlich verworfen hätte.
#[tokio::test]
async fn test_rate_limit_headers_are_recorded_from_a_429_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .insert_header("anthropic-ratelimit-requests-limit", "50")
                .insert_header("anthropic-ratelimit-requests-remaining", "0")
                .set_body_string("rate limited"),
        )
        .expect(4)
        .mount(&server)
        .await;
    let budget = test_budget();
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        budget.clone(),
        None,
    );

    let _events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert!(
        budget.wait_duration(0).is_some(),
        "0 verbleibende Requests aus einer 429-Antwort müssen den Wächter zum Warten veranlassen"
    );
}

/// Spec 0065, Teil 3 (spec-reviewer-Fund, Testbarkeit-Lücke, ERHÖHT):
/// Ende-zu-Ende-Test über den echten `send()`-Retry-Mechanismus (nicht nur
/// `process_frame_stream` isoliert) — ein abgeschnittener Tool-Call löst
/// GENAU EINEN zweiten HTTP-Request aus, der (bei Erfolg) normal
/// durchgereicht wird. `.expect(2)` beweist, dass wirklich ein zweiter
/// Request rausging (nicht nur derselbe Body erneut lokal verarbeitet).
#[tokio::test]
async fn test_truncated_tool_call_triggers_exactly_one_retry_request_that_then_succeeds() {
    let server = MockServer::start().await;
    let truncated_body = "\
event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"suggest_command\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\": \\\"rm -rf /var/log/app\\\"}\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n\
event: message_stop\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(truncated_body.to_string()),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    let complete_body = "\
event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"suggest_command\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\": \\\"ls -la\\\"}\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n\
event: message_stop\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(complete_body.to_string()),
        )
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    // `.expect(1)` auf beiden Mocks (per Drop-Assertion von wiremock) prüft
    // bereits die Request-Anzahl — hier zusätzlich das sichtbare Ergebnis:
    // der ERSTE (abgeschnittene) Tool-Call taucht NIRGENDS auf, nur der
    // zweite, vollständige.
    assert_eq!(
        events,
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string()
            }),
            AiEvent::Done
        ]
    );
}

/// Gegenprobe: scheitert auch der Retry (wieder `max_tokens`), gibt es
/// GENAU ZWEI Requests insgesamt (kein dritter, keine Schleife) und einen
/// sichtbaren `AiError::ResponseTruncated` statt einer Ausführung.
#[tokio::test]
async fn test_truncated_tool_call_that_fails_twice_yields_response_truncated_error_no_third_request(
) {
    let server = MockServer::start().await;
    let truncated_body = "\
event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"suggest_command\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\": \\\"rm -rf /var/log/app\\\"}\"}}\n\n\
event: content_block_stop\ndata: {\"index\":0}\n\n\
event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n\
event: message_stop\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(truncated_body.to_string()),
        )
        .expect(2)
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(
        server.uri(),
        "claude-test",
        "test-key",
        true,
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::ResponseTruncated)]);
}
