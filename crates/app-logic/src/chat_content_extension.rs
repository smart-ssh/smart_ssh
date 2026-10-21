//! Extension point for content an edition attaches to user chat messages
//! (Issue #504, ADR 0141, Spec 0110 §5, Spec 0038 §2a).
//!
//! The core stores only an opaque reference ([`ExtensionPart`]) in the chat
//! history and never learns the extension's data model. This module holds the
//! trait an edition implements, the registry that validates names, and the
//! removal callbacks: [`ChatContentExtension::session_removed`] after every
//! removed chat session and [`ChatContentExtension::reconcile`] once per
//! start.
//!
//! Security: nothing an extension supplies reaches a provider unfenced. The
//! core redacts and fences extension content itself; an extension picks only a
//! source from the closed [`ExtensionSource`] list.

use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use persistence_sqlite::ChatSessionRemovalObserver;
use ssh_manager_core::ai::{
    ChatMessage, ContentLimitError, ExtensionPart, ExtensionSource, MessageContent, OutputRedactor,
    SealedContent, MAX_EXTENSION_PARTS_PER_MESSAGE, MAX_REQUEST_BINARY_BYTES,
};

pub use ssh_manager_core::ai::{RedactedBlock, ResolvedBlock};

/// Content behind an opaque id from the composer, before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingContent {
    pub label: String,
    pub kind: String,
    pub blocks: Vec<ResolvedBlock>,
}

/// An extension failed to deliver content; the core does not send (fail
/// closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatContentError(pub String);

/// Registered through `Wiring`. All methods have no-op defaults.
///
/// Every `session_id` passed to a hook is the **saved chat session id** (the
/// id `session_removed` and `reconcile` already use), not the id of the open
/// tab: it stays the same after the chat is resumed under a new tab and
/// changes with "New Chat". Only a connection without a saved chat (chat
/// persistence off) passes the tab id.
pub trait ChatContentExtension: Send + Sync {
    /// Registered name: `[a-z0-9]+`, unique.
    fn name(&self) -> &str {
        ""
    }

    /// Source of the content; selects the fence kind.
    fn source(&self) -> ExtensionSource {
        ExtensionSource::LocalFile
    }

    /// Translates an opaque composer id (never a path) into label and
    /// blocks. `None` means "not my id".
    fn take_pending(&self, _session_id: Uuid, _id: &str) -> Option<PendingContent> {
        None
    }

    /// Stores what the core hands back: text already redacted, binary
    /// checked. An error means nothing is sent.
    fn persist(
        &self,
        _session_id: Uuid,
        _part_id: &str,
        _blocks: Vec<RedactedBlock>,
    ) -> Result<(), ChatContentError> {
        Ok(())
    }

    /// Content for a send. `Ok(None)` means the content no longer exists.
    fn resolve(
        &self,
        _session_id: Uuid,
        _part: &ExtensionPart,
    ) -> Result<Option<Vec<ResolvedBlock>>, ChatContentError> {
        Ok(None)
    }

    /// A chat session was removed (called after the delete succeeded).
    fn session_removed(&self, _session_id: Uuid) {}

    /// Called once per start with the ids of all existing chat sessions, so
    /// leftovers of a crash between delete and callback disappear.
    fn reconcile(&self, _live_session_ids: &[Uuid]) {}
}

/// Valid extension name: non-empty, lowercase ASCII letters and digits.
pub fn is_valid_extension_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// The validated list of registered extensions.
#[derive(Default)]
pub struct ChatContentRegistry {
    extensions: Vec<Arc<dyn ChatContentExtension>>,
}

impl ChatContentRegistry {
    /// Skips (and logs, without the offending value) every extension whose
    /// name is invalid or already taken.
    pub fn new(candidates: Vec<Arc<dyn ChatContentExtension>>) -> Self {
        let mut seen: HashSet<String> = HashSet::new();
        let mut extensions = Vec::new();
        for (index, ext) in candidates.into_iter().enumerate() {
            let name = ext.name();
            if !is_valid_extension_name(name) {
                tracing::warn!(index, "chat content extension skipped: invalid name");
                continue;
            }
            if !seen.insert(name.to_string()) {
                tracing::warn!(index, "chat content extension skipped: duplicate name");
                continue;
            }
            extensions.push(ext);
        }
        Self { extensions }
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    pub fn extensions(&self) -> &[Arc<dyn ChatContentExtension>] {
        &self.extensions
    }

    /// The registered extension with this name.
    pub fn find(&self, name: &str) -> Option<&Arc<dyn ChatContentExtension>> {
        self.extensions.iter().find(|e| e.name() == name)
    }

    /// Calls `reconcile` once per extension.
    pub fn reconcile(&self, live_session_ids: &[Uuid]) {
        for ext in &self.extensions {
            ext.reconcile(live_session_ids);
        }
    }
}

impl ChatSessionRemovalObserver for ChatContentRegistry {
    fn sessions_removed(&self, session_ids: &[Uuid]) {
        for id in session_ids {
            for ext in &self.extensions {
                ext.session_removed(*id);
            }
        }
    }
}

/// Stable error code of a violated core limit.
pub fn limit_error_code(err: &ContentLimitError) -> &'static str {
    use crate::orchestration::{
        ATTACHMENTS_REQUEST_TOO_LARGE, ATTACHMENTS_TOO_MANY, ATTACHMENT_INVALID,
        ATTACHMENT_TOO_LARGE,
    };
    match err {
        ContentLimitError::TooManyParts { .. } | ContentLimitError::TooManyBlocks { .. } => {
            ATTACHMENTS_TOO_MANY
        }
        ContentLimitError::BlockTooLarge { .. } => ATTACHMENT_TOO_LARGE,
        ContentLimitError::MimeNotAllowed { .. } | ContentLimitError::MimeMismatch { .. } => {
            ATTACHMENT_INVALID
        }
        ContentLimitError::RequestTooLarge { .. } => ATTACHMENTS_REQUEST_TOO_LARGE,
    }
}

