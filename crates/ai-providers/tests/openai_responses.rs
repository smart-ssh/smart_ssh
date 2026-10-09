//! Wiremock tests for `OpenAiResponsesProvider` (ADR 0124): request body
//! shape and stream parsing from recorded Responses API events.

use std::sync::Arc;

use ai_providers::{OpenAiResponsesProvider, ProviderBudgetGuard};
use futures::StreamExt;
use ssh_manager_core::ai::{
    default_action_schemas, AiError, AiEvent, AiProvider, ChatMessage, MessageContent, Role,
    SessionContext,
};
use ssh_manager_core::profiles::AiAction;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider(server: &MockServer, native: bool) -> OpenAiResponsesProvider {
    OpenAiResponsesProvider::new(
        server.uri(),
        "gpt-test",
        "key",
        native,
        Vec::new(),
        Arc::new(ProviderBudgetGuard::new()),
        None,
    )
}

fn context() -> SessionContext {
    SessionContext {
        system_context: "System.".to_string(),
        history: vec![
            ChatMessage {
                role: Role::User,
                content: MessageContent::Text("hi".to_string()),
            },
            ChatMessage {
                role: Role::Assistant,
                content: MessageContent::Text("hello".to_string()),
            },
        ],
        available_actions: default_action_schemas(),
        max_tokens_hint: None,
    }
}

fn sse(events: &[(&str, &str)]) -> String {
    events
        .iter()
        .map(|(kind, data)| format!("event: {kind}\ndata: {data}\n\n"))
        .collect()
}

async fn serve(body: String) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;
    server
}

async fn collect(provider: &OpenAiResponsesProvider) -> Vec<AiEvent> {
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        provider.send(context()).collect::<Vec<_>>(),
    )
    .await
    .expect("stream must finish")
}

async fn sent_bodies(server: &MockServer) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

const COMPLETED: (&str, &str) = (
    "response.completed",
    r#"{"type":"response.completed","response":{"status":"completed"}}"#,
);

#[tokio::test]
async fn request_body_with_native_tools_is_stateless() {
    let server = serve(sse(&[COMPLETED])).await;
    collect(&provider(&server, true)).await;
    let body = &sent_bodies(&server).await[0];
    assert_eq!(body["store"], false);
    assert!(body.get("previous_response_id").is_none());
    assert!(body.get("messages").is_none());
    assert_eq!(body["stream"], true);
    assert_eq!(body["instructions"], "System.");
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][1]["content"], "hello");
    assert!(body["max_output_tokens"].as_u64().unwrap() > 0);
    let tool = &body["tools"][0];
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["strict"], false);
    assert!(tool["name"].is_string() && tool["parameters"].is_object());
    assert!(tool.get("function").is_none());
}

#[tokio::test]
async fn request_body_without_native_tools_has_no_tools_and_fallback_prompt() {
    let server = serve(sse(&[COMPLETED])).await;
    collect(&provider(&server, false)).await;
    let body = &sent_bodies(&server).await[0];
    assert!(body.get("tools").is_none());
    assert_eq!(body["store"], false);
    assert!(body["instructions"].as_str().unwrap().len() > "System.".len());
}

#[tokio::test]
async fn native_function_call_yields_action_proposed() {
    let body = sse(&[
        (
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"Sure,"}"#,
        ),
        (
            "response.output_item.added",
            r#"{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"c1","name":"suggest_command","arguments":""}}"#,
        ),
        (
            "response.function_call_arguments.delta",
            r#"{"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"command\": \"ls"}"#,
        ),
        (
            "response.function_call_arguments.delta",
            r#"{"type":"response.function_call_arguments.delta","output_index":1,"delta":" -la\"}"}"#,
        ),
        COMPLETED,
    ]);
    let server = serve(body).await;
    let events = collect(&provider(&server, true)).await;
    assert_eq!(events[0], AiEvent::TextDelta("Sure,".to_string()));
    match &events[1] {
        AiEvent::ActionProposed(AiAction::SuggestCommand { command, .. }) => {
            assert_eq!(command, "ls -la")
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(events.last(), Some(&AiEvent::Done));
}

#[tokio::test]
async fn truncated_function_call_is_never_released() {
    // Incomplete (max_output_tokens) with a half-received call: the first
    // request is retried once with a higher budget; if the retry is also
    // incomplete, no action is proposed.
    let body = sse(&[
        (
            "response.output_item.added",
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","name":"suggest_command","arguments":""}}"#,
        ),
        (
            "response.function_call_arguments.delta",
            r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"command\": \"rm -r"}"#,
        ),
        (
            "response.incomplete",
            r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
        ),
    ]);
    let server = serve(body).await;
    let events = collect(&provider(&server, true)).await;
    assert!(events
        .iter()
        .all(|e| !matches!(e, AiEvent::ActionProposed(_))));
    assert_eq!(
        events.last(),
        Some(&AiEvent::Error(AiError::ResponseTruncated))
    );
    let bodies = sent_bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    assert!(bodies[1]["max_output_tokens"].as_u64() > bodies[0]["max_output_tokens"].as_u64());
}

