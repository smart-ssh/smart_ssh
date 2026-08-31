//! Wiremock-basierte Tests für `AnthropicProvider` (Spec 0006, Abschnitt 7,
//! zweiter Block).

use ai_providers::AnthropicProvider;
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
event: message_stop\ndata: {}\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = AnthropicProvider::new(server.uri(), "claude-test", "test-key", true);

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
event: message_stop\ndata: {}\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = AnthropicProvider::new(server.uri(), "claude-test", "test-key", false);

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
    let provider = AnthropicProvider::new(server.uri(), "claude-test", "test-key", false);

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta(full_text.to_string()), AiEvent::Done]
    );
}

#[tokio::test]
async fn test_authentication_failure_maps_401_to_ai_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new(server.uri(), "claude-test", "bad-key", true);

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::AuthenticationFailed)]);
}
