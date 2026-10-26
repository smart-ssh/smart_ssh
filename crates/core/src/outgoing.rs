//! Extension point for transforming approved text right before it is sent
//! to a server (issue #464, Spec 0038).
//!
//! A transform runs **after** filter, risk check, second opinion and
//! confirmation, on the final approved text, and immediately before the
//! transport call. Everything before that point (filter, risk, dialog,
//! ledger, result event, history, logs) sees only the original text. The
//! prepared text exists only for the transport call, so [`Prepared`] never
//! shows it in `Debug`.

use async_trait::async_trait;
use secrecy::SecretString;

use crate::profiles::GroupId;
use crate::shared::ServerId;

/// What kind of text is about to be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutgoingKind {
    /// A shell command (AI-proposed or MCP-proposed).
    Command,
    /// The content of an AI `WriteRemoteFile` action.
    FileContent,
}

/// The server an extension point is asked about. Shared by the session
/// extension and the outgoing transform so both describe the same target
/// server the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerScope {
    /// The server id; the local pseudo-server carries the nil id.
    pub server_id: ServerId,
    /// The group chain of the server, root first. Empty for servers
    /// without a group and for the local pseudo-server.
    pub group_chain: Vec<GroupId>,
    pub tags: Vec<String>,
}

/// The result of a transform: the text to send and the secret values it
/// inserted. The secret values are added to the session's redactor before the
/// command starts. `Debug` shows neither the text nor the values.
pub struct Prepared {
    text: SecretString,
    secrets: Vec<SecretString>,
}

impl Prepared {
    pub fn new(text: String, secrets: Vec<SecretString>) -> Self {
        Self {
            text: SecretString::from(text),
            secrets,
        }
    }

    /// The text to send.
    pub fn text(&self) -> &SecretString {
        &self.text
    }

    /// The secret values the transform inserted into the text.
    pub fn secrets(&self) -> &[SecretString] {
        &self.secrets
    }
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("text", &"[REDACTED]")
            .field("secrets", &self.secrets.len())
            .finish()
    }
}

/// The transform refuses to prepare the text. Nothing is sent; `message` is
/// returned to the AI as the action result and must not contain secret values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub message: String,
}

/// A trusted edition extension, registered via the app wiring. Without one,
/// the text is sent unchanged.
#[async_trait]
pub trait OutgoingTransform: Send + Sync {
    async fn prepare_outgoing(
        &self,
        scope: &ServerScope,
        kind: OutgoingKind,
        text: &str,
    ) -> Result<Prepared, Rejection>;

    /// Pure query: will `prepare_outgoing` change `text` (e.g. insert stored
    /// secret values)? `true` makes the core require a confirmation for an
    /// action that would otherwise run without one. No side effects; must
    /// neither see nor return secret values. Default: `false` (never asks).
    fn requires_confirmation(
        &self,
        _scope: &ServerScope,
        _kind: OutgoingKind,
        _text: &str,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_of_prepared_shows_neither_text_nor_secrets() {
        let prepared = Prepared::new(
            "echo hunter2".to_string(),
            vec![SecretString::from("hunter2".to_string())],
        );
        let shown = format!("{prepared:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(!shown.contains("echo"), "{shown}");
    }
}