#[tokio::test]
async fn stream_ending_without_terminal_event_discards_function_call() {
    let body = sse(&[
        (
            "response.output_item.added",
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","name":"suggest_command","arguments":""}}"#,
        ),
        (
            "response.function_call_arguments.delta",
            r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"command\": \"ls\"}"}"#,
        ),
    ]);
    let server = serve(body).await;
    let events = collect(&provider(&server, true)).await;
    assert!(events
        .iter()
        .all(|e| !matches!(e, AiEvent::ActionProposed(_))));
}

#[tokio::test]
async fn text_truncation_ends_with_text_truncated() {
    let body = sse(&[
        (
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"partial"}"#,
        ),
        (
            "response.incomplete",
            r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
        ),
    ]);
    let server = serve(body).await;
    let events = collect(&provider(&server, true)).await;
    assert_eq!(
        events,
        vec![
            AiEvent::TextDelta("partial".to_string()),
            AiEvent::TextTruncated
        ]
    );
}

#[tokio::test]
async fn failed_response_and_error_event_map_to_errors() {
    let body = sse(&[(
        "response.failed",
        r#"{"type":"response.failed","response":{"error":{"code":"rate_limit_exceeded","message":"slow down"}}}"#,
    )]);
    let server = serve(body).await;
    let events = collect(&provider(&server, true)).await;
    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);

    let body = sse(&[(
        "error",
        r#"{"type":"error","code":"server_error","message":"boom"}"#,
    )]);
    let server = serve(body).await;
    let events = collect(&provider(&server, true)).await;
    assert!(matches!(
        events.as_slice(),
        [AiEvent::Error(AiError::ProviderUnavailable(m))] if m == "boom"
    ));
}

#[tokio::test]
async fn http_errors_use_the_shared_mapping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(401).set_body_string("{}"))
        .mount(&server)
        .await;
    let events = collect(&provider(&server, true)).await;
    assert_eq!(events, vec![AiEvent::Error(AiError::AuthenticationFailed)]);
}

// --- Issue #168: web research via the Responses `web_search` tool ---------

fn web_provider(server: &MockServer, enabled: bool) -> OpenAiResponsesProvider {
    provider(server, true).with_web_research(enabled)
}

