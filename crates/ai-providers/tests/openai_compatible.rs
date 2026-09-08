//! Wiremock-basierte Tests für `OpenAiCompatibleProvider` (Spec 0006,
//! Abschnitt 7, zweiter Block).

use ai_providers::OpenAiCompatibleProvider;
use futures::StreamExt;
use ssh_manager_core::ai::{default_action_schemas, AiError, AiEvent, AiProvider, SessionContext};
use ssh_manager_core::profiles::AiAction;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn empty_context() -> SessionContext {
    SessionContext {
        system_context: "Testkontext".to_string(),
        history: Vec::new(),
        available_actions: default_action_schemas(),
    }
}

async fn mock_server_with_sse_body(sse_body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
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
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"Klar,\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"suggest_command\",\"arguments\":\"\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\": \\\"ls -la\\\"}\"}}]}}]}\n\n\
data: [DONE]\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", true, Vec::new());

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
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"Sicher. \"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"<!--ACTION-->{\\\"action\\\": \\\"suggest_command\\\", \\\"parameters\\\": {\\\"command\\\": \\\"df -h\\\"}}<!--/ACTION-->\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" Fertig.\"}}]}\n\n\
data: [DONE]\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", false, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    // Im Fallback-Modus wird nichts inkrementell gestreamt (s. Kommentar in
    // fallback.rs) — genau ein TextDelta mit dem bereinigten Text, dann die
    // Aktion, dann Done.
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
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(full_text).unwrap()
    );
    let server = mock_server_with_sse_body(&sse_body).await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", false, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta(full_text.to_string()), AiEvent::Done]
    );
}

#[tokio::test]
async fn test_authentication_failure_maps_401_to_ai_error() {
    let server = MockServer::start().await;
    // `.expect(1)`: s. identischer Kommentar in
    // `tests/anthropic.rs::test_authentication_failure_maps_401_to_ai_error`.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .expect(1)
        .mount(&server)
        .await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "bad-key", true, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::AuthenticationFailed)]);
}

/// Spec 0051, Teil 1: s. identischer Kommentar in
/// `tests/anthropic.rs::test_429_with_retry_after_retries_and_then_succeeds`
/// — gilt "für alle 429-fähigen KI-Requests", also auch für den
/// OpenAI-kompatiblen Provider.
#[tokio::test]
async fn test_429_with_retry_after_retries_and_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body.to_string()),
        )
        .with_priority(2)
        .mount(&server)
        .await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", true, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0051, Teil 1: s. identischer Kommentar in
/// `tests/anthropic.rs::test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error`.
#[tokio::test]
async fn test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error() {
    let server = MockServer::start().await;
    // `.expect(4)`: s. identischer Kommentar in
    // `tests/anthropic.rs::test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error`
    // zur Begründung, warum die reine Endzustands-Assertion allein
    // tautologisch wäre.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .expect(4)
        .mount(&server)
        .await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", true, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}

/// Spec-Reviewer-Fund: s. identischer Kommentar in
/// `tests/anthropic.rs::test_retry_after_longer_than_total_budget_gives_up_without_extra_request`.
#[tokio::test]
async fn test_retry_after_longer_than_total_budget_gives_up_without_extra_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "3600")
                .set_body_string("rate limited"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider =
        OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", true, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}
