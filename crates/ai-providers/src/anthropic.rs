//! [`AiProvider`]-Implementierung gegen die Anthropic-Messages-API (Spec
//! 0006, Abschnitt 4) — natives Tool-Calling über `tool_use`-Content-Blocks.
//!
//! **SSE-Format-Annahme** (nicht bis auf Byte-Ebene in Spec 0006
//! festgelegt, s. Aufgabenstellung Teil 2, Punkt 6 — ADR wird am Ende
//! vorgeschlagen): benannte Events (`event: <typ>`) mit JSON-Payload;
//! `content_block_start` mit `content_block.type` (`"text"`/`"tool_use"`)
//! eröffnet einen nach `index` adressierten Block,
//! `content_block_delta` liefert `delta.type == "text_delta"` (Feld
//! `text`) bzw. `"input_json_delta"` (Feld `partial_json`, akkumulierend),
//! `content_block_stop` schließt den Block ab, `message_stop` beendet den
//! Stream.
//!
//! Die Anthropic-API verlangt zwingend ein `max_tokens`-Feld, das die Spec
//! nicht erwähnt und das dieser Provider aktuell nicht konfigurierbar
//! macht — s. [`DEFAULT_MAX_TOKENS`].

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;

use futures::future::FutureExt;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use ssh_manager_core::ai::{
    ActionSchema, AiError, AiEvent, AiProvider, MessageContent, Role, SessionContext,
};
use ssh_manager_core::ssh::CommandOutput;

use crate::action::{action_from_tool_arguments, parameters_json_schema};
use crate::error::{error_stream, map_http_status, map_transport_error};
use crate::fallback::{fallback_system_prompt_addition, parse_fallback_response};
use crate::sse::{sse_frame_stream, SseFrame};

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// s. Modul-Dokumentation — von der Spec nicht vorgegeben, aber von der
/// Anthropic-API zwingend verlangt.
const DEFAULT_MAX_TOKENS: u32 = 4096;

pub struct AnthropicProvider {
    client: reqwest::Client,
    /// Ohne abschließenden Slash, z. B. `https://api.anthropic.com`. Es
    /// wird `/v1/messages` angehängt.
    base_url: String,
    model: String,
    api_key: String,
    supports_native_tool_calling: bool,
}

impl AnthropicProvider {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        supports_native_tool_calling: bool,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
            supports_native_tool_calling,
        }
    }

    fn build_request_body(&self, context: &SessionContext) -> Value {
        let mut system_text = context.system_context.clone();
        if !self.supports_native_tool_calling {
            system_text.push_str(&fallback_system_prompt_addition(&context.available_actions));
        }

        let messages: Vec<Value> = context
            .history
            .iter()
            .map(|message| {
                json!({
                    "role": role_str(message.role),
                    "content": message_content_text(&message.content),
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "system": system_text,
            "messages": messages,
            "max_tokens": DEFAULT_MAX_TOKENS,
            "stream": true,
        });

        if self.supports_native_tool_calling && !context.available_actions.is_empty() {
            body["tools"] = Value::Array(
                context
                    .available_actions
                    .iter()
                    .map(anthropic_tool_definition)
                    .collect(),
            );
        }

        body
    }
}

fn role_str(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        // Wie bei OpenAiCompatibleProvider (s. dort): Anthropic kennt keine
        // eigene Rolle für ein "loses" Aktionsergebnis ohne zugehörige
        // `tool_use_id` — wird als `user`-Nachricht mit beschriftetem Inhalt
        // eingereiht.
        Role::ActionResult => "user",
        Role::Assistant => "assistant",
    }
}

fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::CommandResult { command, output } => format_command_result(command, output),
    }
}