fn tool_types(body: &serde_json::Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|t| t["type"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn web_search_tool_is_first_and_limited_when_enabled() {
    let server = serve(sse(&[COMPLETED])).await;
    collect(&web_provider(&server, true)).await;
    let body = &sent_bodies(&server).await[0];
    let types = tool_types(body);
    assert_eq!(types[0], "web_search");
    assert!(types[1..].iter().all(|t| t == "function"), "{types:?}");
    assert_eq!(body["max_tool_calls"], 5);
    assert_eq!(body["store"], false);
    assert!(body.get("web_search_options").is_none());
}

#[tokio::test]
async fn no_web_field_when_switched_off() {
    let server = serve(sse(&[COMPLETED])).await;
    collect(&web_provider(&server, false)).await;
    let body = &sent_bodies(&server).await[0];
    assert!(!tool_types(body).iter().any(|t| t == "web_search"));
    assert!(body.get("max_tool_calls").is_none());
    assert!(!body.to_string().contains("web_search"), "{body}");
}

#[tokio::test]
async fn side_calls_never_get_the_web_tool() {
    let server = serve(sse(&[COMPLETED])).await;
    let mut ctx = context();
    ctx.max_tokens_hint = Some(300);
    let _ = web_provider(&server, true)
        .send(ctx)
        .collect::<Vec<_>>()
        .await;
    let body = &sent_bodies(&server).await[0];
    assert!(!body.to_string().contains("web_search"), "{body}");
    assert!(body.get("max_tool_calls").is_none());
}

#[tokio::test]
async fn web_search_call_and_citations_become_one_search_activity() {
    let server = serve(sse(&[
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","id":"ws_1","status":"completed","action":{"type":"search","query":"nginx 1.29 changes"}}}"#,
        ),
        (
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"See the changelog."}"#,
        ),
        (
            "response.output_text.annotation.added",
            r#"{"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://nginx.org/en/CHANGES","title":"nginx changes","start_index":0,"end_index":4}}"#,
        ),
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"message","content":[{"type":"output_text","text":"x","annotations":[{"type":"url_citation","url":"https://nginx.org/en/CHANGES","title":"nginx changes"}]}]}}"#,
        ),
        COMPLETED,
    ]))
    .await;
    let events = collect(&web_provider(&server, true)).await;
    let activities: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AiEvent::WebActivity(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(activities.len(), 1, "{events:?}");
    let a = activities[0];
    assert_eq!(a.kind, ssh_manager_core::ai::WebActivityKind::Search);
    assert_eq!(a.input, "nginx 1.29 changes");
    assert_eq!(a.cited.len(), 1, "citations are deduplicated");
    assert_eq!(a.cited[0].url, "https://nginx.org/en/CHANGES");
    assert_eq!(a.results, a.cited);
    assert!(a.content.is_none());
    assert!(a.error_code.is_none());
    // Plain information: no action proposal.
    assert!(!events
        .iter()
        .any(|e| matches!(e, AiEvent::ActionProposed(_))));
}

#[tokio::test]
async fn open_page_action_counts_as_activity_and_failed_search_carries_a_code() {
    let server = serve(sse(&[
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","status":"completed","action":{"type":"open_page","url":"https://example.com/x"}}}"#,
        ),
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"web_search_call","status":"failed","action":{"type":"search","query":"q"}}}"#,
        ),
        COMPLETED,
    ]))
    .await;
    let events = collect(&web_provider(&server, true)).await;
    let activities: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AiEvent::WebActivity(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(activities.len(), 2, "{events:?}");
    assert_eq!(activities[0].input, "https://example.com/x");
    assert!(activities[0].content.is_none());
    assert_eq!(activities[1].input, "q");
    assert_eq!(activities[1].error_code.as_deref(), Some("search_failed"));
}

#[tokio::test]
async fn open_page_only_response_still_emits_an_activity() {
    let server = serve(sse(&[
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","status":"completed","action":{"type":"open_page","url":"https://example.com/x"}}}"#,
        ),
        COMPLETED,
    ]))
    .await;
    let events = collect(&web_provider(&server, true)).await;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, AiEvent::WebActivity(_)))
            .count(),
        1,
        "{events:?}"
    );
}

#[tokio::test]
async fn citation_without_search_call_still_emits_an_activity() {
    let server = serve(sse(&[
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","content":[{"type":"output_text","text":"x","annotations":[{"type":"url_citation","url":"https://example.com/a","title":"A"}]}]}}"#,
        ),
        COMPLETED,
    ]))
    .await;
    let events = collect(&web_provider(&server, true)).await;
    let acts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AiEvent::WebActivity(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(acts.len(), 1, "{events:?}");
    assert_eq!(acts[0].cited.len(), 1);
}

#[tokio::test]
async fn failed_response_releases_no_web_activity() {
    let server = serve(sse(&[
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","status":"completed","action":{"type":"search","query":"q"}}}"#,
        ),
        (
            "response.failed",
            r#"{"type":"response.failed","response":{"error":{"code":"server_error","message":"boom"}}}"#,
        ),
    ]))
    .await;
    let events = collect(&web_provider(&server, true)).await;
    assert!(events.iter().any(|e| matches!(e, AiEvent::Error(_))));
    assert!(!events.iter().any(|e| matches!(e, AiEvent::WebActivity(_))));
}
