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
//! akkumulierende Tool-Call-Fragmente (nach `index` gruppiert),
//! `choices[0].finish_reason` (geloggt; seit Spec 0065,
//! Teil 3 zusätzlich sicherheitskritisch ausgewertet — ein Tool-Call aus
//! einer Antwort mit `finish_reason: length` wird nie freigegeben, s.
//! `OpenAiStreamState::finalize`), Abschluss durch das Literal
//! `data: [DONE]`.

use std::pin::Pin;

use futures::Stream;
use serde_json::{json, Value};
use uuid::Uuid;

use ssh_manager_core::ai::{ActionSchema, AiEvent, AiProvider, RedactedContext, SessionContext};

use crate::action::parameters_json_schema;
use crate::common::frame_loop;
use crate::common::max_tokens_retry::{
    retry_stream_with_overload, ConnectFn, RawEvent, RetryParams,
};
use crate::common::prompt_format::{message_content_text, role_str};
use crate::error::error_stream;
use crate::fallback::fallback_system_prompt_addition;
use crate::openai_common::{
    connect_and_stream, openai_compatible_default_max_tokens,
    openai_compatible_model_max_output_tokens, openai_max_tokens_field_name, OpenAiStreamState,
    StreamOptionsRejections, OPENAI_COMPATIBLE_CONTEXT_LIMIT_RETRY_MAX_TOKENS,
};
use crate::request_logging::log_outgoing_context;
use crate::sse::build_http_client;

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
    /// Spec 0025, Abschnitt 3: anbieterspezifische Zusatz-Header (z. B.
    /// OpenRouters optionale `HTTP-Referer`/`X-Title`) — an jeden Request
    /// angehängt, nach `bearer_auth`/`accept` gesetzt, kann diese bei
    /// Namensgleichheit also überschreiben (bewusst: ein Nutzer, der z. B.
    /// selbst einen `accept`-Header einträgt, meint das ernst).
    extra_headers: Vec<(String, String)>,
    /// Issue #609: endpoints that rejected `stream_options`, shared with every
    /// provider built from the same registry (see [`StreamOptionsRejections`]).
    /// This family never records rate-limit headers (Spec 0061: a header-less
    /// provider is never blocked), so it holds no budget guard.
    stream_options_rejections: StreamOptionsRejections,
    /// Spec 0065, Teil 4 — s. `crate::anthropic::AnthropicProvider::
    /// max_tokens_override`-Doc-Kommentar für das identische Muster
    /// (Vorrang vor dem modellabhängigen Default, aber NICHT vor einem
    /// `SessionContext::max_tokens_hint` eines Nebenaufrufs).
    max_tokens_override: Option<u32>,
    /// Issue #599: see `AiProvider::last_request_id`.
    last_request: std::sync::Mutex<Option<Uuid>>,
}

