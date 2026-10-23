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
use crate::error::{
    classify_provider_error, map_transport_error, timeout_error, ProviderErrorInfo,
};
use crate::fallback::fallback_system_prompt_addition;
use crate::openai_compatible::{
    message_content_text, openai_compatible_default_max_tokens,
    openai_compatible_model_max_output_tokens, retry_stream_with_overload, role_str,
    OpenAiStreamState, RawEvent, RetryKind, RetryState,
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
    /// Issue #599: see `AiProvider::last_request_id`.
    last_request: std::sync::Mutex<Option<Uuid>>,
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
            last_request: std::sync::Mutex::new(None),
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
                let mut parts =
                    crate::content::message_parts(&message.content, message_content_text);
                // Issue #470: text-only keeps the plain string form;
                // image/PDF attachments become `input_*` content parts.
                let content = match parts.as_slice() {
                    [crate::content::ContentPart::Text(_)] => match parts.remove(0) {
                        crate::content::ContentPart::Text(text) => json!(text),
                        _ => unreachable!(),
                    },
                    _ => Value::Array(parts.iter().map(crate::content::responses_block).collect()),
                };
                json!({"role": role_str(message.role), "content": content})
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

        // Issue #470: reject unreadable image/PDF attachments before any
        // request is built.
        if let Err(err) = crate::content::check_attachments(
            &context,
            crate::content::openai_responses_support(&self.base_url, &self.model),
        ) {
            return crate::error::error_stream(err);
        }

        let max_tokens_is_user_override =
            context.max_tokens_hint.is_none() && self.max_tokens_override.is_some();
        let body = self.build_request_body(&context);
        let model_max_tokens =
            openai_compatible_model_max_output_tokens(&self.base_url, &self.model);
        let max_tokens = body[MAX_TOKENS_FIELD]
            .as_u64()
            .unwrap_or(u64::from(model_max_tokens)) as u32;

        retry_stream_with_overload(
            "openai_responses",
            &self.model,
            &self.last_request,
            RetryState {
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
            },
        )
    }

    fn last_request_id(&self) -> Option<Uuid> {
        *self
            .last_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn event_stream_from_response(
    response: reqwest::Response,
    native_tool_calling: bool,
    request_id: Uuid,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    provider_request_id: Option<String>,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>> {
    process_frame_stream(
        Box::pin(sse_frame_stream(response)),
        native_tool_calling,
        request_id,
        api_key,
        extra_headers,
        provider_request_id,
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
    /// `WebContentIngested` was already signalled for this response.
    ingested_signalled: bool,
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

    /// True exactly once per response, at the first web evidence (a search
    /// call or a cited source). The caller emits the content-free
    /// `WebContentIngested` signal right away, so a later error or stop
    /// cannot hide that untrusted content was read (Spec 0105 §6).
    fn first_ingest(&mut self) -> bool {
        !std::mem::replace(&mut self.ingested_signalled, true)
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

fn is_url_citation(annotation: &Value) -> bool {
    annotation.get("type").and_then(Value::as_str) == Some("url_citation")
}

/// Maps a `response.failed` / `error` event to a visible error (Issue #599:
/// shared classification first, then the Responses-specific codes), and
/// writes the one WARN line for it.
fn stream_error(
    state: &OpenAiStreamState,
    error_type: Option<&str>,
    code: Option<&str>,
    message: &str,
) -> AiError {
    let info = ProviderErrorInfo {
        error_type: error_type.map(str::to_string),
        code: code.map(str::to_string),
        message: Some(message.to_string()),
    };
    let mapped = map_stream_error(&info);
    state.report_stream_error(&info, &mapped);
    mapped
}

fn map_stream_error(info: &ProviderErrorInfo) -> AiError {
    if let Some(mapped) = classify_provider_error(None, info) {
        return mapped;
    }
    let message = info.message.clone().unwrap_or_default();
    match info.code.as_deref() {
        Some("context_length_exceeded") => AiError::ContextTooLarge,
        Some("invalid_api_key") => AiError::AuthenticationFailed,
        Some("model_not_found") => AiError::ModelNotFound(message),
        _ => AiError::ProviderUnavailable(message),
    }
}

/// Issue #609: usage, model and tier from the terminal event's `response`.
fn record_usage(state: &mut OpenAiStreamState, event: &Value) {
    let Some(response) = event.get("response") else {
        return;
    };
    if let Some(usage) = response.get("usage").filter(|u| u.is_object()) {
        state.usage = crate::usage::parse_responses(usage);
    }
    state.usage.response_model = crate::usage::model_of(response);
    state.usage.service_tier = crate::usage::service_tier_of(response);
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
                if is_url_citation(annotation) && web.first_ingest() {
                    state
                        .pending
                        .push_back(RawEvent::Public(AiEvent::WebContentIngested));
                }
            }
        }
        "response.output_item.done"
            if event.pointer("/item/type").and_then(Value::as_str) == Some("web_search_call") =>
        {
            web.record_call(event.get("item")?);
            if web.first_ingest() {
                state
                    .pending
                    .push_back(RawEvent::Public(AiEvent::WebContentIngested));
            }
        }
        "response.output_item.done"
            if event.pointer("/item/type").and_then(Value::as_str) == Some("message") =>
        {
            let item = event.get("item")?;
            web.record_message_item(item);
            let cites = item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| part.get("annotations").and_then(Value::as_array))
                .flatten()
                .any(is_url_citation);
            if cites && web.first_ingest() {
                state
                    .pending
                    .push_back(RawEvent::Public(AiEvent::WebContentIngested));
            }
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
            record_usage(state, event);
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
            record_usage(state, event);
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
            let error_type = error.and_then(|e| e.get("type")).and_then(Value::as_str);
            let message = error
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("response failed");
            return Some(Err(stream_error(state, error_type, code, message)));
        }
        "error" => {
            let code = event.get("code").and_then(Value::as_str);
            let error_type = None::<&str>;
            let message = event
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("stream error");
            return Some(Err(stream_error(state, error_type, code, message)));
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
    provider_request_id: Option<String>,
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
        provider_request_id,
        finish_reason: None,
        usage: Default::default(),
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

#[cfg(test)]
mod attachment_tests {
    use super::*;
    use crate::content::test_fixtures::{context_with_image_and_pdf, first_events};
    use ssh_manager_core::ai::{AiEvent, MediaClass};

    fn budget() -> std::sync::Arc<crate::rate_limit_budget::ProviderBudgetGuard> {
        std::sync::Arc::new(crate::rate_limit_budget::ProviderBudgetGuard::new())
    }

    /// Issue #504: an unsealed extension part is the fixed fenced placeholder.
    #[test]
    fn test_responses_extension_part_renders_fixed_placeholder() {
        let p = OpenAiResponsesProvider::new(
            "https://api.openai.com/v1",
            "gpt-4o",
            "k",
            true,
            Vec::new(),
            budget(),
            None,
        );
        let ctx = crate::content::test_fixtures::context_with_extension_part("report.pdf");
        crate::content::test_fixtures::assert_extension_placeholder(&p.build_request_body(&ctx));
    }

    #[test]
    fn test_responses_image_and_pdf_input_parts() {
        let p = OpenAiResponsesProvider::new(
            "https://api.openai.com/v1",
            "gpt-4o",
            "k",
            true,
            Vec::new(),
            budget(),
            None,
        );
        let body = p.build_request_body(&context_with_image_and_pdf("hello"));
        let content = body["input"][0]["content"].as_array().expect("array");
        let types: Vec<&str> = content
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            ["input_text", "input_image", "input_text", "input_file"]
        );
        assert_eq!(
            content[1]["image_url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(content[3]["filename"], "d.pdf");
        assert_eq!(
            content[3]["file_data"],
            "data:application/pdf;base64,JVBERi0x"
        );
        // A plain text message keeps the string form.
        assert_eq!(body["input"][1]["content"], "what is this?");
    }

    #[tokio::test]
    async fn test_responses_non_openai_endpoint_rejects_image_without_request() {
        let p = OpenAiResponsesProvider::new(
            "http://127.0.0.1:1/v1",
            "gpt-4o",
            "k",
            true,
            Vec::new(),
            budget(),
            None,
        );
        let events = first_events(&p, context_with_image_and_pdf("x")).await;
        assert!(
            matches!(
                events[..],
                [AiEvent::Error(AiError::InputUnsupported(MediaClass::Image))]
            ),
            "{events:?}"
        );
    }

    /// Issue #604: an unknown function call on the Responses path is
    /// non-fatal; the valid call of the same response is kept.
    #[tokio::test]
    async fn test_responses_unknown_tool_is_non_fatal() {
        use futures::StreamExt;
        let frame = |data: &str| -> Result<crate::sse::SseFrame, reqwest::Error> {
            Ok(crate::sse::SseFrame {
                event: None,
                data: data.to_string(),
            })
        };
        let frames: Pin<
            Box<dyn Stream<Item = Result<crate::sse::SseFrame, reqwest::Error>> + Send>,
        > = Box::pin(futures::stream::iter(vec![
            frame(
                r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","name":"date","arguments":""}}"#,
            ),
            frame(
                r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","name":"suggest_command","arguments":"{\"command\":\"ls\"}"}}"#,
            ),
            frame(r#"{"type":"response.completed"}"#),
        ]));
        let events: Vec<RawEvent> = process_frame_stream(
            frames,
            true,
            Uuid::new_v4(),
            String::new(),
            Vec::new(),
            None,
        )
        .collect()
        .await;
        assert!(
            matches!(
                &events[0],
                RawEvent::Public(AiEvent::InvalidToolCall { tool_name, .. }) if tool_name == "date"
            ),
            "{events:?}"
        );
        assert!(matches!(
            events[1],
            RawEvent::Public(AiEvent::ActionProposed(_))
        ));
        assert!(!events
            .iter()
            .any(|e| matches!(e, RawEvent::Public(AiEvent::Error(_)))));
    }
}

#[cfg(test)]
mod issue_615_tests {
    use super::*;

    fn info(error_type: &str, code: &str) -> ProviderErrorInfo {
        ProviderErrorInfo {
            error_type: Some(error_type.into()),
            code: Some(code.into()),
            message: Some("m".into()),
        }
    }

    #[test]
    fn test_in_stream_context_length_exceeded_is_context_too_large() {
        assert_eq!(
            map_stream_error(&info("invalid_request_error", "context_length_exceeded")),
            AiError::ContextTooLarge
        );
        assert!(matches!(
            map_stream_error(&info("invalid_request_error", "other")),
            AiError::RejectedRequest(_)
        ));
    }
}
