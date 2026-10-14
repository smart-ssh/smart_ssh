//! Issue #469, Spec 0110: Dateianhänge an Nutzer-Nachrichten. Der Kern prüft
//! Grenzen und löst IDs über die von einer Edition registrierten
//! [`ChatAttachmentResolver`] auf; er kennt keinen Dateiauswahl-Weg und
//! akzeptiert nie Pfade aus dem Webview.

use std::sync::Arc;

use ssh_manager_core::ai::{
    validate_attachments, AttachmentError, ChatAttachment, ChatAttachmentResolver, ChatMessage,
    MessageContent, OutputRedactor, Role, MAX_ATTACHMENTS_PER_MESSAGE,
};

use crate::error::{CommandError, CommandResult};
use crate::session::Session;
use crate::state::SessionId;

use ssh_manager_core::ai::{seal_pending, ExtensionPart, RedactedBlock};

use crate::chat_content_extension::{limit_error_code, ChatContentExtension, ChatContentRegistry};

use super::chat_turn::{push_history, redact_attachments};

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

pub fn attachment_error(err: AttachmentError) -> CommandError {
    let code = match &err {
        AttachmentError::TooMany { .. } => ATTACHMENTS_TOO_MANY,
        AttachmentError::TooLarge { .. } => ATTACHMENT_TOO_LARGE,
        AttachmentError::InvalidText { .. } => ATTACHMENT_INVALID,
        AttachmentError::NoResolver => ATTACHMENTS_UNAVAILABLE,
        AttachmentError::Unresolved { .. } => ATTACHMENT_UNRESOLVED,
    };
    CommandError::with_code(format!("Die Nachricht wurde nicht gesendet: {err}"), code)
}

/// Löst `ids` auf. Ohne IDs: leere Liste, ohne Resolver zu brauchen. Mit IDs
/// und ohne Resolver: Fehler. Die Grenzen (Anzahl, Größe) gelten für das
/// Ergebnis, unabhängig davon, was ein Resolver liefert.
pub async fn resolve_chat_attachments(
    resolvers: &[Arc<dyn ChatAttachmentResolver>],
    session_id: SessionId,
    ids: &[String],
) -> CommandResult<Vec<ChatAttachment>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if resolvers.is_empty() {
        return Err(attachment_error(AttachmentError::NoResolver));
    }
    if ids.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(attachment_error(AttachmentError::TooMany {
            count: ids.len(),
        }));
    }
    let mut resolved = Vec::with_capacity(ids.len());
    'ids: for id in ids {
        for resolver in resolvers {
            if let Some(attachment) = resolver
                .resolve(session_id, id)
                .await
                .map_err(attachment_error)?
            {
                resolved.push(attachment);
                continue 'ids;
            }
        }
        return Err(attachment_error(AttachmentError::Unresolved {
            reason: "unbekannte Anhang-ID".to_string(),
        }));
    }
    validate_attachments(&resolved).map_err(attachment_error)?;
    Ok(resolved)
}

/// Legt die Anhänge als Nutzer-Nachricht NACH dem zugehörigen Text in die
/// Historie. Prüft die Grenzen erneut (fail closed), redigiert den Inhalt vor
/// dem Speichern und setzt das Untrusted-Flag (Spec 0039 §5).
pub async fn push_user_attachments(
    session: &Session,
    attachments: Vec<ChatAttachment>,
) -> CommandResult<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    validate_attachments(&attachments).map_err(attachment_error)?;
    let redactor: &dyn OutputRedactor = &session.redactor;
    let attachments = redact_attachments(attachments, redactor);
    session
        .untrusted_content_ingested
        .store(true, std::sync::atomic::Ordering::SeqCst);
    push_history(
        session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Attachments(attachments),
        },
    )
    .await;
    Ok(())
}

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
        return Err(attachment_error(AttachmentError::NoResolver));
    }
    let limit_err = |err: ssh_manager_core::ai::ContentLimitError| {
        CommandError::with_code(
            format!("The message was not sent: {err}"),
            limit_error_code(&err),
        )
    };
    if ids.len() > MAX_ATTACHMENTS_PER_MESSAGE {
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
        return Err(attachment_error(AttachmentError::Unresolved {
            reason: "unknown attachment id".to_string(),
        }));
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

// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ssh_manager_core::ai::{AttachmentKind, MAX_ATTACHMENT_BYTES};
    use uuid::Uuid;

    /// Resolver, der jede ID mit einem Textanhang der Größe `size` beantwortet.
    struct FixedResolver {
        size: usize,
    }

    #[async_trait]
    impl ChatAttachmentResolver for FixedResolver {
        async fn resolve(
            &self,
            _session_id: Uuid,
            attachment_id: &str,
        ) -> Result<Option<ChatAttachment>, AttachmentError> {
            if attachment_id == "unknown" {
                return Ok(None);
            }
            Ok(Some(ChatAttachment {
                name: format!("{attachment_id}.txt"),
                media_type: "text/plain".to_string(),
                kind: AttachmentKind::Text,
                data: vec![b'a'; self.size],
                byte_len: self.size as u64,
            }))
        }
    }

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("id{i}")).collect()
    }

    fn one_resolver(size: usize) -> Vec<Arc<dyn ChatAttachmentResolver>> {
        vec![Arc::new(FixedResolver { size })]
    }

    #[tokio::test]
    async fn test_ids_without_resolver_is_typed_error() {
        let err = resolve_chat_attachments(&[], Uuid::new_v4(), &ids(1))
            .await
            .unwrap_err();
        assert_eq!(err.code, Some(ATTACHMENTS_UNAVAILABLE));
    }

    #[tokio::test]
    async fn test_no_ids_needs_no_resolver_and_yields_nothing() {
        let out = resolve_chat_attachments(&[], Uuid::new_v4(), &[])
            .await
            .unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn test_six_ids_rejected_five_accepted() {
        let resolvers = one_resolver(10);
        let err = resolve_chat_attachments(&resolvers, Uuid::new_v4(), &ids(6))
            .await
            .unwrap_err();
        assert_eq!(err.code, Some(ATTACHMENTS_TOO_MANY));
        let ok = resolve_chat_attachments(&resolvers, Uuid::new_v4(), &ids(5))
            .await
            .unwrap();
        assert_eq!(ok.len(), 5);
    }

    #[tokio::test]
    async fn test_attachment_above_one_mib_rejected() {
        let resolvers = one_resolver(MAX_ATTACHMENT_BYTES + 1);
        let err = resolve_chat_attachments(&resolvers, Uuid::new_v4(), &ids(1))
            .await
            .unwrap_err();
        assert_eq!(err.code, Some(ATTACHMENT_TOO_LARGE));
        let at_limit = one_resolver(MAX_ATTACHMENT_BYTES);
        assert!(resolve_chat_attachments(&at_limit, Uuid::new_v4(), &ids(1))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_unknown_id_is_typed_error() {
        let resolvers = one_resolver(1);
        let err = resolve_chat_attachments(&resolvers, Uuid::new_v4(), &["unknown".to_string()])
            .await
            .unwrap_err();
        assert_eq!(err.code, Some(ATTACHMENT_UNRESOLVED));
    }
}
