//! Issue #1185: the events of a login prompt (`auth-prompt-needed`,
//! `auth-prompt-ended`). They carry the questions and the prompt id, never
//! an answer.

use serde::Serialize;

use crate::event_names as names;
use crate::events::EventEmitter;

fn emit<T: Serialize>(emitter: &dyn EventEmitter, event: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(value) => emitter.emit_event(event, value),
        Err(err) => eprintln!("Event '{event}' nicht serialisierbar: {err}"),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub(crate) struct AuthPromptItemPayload {
    text: String,
    echo: bool,
}

/// Issue #1185: the server asks for login input. Carries the questions
/// (untrusted server text, already reduced to plain text) and never a secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub(crate) struct AuthPromptNeededPayload {
    prompt_id: String,
    /// Identifies the connect that asks; later rounds of the same login share it.
    connect_id: String,
    /// `user@host:port` of the hop that asks.
    hop: String,
    /// `keyboard_interactive` or `password`.
    kind: &'static str,
    name: String,
    instructions: String,
    prompts: Vec<AuthPromptItemPayload>,
}

pub fn emit_auth_prompt_needed(
    emitter: &dyn EventEmitter,
    prompt_id: &str,
    connect_id: &str,
    request: &ssh_manager_core::ssh::AuthPromptRequest,
) {
    use ssh_manager_core::ssh::AuthPromptKind;
    let (kind, name, instructions, prompts) = match &request.kind {
        AuthPromptKind::KeyboardInteractive {
            name,
            instructions,
            prompts,
        } => (
            "keyboard_interactive",
            name.clone(),
            instructions.clone(),
            prompts
                .iter()
                .map(|p| AuthPromptItemPayload {
                    text: p.text.clone(),
                    echo: p.echo,
                })
                .collect(),
        ),
        AuthPromptKind::Password => ("password", String::new(), String::new(), Vec::new()),
    };
    emit(
        emitter,
        names::AUTH_PROMPT_NEEDED,
        &AuthPromptNeededPayload {
            prompt_id: prompt_id.to_string(),
            connect_id: connect_id.to_string(),
            hop: request.hop.clone(),
            kind,
            name,
            instructions,
            prompts,
        },
    );
}

/// Why the backend stopped waiting for a login prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum AuthPromptEndReason {
    Answered,
    Cancelled,
    TimedOut,
    /// The connect was aborted while the dialog was open.
    Abandoned,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub(crate) struct AuthPromptEndedPayload {
    prompt_id: String,
    reason: AuthPromptEndReason,
}

pub fn emit_auth_prompt_ended(
    emitter: &dyn EventEmitter,
    prompt_id: &str,
    reason: AuthPromptEndReason,
) {
    emit(
        emitter,
        names::AUTH_PROMPT_ENDED,
        &AuthPromptEndedPayload {
            prompt_id: prompt_id.to_string(),
            reason,
        },
    );
}