impl OpenAiCompatibleProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        supports_native_tool_calling: bool,
        extra_headers: Vec<(String, String)>,
        stream_options_rejections: StreamOptionsRejections,
        max_tokens_override: Option<u32>,
    ) -> Self {
        Self {
            client: build_http_client(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
            supports_native_tool_calling,
            extra_headers,
            stream_options_rejections,
            max_tokens_override,
            last_request: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn build_request_body(&self, context: &SessionContext) -> Value {
        let mut system_text = context.system_context.clone();
        if !self.supports_native_tool_calling {
            system_text.push_str(&fallback_system_prompt_addition(&context.available_actions));
        }

        // Spec 0087, A3.1 (BL-0264): analog zu
        // `AnthropicProvider::build_request_body`s gleichnamiger Regel (Spec
        // 0081) — ist der System-Text (nach dem optionalen Fallback-Zusatz)
        // leer oder reiner Leerraum, entfällt die System-Nachricht ganz,
        // statt mit leerem `content` gesendet zu werden. Der Fallback-Zusatz
        // (`!self.supports_native_tool_calling`-Zweig oben) ist nie leer, der
        // Fallback-Modus behält seine System-Nachricht also immer.
        let mut messages = Vec::new();
        if !system_text.trim().is_empty() {
            messages.push(json!({"role": "system", "content": system_text}));
        }
        // Issue #245 (Spec 0066): adjacent messages with the same provider
        // role (action result + queued user text, several queued texts) go
        // out as one message, texts joined by a blank line. Runs on the
        // already fenced/formatted text; the stored history is untouched.
        for (role, parts) in
            crate::content::group_parts(&context.history, role_str, message_content_text)
        {
            messages.push(json!({
                "role": role,
                "content": chat_content(parts),
            }));
        }

        // Spec 0065, Teil 1+4: `max_tokens_hint` (Nebenaufrufe) > Nutzer-
        // Override (Teil 4) > modellabhängiger Haupt-Chat-Default — s.
        // `SessionContext::max_tokens_hint`-Doc-Kommentar (core) und
        // `crate::anthropic`s identisches Muster.
        let max_tokens = context
            .max_tokens_hint
            .or(self.max_tokens_override)
            .unwrap_or_else(|| openai_compatible_default_max_tokens(&self.base_url, &self.model));

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            // Issue #609: ask for the final usage chunk. Endpoints that
            // reject the option are retried once without it
            // (`connect_and_stream`).
            "stream_options": {"include_usage": true},
        });
        body[openai_max_tokens_field_name(&self.base_url, &self.model)] = json!(max_tokens);

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

/// Issue #245/#470: message content for chat completions. Text-only groups
/// keep the plain string form (texts joined by a blank line); a group with an
/// image/PDF becomes a content-part array with the texts coalesced between
/// the binary parts.
fn chat_content(parts: Vec<crate::content::ContentPart>) -> Value {
    use crate::content::ContentPart;
    if parts.iter().all(|p| matches!(p, ContentPart::Text(_))) {
        let texts: Vec<String> = parts
            .into_iter()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        return json!(texts.join("\n\n"));
    }
    let mut blocks: Vec<Value> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for part in &parts {
        if let ContentPart::Text(t) = part {
            pending.push(t.clone());
            continue;
        }
        if !pending.is_empty() {
            blocks.push(json!({"type": "text", "text": pending.join("\n\n")}));
            pending.clear();
        }
        blocks.push(crate::content::chat_completions_block(part));
    }
    if !pending.is_empty() {
        blocks.push(json!({"type": "text", "text": pending.join("\n\n")}));
    }
    Value::Array(blocks)
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
    fn send(&self, context: RedactedContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
        let context = context.into_parts();
        // s. `AnthropicProvider::send` (Spec 0016, Abschnitt 4).
        let request_id = Uuid::new_v4();
        log_outgoing_context(request_id, &context);

        // Issue #470: reject unreadable image/PDF attachments before any
        // request is built.
        if let Err(err) = crate::content::check_attachments(
            &context,
            crate::content::openai_compatible_support(&self.base_url, &self.model),
        ) {
            return error_stream(err);
        }

        let client = self.client.clone();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let api_key = self.api_key.clone();
        let native_tool_calling = self.supports_native_tool_calling;
        let extra_headers = self.extra_headers.clone();
        let rejections = self.stream_options_rejections.clone();
        // Spec 0087, A1.2a: MUSS vor `build_request_body` berechnet werden —
        // dieselbe Vorrang-Reihenfolge wie dort (`max_tokens_hint.or(self.
        // max_tokens_override)`).
        let max_tokens_is_user_override =
            context.max_tokens_hint.is_none() && self.max_tokens_override.is_some();
        let body = self.build_request_body(&context);
        let model_max_tokens =
            openai_compatible_model_max_output_tokens(&self.base_url, &self.model);
        let max_tokens_field = openai_max_tokens_field_name(&self.base_url, &self.model);
        // The `.unwrap_or(..)` practically never applies: `body` always
        // carries the value from `build_request_body`.
        let max_tokens = body[max_tokens_field]
            .as_u64()
            .unwrap_or(u64::from(model_max_tokens)) as u32;

        let connect: ConnectFn = std::sync::Arc::new(move |body| {
            Box::pin(connect_and_stream(
                parse_sse_response,
                client.clone(),
                url.clone(),
                api_key.clone(),
                native_tool_calling,
                request_id,
                extra_headers.clone(),
                rejections.clone(),
                body,
            ))
        });
        retry_stream_with_overload(
            "openai_compatible",
            &self.model,
            &self.last_request,
            RetryParams {
                connect,
                request_id,
                body,
                max_tokens,
                model_max_tokens,
                max_tokens_field,
                max_tokens_is_user_override,
                context_limit_retry_max_tokens: Some(
                    OPENAI_COMPATIBLE_CONTEXT_LIMIT_RETRY_MAX_TOKENS,
                ),
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

fn parse_sse_response(
    response: reqwest::Response,
    native_tool_calling: bool,
    request_id: Uuid,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    provider_request_id: Option<String>,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>> {
    frame_loop::event_stream_from_response(
        response,
        OpenAiStreamState::new(
            native_tool_calling,
            request_id,
            api_key,
            extra_headers,
            provider_request_id,
        ),
    )
}

/// Test entry: the shared frame loop over a synthetic `frames` stream.
#[cfg(test)]
fn frame_stream(
    frames: frame_loop::FrameStream,
    native_tool_calling: bool,
    request_id: Uuid,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    provider_request_id: Option<String>,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>> {
    frame_loop::process_frame_stream(
        frames,
        OpenAiStreamState::new(
            native_tool_calling,
            request_id,
            api_key,
            extra_headers,
            provider_request_id,
        ),
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_attachments;