/// Why a send was stopped while sealing extension content. Nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealError {
    pub code: &'static str,
    pub message: String,
}

impl From<ContentLimitError> for SealError {
    fn from(err: ContentLimitError) -> Self {
        Self {
            code: limit_error_code(&err),
            message: format!("The message was not sent: {err}"),
        }
    }
}

/// Replaces every `Extension` part of the send copy by a [`SealedContent`]
/// (ADR 0141, decision 3). The single place where extension content enters a
/// request: limits are re-checked on every send, `resolve` runs per part, text
/// is redacted again and fenced. `resolve` -> `None`, or an extension that is
/// not registered, yields the core placeholder; a `resolve` error stops the
/// send.
pub fn seal_extension_parts(
    history: Vec<ChatMessage>,
    registry: &ChatContentRegistry,
    session_id: Uuid,
    redactor: &dyn OutputRedactor,
) -> Result<Vec<ChatMessage>, SealError> {
    let mut run = 0usize;
    let mut binary_total = 0usize;
    let mut out = Vec::with_capacity(history.len());
    for message in history {
        let MessageContent::Extension(part) = message.content else {
            run = 0;
            out.push(message);
            continue;
        };
        run += 1;
        if run > MAX_EXTENSION_PARTS_PER_MESSAGE {
            return Err(ContentLimitError::TooManyParts { count: run }.into());
        }
        let sealed = match registry.find(&part.extension) {
            Some(ext) => {
                let content = ext.resolve(session_id, &part).map_err(|_| SealError {
                    code: crate::orchestration::ATTACHMENT_UNRESOLVED,
                    message: "The message was not sent: attached content could not be read."
                        .to_string(),
                })?;
                SealedContent::seal(&part, ext.source(), content, redactor)?
            }
            None => SealedContent::seal(&part, ExtensionSource::LocalFile, None, redactor)?,
        };
        binary_total += sealed.binary_bytes();
        if binary_total > MAX_REQUEST_BINARY_BYTES {
            return Err(ContentLimitError::RequestTooLarge {
                bytes: binary_total,
            }
            .into());
        }
        out.push(ChatMessage {
            role: message.role,
            content: MessageContent::Sealed(sealed),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Named(&'static str);
    impl ChatContentExtension for Named {
        fn name(&self) -> &str {
            self.0
        }
    }

    #[derive(Default)]
    struct Recorder {
        removed: Mutex<Vec<Uuid>>,
        reconciled: Mutex<Vec<Vec<Uuid>>>,
    }
    struct Rec(&'static str, Arc<Recorder>);
    impl ChatContentExtension for Rec {
        fn name(&self) -> &str {
            self.0
        }
        fn session_removed(&self, id: Uuid) {
            self.1.removed.lock().unwrap().push(id);
        }
        fn reconcile(&self, live: &[Uuid]) {
            self.1.reconciled.lock().unwrap().push(live.to_vec());
        }
    }

    #[test]
    fn invalid_and_duplicate_names_are_skipped() {
        let registry = ChatContentRegistry::new(vec![
            Arc::new(Named("files")),
            Arc::new(Named("Files")),
            Arc::new(Named("")),
            Arc::new(Named("my-ext")),
            Arc::new(Named("a b")),
            Arc::new(Named("files")),
            Arc::new(Named("docs2")),
        ]);
        let names: Vec<&str> = registry.extensions().iter().map(|e| e.name()).collect();
        assert_eq!(names, vec!["files", "docs2"]);
    }

    #[test]
    fn empty_registry_is_empty() {
        assert!(ChatContentRegistry::default().is_empty());
    }

    #[test]
    fn removal_and_reconcile_reach_every_extension() {
        let rec = Arc::new(Recorder::default());
        let registry = ChatContentRegistry::new(vec![
            Arc::new(Rec("one", rec.clone())),
            Arc::new(Rec("two", rec.clone())),
        ]);
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        registry.sessions_removed(&[a, b]);
        assert_eq!(rec.removed.lock().unwrap().len(), 4);
        registry.reconcile(&[a]);
        assert_eq!(*rec.reconciled.lock().unwrap(), vec![vec![a], vec![a]]);
    }
}