fn format_command_result(command: &str, output: &CommandOutput) -> String {
    format!(
        "Kommando ausgeführt: {command}\nExit-Code: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.exit_code,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn anthropic_tool_definition(action: &ActionSchema) -> Value {
    json!({
        "name": action.name,
        "description": action.description,
        "input_schema": parameters_json_schema(action),
    })
}

impl AiProvider for AnthropicProvider {
    fn send(&self, context: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
        let client = self.client.clone();
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let api_key = self.api_key.clone();
        let native_tool_calling = self.supports_native_tool_calling;
        let body = self.build_request_body(&context);

        let request = async move {
            let response = match client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("accept", "text/event-stream")
                .json(&body)
                .send()
                .await
            {
                Ok(response) => response,
                Err(err) => return error_stream(map_transport_error(&err)),
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return error_stream(map_http_status(status, &text));
            }

            event_stream_from_response(response, native_tool_calling)
        };

        Box::pin(request.flatten_stream())
    }
}

enum BlockKind {
    Text,
    ToolUse { name: String, json_acc: String },
}

struct AnthropicStreamState {
    frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>>,
    blocks: BTreeMap<u64, BlockKind>,
    fallback_text: String,
    native_tool_calling: bool,
    pending: VecDeque<AiEvent>,
    finished: bool,
}

impl AnthropicStreamState {
    fn handle_event(&mut self, event: &str, data: &Value) {
        match event {
            "content_block_start" => {
                let Some(index) = data.get("index").and_then(Value::as_u64) else {
                    return;
                };
                let block_type = data
                    .get("content_block")
                    .and_then(|b| b.get("type"))
                    .and_then(Value::as_str);
                let kind = match block_type {
                    Some("tool_use") => BlockKind::ToolUse {
                        name: data
                            .get("content_block")
                            .and_then(|b| b.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        json_acc: String::new(),
                    },
                    _ => BlockKind::Text,
                };
                self.blocks.insert(index, kind);
            }
            "content_block_delta" => {
                let Some(index) = data.get("index").and_then(Value::as_u64) else {
                    return;
                };
                let Some(delta) = data.get("delta") else {
                    return;
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str) {
                            if self.native_tool_calling {
                                self.pending.push_back(AiEvent::TextDelta(text.to_string()));
                            } else {
                                self.fallback_text.push_str(text);
                            }
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(BlockKind::ToolUse { json_acc, .. }) =
                            self.blocks.get_mut(&index)
                        {
                            if let Some(partial) = delta.get("partial_json").and_then(Value::as_str)
                            {
                                json_acc.push_str(partial);
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(index) = data.get("index").and_then(Value::as_u64) else {
                    return;
                };
                if let Some(BlockKind::ToolUse { name, json_acc }) = self.blocks.remove(&index) {
                    self.pending.push_back(finalize_tool_use(&name, &json_acc));
                }
            }
            "message_stop" => {
                self.finished = true;
                let events = self.finalize();
                self.pending.extend(events);
            }
            "error" => {
                let message = data
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("unbekannter Fehler")
                    .to_string();
                self.pending
                    .push_back(AiEvent::Error(AiError::ProviderUnavailable(message)));
                self.finished = true;
            }
            _ => {}
        }
    }

    fn finalize(&mut self) -> Vec<AiEvent> {
        let mut events = Vec::new();
        if !self.native_tool_calling {
            let result = parse_fallback_response(&self.fallback_text);
            if !result.text.is_empty() {
                events.push(AiEvent::TextDelta(result.text));
            }
            if let Some(action) = result.action {
                events.push(AiEvent::ActionProposed(action));
            }
        }
        events.push(AiEvent::Done);
        events
    }
}

fn finalize_tool_use(name: &str, json_acc: &str) -> AiEvent {
    match serde_json::from_str::<Value>(json_acc) {
        Ok(args_json) => match action_from_tool_arguments(name, &args_json) {
            Ok(action) => AiEvent::ActionProposed(action),
            Err(err) => AiEvent::Error(err),
        },
        Err(err) => AiEvent::Error(AiError::InvalidResponse(format!(
            "Tool-Use-Input ist kein gültiges JSON: {err}"
        ))),
    }
}

fn event_stream_from_response(
    response: reqwest::Response,
    native_tool_calling: bool,
) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
    let state = AnthropicStreamState {
        frames: Box::pin(sse_frame_stream(response)),
        blocks: BTreeMap::new(),
        fallback_text: String::new(),
        native_tool_calling,
        pending: VecDeque::new(),
        finished: false,
    };

    Box::pin(futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((event, state));
            }
            if state.finished {
                return None;
            }
            match state.frames.next().await {
                Some(Ok(frame)) => {
                    let Some(event_type) = frame.event.clone() else {
                        continue;
                    };
                    if let Ok(data) = serde_json::from_str::<Value>(&frame.data) {
                        state.handle_event(&event_type, &data);
                    }
                    // Frames mit nicht-parsebarem JSON werden ignoriert
                    // statt den Stream mit einem Fehler abzubrechen — s.
                    // Begründung in `openai_compatible.rs`.
                }
                Some(Err(err)) => {
                    state
                        .pending
                        .push_back(AiEvent::Error(map_transport_error(&err)));
                    state.finished = true;
                }
                None => {
                    // Verbindung endete ohne `message_stop`-Event (z. B.
                    // abgeschnittene Antwort) — trotzdem sauber abschließen
                    // statt den Stream einfach verstummen zu lassen.
                    if !state.finished {
                        state.finished = true;
                        let events = state.finalize();
                        state.pending.extend(events);
                    } else {
                        return None;
                    }
                }
            }
        }
    }))
}
