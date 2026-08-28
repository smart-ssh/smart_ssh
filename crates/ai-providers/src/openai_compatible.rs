//! [`AiProvider`]-Implementierung gegen die OpenAI-Chat-Completions-API
//! (Spec 0006, Abschnitt 4, erste Kategorie) — konfigurierbar über die
//! Basis-URL, sodass dieselbe Implementierung für OpenAI selbst, generische
//! OpenAI-kompatible Endpunkte und Ollama im OpenAI-kompatiblen Modus
//! funktioniert.
//!
//! **SSE-Format-Annahme** (nicht bis auf Byte-Ebene in Spec 0006
//! festgelegt, s. Aufgabenstellung Teil 2, Punkt 6 — ADR wird am Ende
//! vorgeschlagen): `data: {...}`-Frames ohne `event:`-Feld,
//! `choices[0].delta.content` für Text-Fragmente,
//! `choices[0].delta.tool_calls[].function.{name,arguments}` für
//! akkumulierende Tool-Call-Fragmente (nach `index` gruppiert), Abschluss
//! durch das Literal `data: [DONE]`.

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

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    /// Ohne abschließenden Slash, z. B. `https://api.openai.com/v1` oder
    /// `http://localhost:11434/v1` (Ollama, OpenAI-kompatibler Modus).
    /// Es wird `/chat/completions` angehängt.
    base_url: String,
    model: String,
    api_key: String,
    /// Fallback-Modus (Spec 0006, Abschnitt 4) wenn `false`.
    supports_native_tool_calling: bool,
}

impl OpenAiCompatibleProvider {
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

        let mut messages = vec![json!({"role": "system", "content": system_text})];
        for message in &context.history {
            messages.push(json!({
                "role": role_str(message.role),
                "content": message_content_text(&message.content),
            }));
        }

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });

        if self.supports_native_tool_calling && !context.available_actions.is_empty() {
            body["tools"] = Value::Array(
                context
                    .available_actions
                    .iter()
                    .map(openai_tool_definition)
                    .collect(),
            );
        }

        body
    }
}

fn role_str(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        // OpenAI kennt keine eigene Rolle für das Ergebnis einer Aktion, die
        // nicht über einen nativen Tool-Call mit `tool_call_id` lief (unser
        // `SessionContext` verfolgt keine Tool-Call-IDs über Turns hinweg) —
        // wird deshalb als normale `user`-Nachricht mit klar beschriftetem
        // Inhalt eingereiht (s. `message_content_text`).
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

fn openai_tool_definition(action: &ActionSchema) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": action.name,
            "description": action.description,
            "parameters": parameters_json_schema(action),
        }
    })
}

impl AiProvider for OpenAiCompatibleProvider {
    fn send(&self, context: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
        let client = self.client.clone();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let api_key = self.api_key.clone();
        let native_tool_calling = self.supports_native_tool_calling;
        let body = self.build_request_body(&context);

        let request = async move {
            let response = match client
                .post(&url)
                .bearer_auth(&api_key)
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

/// Akkumulator für ein einzelnes, über mehrere Chunks verteiltes
/// Tool-Call-Fragment (`name`/`arguments` werden jeweils als Teilstrings
/// geliefert und müssen aneinandergehängt werden).
#[derive(Default)]
struct ToolCallAccumulator {
    name: String,
    arguments: String,
}

struct OpenAiStreamState {
    frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>>,
    tool_calls: BTreeMap<u64, ToolCallAccumulator>,
    fallback_text: String,
    native_tool_calling: bool,
    pending: VecDeque<AiEvent>,
    finished: bool,
}

impl OpenAiStreamState {
    fn handle_chunk(&mut self, chunk: &Value) {
        let Some(delta) = chunk
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("delta"))
        else {
            return;
        };

        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                if self.native_tool_calling {
                    self.pending
                        .push_back(AiEvent::TextDelta(content.to_string()));
                } else {
                    self.fallback_text.push_str(content);
                }
            }
        }

        if !self.native_tool_calling {
            return;
        }
        let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) else {
            return;
        };
        for tool_call in tool_calls {
            let index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0);
            let entry = self.tool_calls.entry(index).or_default();
            if let Some(name) = tool_call
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
            {
                entry.name.push_str(name);
            }
            if let Some(arguments) = tool_call
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
            {
                entry.arguments.push_str(arguments);
            }
        }
    }

    fn finalize(&mut self) -> Vec<AiEvent> {
        let mut events = Vec::new();
        if self.native_tool_calling {
            for (_, call) in std::mem::take(&mut self.tool_calls) {
                events.push(finalize_tool_call(&call.name, &call.arguments));
            }
        } else {
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

fn finalize_tool_call(name: &str, arguments: &str) -> AiEvent {
    match serde_json::from_str::<Value>(arguments) {
        Ok(args_json) => match action_from_tool_arguments(name, &args_json) {
            Ok(action) => AiEvent::ActionProposed(action),
            Err(err) => AiEvent::Error(err),
        },
        // Ein nativer Tool-Call mit kaputtem JSON ist ein Protokollfehler
        // des Providers, kein "Modell hat halt Prosa statt eines Blocks
        // geliefert" wie im Fallback-Modus — deshalb hier bewusst ein
        // `AiError` statt stillschweigendem Text-Fallback (anders als
        // `parse_fallback_response`, s. `fallback.rs`).
        Err(err) => AiEvent::Error(AiError::InvalidResponse(format!(
            "Tool-Call-Argumente sind kein gültiges JSON: {err}"
        ))),
    }
}

fn event_stream_from_response(
    response: reqwest::Response,
    native_tool_calling: bool,
) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
    let state = OpenAiStreamState {
        frames: Box::pin(sse_frame_stream(response)),
        tool_calls: BTreeMap::new(),
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
                    if frame.data.trim() == "[DONE]" {
                        state.finished = true;
                        let events = state.finalize();
                        state.pending.extend(events);
                        continue;
                    }
                    if let Ok(chunk) = serde_json::from_str::<Value>(&frame.data) {
                        state.handle_chunk(&chunk);
                    }
                    // Nicht als JSON parsebare Frames werden ignoriert statt
                    // den Stream mit einem Fehler abzubrechen — ein
                    // einzelnes kaputtes Chunk soll nicht die ganze
                    // Antwort unbrauchbar machen.
                }
                Some(Err(err)) => {
                    state
                        .pending
                        .push_back(AiEvent::Error(map_transport_error(&err)));
                    state.finished = true;
                }
                None => {
                    state.finished = true;
                    let events = state.finalize();
                    state.pending.extend(events);
                }
            }
        }
    }))
}
