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
    let provider = OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", true, Vec::new());

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
    let provider = OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", false, Vec::new());

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
    let provider = OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "test-key", false, Vec::new());

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
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(server.uri(), "gpt-test", "bad-key", true, Vec::new());

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::AuthenticationFailed)]);
}
