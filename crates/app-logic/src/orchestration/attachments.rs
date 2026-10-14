//! Spec 0110: extension content in user chat messages. The core checks the
//! generic limits and takes opaque ids from the registered chat content
//! extensions; it has no file-picking path and never accepts paths from the
//! webview.

use std::sync::Arc;

use ssh_manager_core::ai::{ChatMessage, MessageContent, OutputRedactor, Role};

use crate::error::{CommandError, CommandResult};
use crate::session::Session;
use crate::state::SessionId;

use ssh_manager_core::ai::{
    seal_pending, ExtensionPart, RedactedBlock, MAX_EXTENSION_PARTS_PER_MESSAGE,
};

use crate::chat_content_extension::{limit_error_code, ChatContentExtension, ChatContentRegistry};

use super::chat_turn::push_history;

/// Stabile Fehlercodes (ein Anhang-Fehler sendet nie etwas).
pub const ATTACHMENTS_UNAVAILABLE: &str = "ATTACHMENTS_UNAVAILABLE";
pub const ATTACHMENTS_TOO_MANY: &str = "ATTACHMENTS_TOO_MANY";
pub const ATTACHMENT_TOO_LARGE: &str = "ATTACHMENT_TOO_LARGE";
pub const ATTACHMENT_INVALID: &str = "ATTACHMENT_INVALID";
pub const ATTACHMENT_UNRESOLVED: &str = "ATTACHMENT_UNRESOLVED";
/// Anhänge lassen sich nicht an einen laufenden Turn anhängen (nur
/// Textnachrichten werden eingereiht).
/// More than 20 MiB of binary bytes in one request (Issue #505).
pub const ATTACHMENTS_REQUEST_TOO_LARGE: &str = "ATTACHMENTS_REQUEST_TOO_LARGE";
pub const ATTACHMENTS_WHILE_BUSY: &str = "ATTACHMENTS_WHILE_BUSY";

/// Extension content the core accepted (limits checked, text redacted) but
/// has not handed to the extension yet. Persisting happens separately so a
/// rejected send (e.g. a running turn) stores nothing.
#[derive(Default)]
pub struct PreparedExtensionParts(Vec<PreparedPart>);

struct PreparedPart {
    extension: Arc<dyn ChatContentExtension>,
    part: ExtensionPart,
    blocks: Vec<RedactedBlock>,
}

impl PreparedExtensionParts {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn parts_for_tests(&self) -> Vec<(&ExtensionPart, &Vec<RedactedBlock>)> {
        self.0.iter().map(|p| (&p.part, &p.blocks)).collect()
    }

    /// Hands the checked blocks to their extensions. An error means nothing
    /// is sent.
    pub fn persist(&self, session_id: SessionId) -> CommandResult<()> {
        for prepared in &self.0 {
            prepared
                .extension
                .persist(session_id, &prepared.part.id, prepared.blocks.clone())
                .map_err(|_| {
                    CommandError::with_code(
                        "The message was not sent: the attachment could not be stored.",
                        ATTACHMENT_UNRESOLVED,
                    )
                })?;
        }
        Ok(())
    }
}

/// Accept path (Issue #505, ADR 0141): asks the registered extensions'
/// `take_pending` in order for each id, checks the generic limits, derives
/// `media`, redacts text and label and computes the token estimate. Nothing
/// is stored yet. No ids: empty result; ids without extension:
/// `ATTACHMENTS_UNAVAILABLE`; unknown id: `ATTACHMENT_UNRESOLVED`.
pub fn accept_extension_content(
    registry: &ChatContentRegistry,
    redactor: &dyn OutputRedactor,
    session_id: SessionId,
    ids: &[String],
) -> CommandResult<PreparedExtensionParts> {
    if ids.is_empty() {
        return Ok(PreparedExtensionParts::default());
    }
    if registry.is_empty() {
        return Err(CommandError::with_code(
            "The message was not sent: attachments are not available.",
            ATTACHMENTS_UNAVAILABLE,
        ));
    }
    let limit_err = |err: ssh_manager_core::ai::ContentLimitError| {
        CommandError::with_code(
            format!("The message was not sent: {err}"),
            limit_error_code(&err),
        )
    };
    if ids.len() > MAX_EXTENSION_PARTS_PER_MESSAGE {
        return Err(limit_err(
            ssh_manager_core::ai::ContentLimitError::TooManyParts { count: ids.len() },
        ));
    }
    let mut prepared = Vec::with_capacity(ids.len());
    'ids: for id in ids {
        for extension in registry.extensions() {
            let Some(pending) = extension.take_pending(session_id, id) else {
                continue;
            };
            let (blocks, media, estimated_tokens) =
                seal_pending(pending.blocks, redactor).map_err(limit_err)?;
            prepared.push(PreparedPart {
                part: ExtensionPart {
                    extension: extension.name().to_string(),
                    kind: pending.kind,
                    id: uuid::Uuid::new_v4().to_string(),
                    label: redactor.redact_text(&pending.label),
                    media,
                    estimated_tokens,
                },
                extension: extension.clone(),
                blocks,
            });
            continue 'ids;
        }
        return Err(CommandError::with_code(
            "The message was not sent: unknown attachment id.",
            ATTACHMENT_UNRESOLVED,
        ));
    }
    Ok(PreparedExtensionParts(prepared))
}

/// Puts one `Extension` part per item into the history, right after the
/// message text, and sets the untrusted flag (Spec 0039 §5).
pub async fn push_extension_parts(session: &Session, parts: PreparedExtensionParts) {
    if parts.is_empty() {
        return;
    }
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);
    for prepared in parts.0 {
        push_history(
            session,
            ChatMessage {
                role: Role::User,
                content: MessageContent::Extension(prepared.part),
            },
        )
        .await;
    }
}
