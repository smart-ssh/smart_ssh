//! [`AiProvider`] for the official OpenAI API via the Responses API
//! (`POST /responses`, ADR 0124).
//!
//! Kept next to `openai_compatible` instead of branching inside it: custom
//! base URLs, generic OpenAI-compatible endpoints and Ollama keep sending the
//! unchanged Chat Completions body. Every request carries `store: false` and
//! never a `previous_response_id`; the full history is sent each time, so
//! OpenAI keeps no conversation state and behaviour stays
//! provider-independent.
//!
//! Stream format: SSE frames whose JSON `type` names the event
//! (`response.output_text.delta`, `response.output_item.added|done`,
//! `response.function_call_arguments.delta`, `response.completed`,
//! `response.incomplete`, `response.failed`, `error`). There is no `[DONE]`
//! literal; a stream that ends without a terminal event is abrupt, and an
//! accumulated tool call is then discarded exactly as in the Chat
//! Completions path (the shared `OpenAiStreamState::finalize` is reused with
//! the terminal status mapped to its `finish_reason` vocabulary).

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;

use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use uuid::Uuid;

use ssh_manager_core::ai::{
    AiError, AiEvent, AiProvider, SessionContext, WebActivity, WebActivityKind, WebSource,
};

use crate::action::parameters_json_schema;
use crate::error::{map_transport_error, timeout_error};
use crate::fallback::fallback_system_prompt_addition;
use crate::openai_compatible::{
    message_content_text, openai_compatible_default_max_tokens,
    openai_compatible_model_max_output_tokens, retry_stream, role_str, OpenAiStreamState, RawEvent,
    RetryKind, RetryState,
};
use crate::request_logging::{log_outgoing_context, log_provider_transport_error};
use crate::sse::{build_http_client, sse_frame_stream, SseFrame, SSE_INACTIVITY_TIMEOUT};

const MAX_TOKENS_FIELD: &str = "max_output_tokens";

/// Issue #168 (Spec 0105 §3): upper bound of built-in tool calls (web
/// searches) per request, sent as `max_tool_calls`. Function tools are not
/// counted by the provider, so this is the search limit.
const WEB_TOOL_MAX_CALLS: u32 = 5;

/// The provider-side web search tool. Position: always first in the tool
/// list, so the tool set depends only on the provider and the setting.
fn web_tool_definition() -> Value {
    json!({"type": "web_search"})
}

pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    supports_native_tool_calling: bool,
    extra_headers: Vec<(String, String)>,
    #[allow(dead_code)]
    budget: std::sync::Arc<crate::rate_limit_budget::ProviderBudgetGuard>,
    max_tokens_override: Option<u32>,
    /// Issue #168: offer the provider's `web_search` tool in the main chat.
    web_research: bool,
}

impl OpenAiResponsesProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        supports_native_tool_calling: bool,
        extra_headers: Vec<(String, String)>,
        budget: std::sync::Arc<crate::rate_limit_budget::ProviderBudgetGuard>,
        max_tokens_override: Option<u32>,
    ) -> Self {
        Self {
            client: build_http_client(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
            supports_native_tool_calling,
            extra_headers,
            budget,
            max_tokens_override,
            web_research: false,
        }
    }

    /// Issue #168: switches the provider's `web_search` tool on or off for
    /// the main chat (provider setting). The constructor leaves it off so
    /// every caller enables it deliberately.
    #[must_use]
    pub fn with_web_research(mut self, enabled: bool) -> Self {
        self.web_research = enabled;
        self
    }

    pub(crate) fn build_request_body(&self, context: &SessionContext) -> Value {
        let mut instructions = context.system_context.clone();
        if !self.supports_native_tool_calling {
            instructions.push_str(&fallback_system_prompt_addition(&context.available_actions));
        }
        let input: Vec<Value> = context
            .history
            .iter()
            .map(|message| {
                json!({
                    "role": role_str(message.role),
                    "content": message_content_text(&message.content),
                })
            })
            .collect();
        let max_tokens = context
            .max_tokens_hint
            .or(self.max_tokens_override)
            .unwrap_or_else(|| openai_compatible_default_max_tokens(&self.base_url, &self.model));

        let mut body = json!({
            "model": self.model,
            "input": input,
            "stream": true,
            "store": false,
        });
        if !instructions.trim().is_empty() {
            body["instructions"] = json!(instructions);
        }
        body[MAX_TOKENS_FIELD] = json!(max_tokens);
        if self.supports_native_tool_calling && !context.available_actions.is_empty() {
            // Web search only for the main chat: every side call sets
            // `max_tokens_hint`, the main chat never does (as on the
            // Anthropic path).
            let mut tools: Vec<Value> = Vec::new();
            if self.web_research && context.max_tokens_hint.is_none() {
                tools.push(web_tool_definition());
                body["max_tool_calls"] = json!(WEB_TOOL_MAX_CALLS);
            }
            tools.extend(context.available_actions.iter().map(|action| {
                json!({
                    "type": "function",
                    "name": action.name,
                    "description": action.description,
                    "parameters": parameters_json_schema(action),
                    // Responses defaults to strict schemas; the
                    // Chat Completions path never used them.
                    "strict": false,
                })
            }));
            body["tools"] = Value::Array(tools);
        }
        body
    }
}

impl AiProvider for OpenAiResponsesProvider {
    fn send(&self, context: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
        let request_id = Uuid::new_v4();
        log_outgoing_context(request_id, &context);

        let max_tokens_is_user_override =
            context.max_tokens_hint.is_none() && self.max_tokens_override.is_some();
        let body = self.build_request_body(&context);
        let model_max_tokens =
            openai_compatible_model_max_output_tokens(&self.base_url, &self.model);
        let max_tokens = body[MAX_TOKENS_FIELD]
            .as_u64()
            .unwrap_or(u64::from(model_max_tokens)) as u32;

        retry_stream(RetryState {
            parse: event_stream_from_response,
            client: self.client.clone(),
            url: format!("{}/responses", self.base_url.trim_end_matches('/')),
            api_key: self.api_key.clone(),
            native_tool_calling: self.supports_native_tool_calling,
            request_id,
            extra_headers: self.extra_headers.clone(),
            body,
            max_tokens,
            model_max_tokens,
            max_tokens_field: MAX_TOKENS_FIELD,
            max_tokens_is_user_override,
            retry_used: RetryKind::None,
            inner: None,
            finished: false,
        })
    }
}

fn event_stream_from_response(
    response: reqwest::Response,
    native_tool_calling: bool,
    request_id: Uuid,
    api_key: String,
    extra_headers: Vec<(String, String)>,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>> {
    process_frame_stream(
        Box::pin(sse_frame_stream(response)),
        native_tool_calling,
        request_id,
        api_key,
        extra_headers,
    )
}

/// Web research of one response, held until its end (ADR 0117 point 10).
///
/// The Responses API returns no page text and no result list for a search
/// (only the query and `url_citation` annotations on the answer), so a
/// search is shown with its query and the answer's cited sources; there are
/// no "Webpage read" activities. Citation titles are stored as `results`
/// too, so the injection check sees them.
#[derive(Default)]
struct WebCollector {
    searches: Vec<WebActivity>,
    citations: Vec<WebSource>,
}

impl WebCollector {
    fn record_call(&mut self, item: &Value) {
        let action = item.get("action");
        // `open_page` / `find_in_page` carry no page text the app could
        // store, fence or check. They still count as web activity (the
        // model read untrusted content), so they are shown as a search
        // card with the page URL and trigger the escalation (Spec 0105 §4/§6).
        let is_page_action = matches!(
            action.and_then(|a| a.get("type")).and_then(Value::as_str),
            Some("open_page") | Some("find_in_page")
        );
        let query = if is_page_action {
            action
                .and_then(|a| a.get("url"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        } else {
            action
                .and_then(|a| a.get("query"))
                .and_then(Value::as_str)
                .or_else(|| {
                    action
                        .and_then(|a| a.get("queries"))
                        .and_then(|q| q.get(0))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default()
                .to_string()
        };
        let failed = item.get("status").and_then(Value::as_str) == Some("failed");
        self.searches.push(WebActivity {
            kind: WebActivityKind::Search,
            input: query,
            results: Vec::new(),
            cited: Vec::new(),
            content: None,
            content_truncated: false,
            error_code: failed.then(|| "search_failed".to_string()),
        });
    }

    fn record_annotation(&mut self, annotation: &Value) {
        if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
            return;
        }
        let Some(url) = annotation.get("url").and_then(Value::as_str) else {
            return;
        };
        let title = annotation
            .get("title")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .unwrap_or(url);
        let source = WebSource {
            title: title.to_string(),
            url: url.to_string(),
        };
        if !self.citations.contains(&source) {
            self.citations.push(source);
        }
    }

    fn record_message_item(&mut self, item: &Value) {
        let Some(parts) = item.get("content").and_then(Value::as_array) else {
            return;
        };
        for part in parts {
            if let Some(annotations) = part.get("annotations").and_then(Value::as_array) {
                for annotation in annotations {
                    self.record_annotation(annotation);
                }
            }
        }
    }

    /// Hands out the activities. The answer's citations cannot be tied to
    /// one search, so they go onto the last search of the response.
    fn take(&mut self) -> Vec<WebActivity> {
        let citations = std::mem::take(&mut self.citations);
        let mut searches = std::mem::take(&mut self.searches);
        if let Some(last) = searches.last_mut() {
            last.results = citations.clone();
            last.cited = citations;
        } else if !citations.is_empty() {
            // Cited sources without a recorded search call: the answer still
            // rests on web content, so it must not go unflagged.
            searches.push(WebActivity {
                kind: WebActivityKind::Search,
                input: String::new(),
                results: citations.clone(),
                cited: citations,
                content: None,
                content_truncated: false,
                error_code: None,
            });
        }
        searches
    }
}

/// Maps a `response.failed` / `error` event to a visible error.
fn map_stream_error(code: Option<&str>, message: &str) -> AiError {
    match code {
        Some("rate_limit_exceeded") | Some("insufficient_quota") => AiError::RateLimited,
        Some("context_length_exceeded") => AiError::ContextTooLarge,
        Some("invalid_api_key") => AiError::AuthenticationFailed,
        Some("model_not_found") => AiError::ModelNotFound(message.to_string()),
        _ => AiError::ProviderUnavailable(message.to_string()),
    }
}

/// Feeds one decoded event into `state`. Returns `Some(terminal)` when the
/// event ends the response: `Ok(())` after completed/incomplete (finalize
/// next), `Err(error)` after a failure (no finalize, no tool call released).
fn handle_event(
    state: &mut OpenAiStreamState,
    web: &mut WebCollector,
    event: &Value,
) -> Option<Result<(), AiError>> {
    let kind = event.get("type").and_then(Value::as_str)?;
    match kind {
        "response.output_text.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    state.text_delta_total_len += delta.len();
                    if state.native_tool_calling {
                        state
                            .pending
                            .push_back(RawEvent::Public(AiEvent::TextDelta(delta.to_string())));
                    } else {
                        state.fallback_text.push_str(delta);
                    }
                }
            }
        }
        // Reasoning summaries are only counted, never shown or parsed as
        // fallback actions (same rule as Chat Completions reasoning deltas).
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                state.reasoning_delta_total_len += delta.len();
            }
        }
        "response.output_text.annotation.added" => {
            if let Some(annotation) = event.get("annotation") {
                web.record_annotation(annotation);
            }
        }
        "response.output_item.done"
            if event.pointer("/item/type").and_then(Value::as_str) == Some("web_search_call") =>
        {
            web.record_call(event.get("item")?);
        }
        "response.output_item.done"
            if event.pointer("/item/type").and_then(Value::as_str) == Some("message") =>
        {
            web.record_message_item(event.get("item")?);
        }
        "response.output_item.added" | "response.output_item.done" if state.native_tool_calling => {
            let item = event.get("item")?;
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let index = event
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let entry = state.tool_calls.entry(index).or_default();
                if let Some(name) = item.get("name").and_then(Value::as_str) {
                    entry.name = name.to_string();
                }
                // The finished item carries the authoritative arguments.
                if kind == "response.output_item.done" {
                    if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
                        entry.arguments = arguments.to_string();
                    }
                }
            }
        }
        "response.function_call_arguments.delta" if state.native_tool_calling => {
            let index = event
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                state
                    .tool_calls
                    .entry(index)
                    .or_default()
                    .arguments
                    .push_str(delta);
            }
        }
        "response.completed" => {
            state.finish_reason = Some(
                if state.tool_calls.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                }
                .to_string(),
            );
            return Some(Ok(()));
        }
        "response.incomplete" => {
            let reason = event
                .pointer("/response/incomplete_details/reason")
                .and_then(Value::as_str)
                .unwrap_or("incomplete");
            // `max_output_tokens` is this API's `length`; any other reason
            // is not a confirmed completion either (allowlist, see
            // `OpenAiStreamState::finalize`).
            state.finish_reason = Some(
                if reason == "max_output_tokens" {
                    "length"
                } else {
                    "incomplete"
                }
                .to_string(),
            );
            return Some(Ok(()));
        }
        "response.failed" => {
            let error = event.pointer("/response/error");
            let code = error.and_then(|e| e.get("code")).and_then(Value::as_str);
            let message = error
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("response failed");
            return Some(Err(map_stream_error(code, message)));
        }
        "error" => {
            let code = event.get("code").and_then(Value::as_str);
            let message = event
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("stream error");
            return Some(Err(map_stream_error(code, message)));
        }
        _ => {}
    }
    None
}

fn process_frame_stream(
    frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>>,
    native_tool_calling: bool,
    request_id: Uuid,
    api_key: String,
    extra_headers: Vec<(String, String)>,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>> {
    let secrets: Vec<String> = std::iter::once(api_key)
        .chain(extra_headers.into_iter().map(|(_, value)| value))
        .collect();
    let state = OpenAiStreamState {
        frames,
        tool_calls: BTreeMap::new(),
        fallback_text: String::new(),
        secrets,
        text_delta_total_len: 0,
        reasoning_delta_total_len: 0,
        native_tool_calling,
        pending: VecDeque::new(),
        finished: false,
        request_id,
        finish_reason: None,
    };

    Box::pin(futures::stream::unfold(
        (state, WebCollector::default()),
        |(mut state, mut web)| async move {
            loop {
                if let Some(event) = state.pending.pop_front() {
                    return Some((event, (state, web)));
                }
                if state.finished {
                    return None;
                }
                match tokio::time::timeout(SSE_INACTIVITY_TIMEOUT, state.frames.next()).await {
                    Ok(Some(Ok(frame))) => {
                        // Malformed frames are skipped, like on the Chat
                        // Completions path.
                        let Ok(event) = serde_json::from_str::<Value>(&frame.data) else {
                            continue;
                        };
                        match handle_event(&mut state, &mut web, &event) {
                            None => {}
                            Some(Ok(())) => {
                                state.finished = true;
                                if let Some(reason) = state.finish_reason.clone() {
                                    crate::request_logging::log_stop_reason(
                                        state.request_id,
                                        "openai_responses",
                                        &reason,
                                    );
                                }
                                let events = state.finalize(false);
                                // Web research is plain information, emitted
                                // before action proposals; a discarded
                                // (retried) response drops it with the rest.
                                let released = !events
                                    .iter()
                                    .any(|e| matches!(e, RawEvent::RetryWithHigherMaxTokens));
                                if released {
                                    state.pending.extend(
                                        web.take()
                                            .into_iter()
                                            .map(|a| RawEvent::Public(AiEvent::WebActivity(a))),
                                    );
                                }
                                state.pending.extend(events);
                            }
                            Some(Err(error)) => {
                                // A failed response never releases a tool call.
                                state.finished = true;
                                state.tool_calls.clear();
                                web.take();
                                state
                                    .pending
                                    .push_back(RawEvent::Public(AiEvent::Error(error)));
                            }
                        }
                    }
                    Ok(Some(Err(err))) => {
                        let mapped = map_transport_error(&err);
                        let secrets: Vec<&str> = state.secrets.iter().map(String::as_str).collect();
                        log_provider_transport_error(state.request_id, &mapped, &secrets);
                        state
                            .pending
                            .push_back(RawEvent::Public(AiEvent::Error(mapped)));
                        state.finished = true;
                    }
                    Ok(None) => {
                        state.finished = true;
                        // No terminal event seen: abrupt.
                        let events = state.finalize(true);
                        state.pending.extend(events);
                    }
                    Err(_elapsed) => {
                        let mapped = timeout_error(SSE_INACTIVITY_TIMEOUT);
                        let secrets: Vec<&str> = state.secrets.iter().map(String::as_str).collect();
                        log_provider_transport_error(state.request_id, &mapped, &secrets);
                        state
                            .pending
                            .push_back(RawEvent::Public(AiEvent::Error(mapped)));
                        state.finished = true;
                    }
                }
            }
        },
    ))
}
