//! Extension point for transforming approved text right before it is sent
//! to a server (issue #464, Spec 0038).
//!
//! A transform runs **after** filter, risk check, second opinion and
//! confirmation, on the final approved text, and immediately before the
//! transport call. Everything before that point (filter, risk, dialog,
//! ledger, result event, history, logs) sees only the original text. The
//! prepared text exists only for the transport call, so [`Prepared`] never
//! shows it in `Debug`.

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

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

/// Several transforms run as one (issue #893, Spec 0038).
///
/// They run in registration order; transform *n+1* receives the text produced
/// by transform *n* (same scope and kind). The inserted secrets of all
/// transforms are returned together, in order. The first [`Rejection`] stops
/// the chain and nothing of the partial result is returned.
/// `requires_confirmation` is `true` if any transform answers `true`
/// (fail-closed). The core's checks on the transformed text (hard blacklist,
/// redactor update, rejection redaction) apply to the final text, once.
pub struct CompositeOutgoingTransform {
    transforms: Vec<Arc<dyn OutgoingTransform>>,
}

impl CompositeOutgoingTransform {
    pub fn new(transforms: Vec<Arc<dyn OutgoingTransform>>) -> Self {
        Self { transforms }
    }
}

impl std::fmt::Debug for CompositeOutgoingTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeOutgoingTransform")
            .field("transforms", &self.transforms.len())
            .finish()
    }
}

#[async_trait]
impl OutgoingTransform for CompositeOutgoingTransform {
    async fn prepare_outgoing(
        &self,
        scope: &ServerScope,
        kind: OutgoingKind,
        text: &str,
    ) -> Result<Prepared, Rejection> {
        let mut current = SecretString::from(text.to_string());
        let mut secrets: Vec<SecretString> = Vec::new();
        for transform in &self.transforms {
            let prepared = transform
                .prepare_outgoing(scope, kind, current.expose_secret())
                .await?;
            current = prepared.text;
            secrets.extend(prepared.secrets);
        }
        Ok(Prepared {
            text: current,
            secrets,
        })
    }

    /// `true` if any transform answers `true`. Each one is asked about the
    /// original text (a pure query cannot see its predecessor's output).
    fn requires_confirmation(&self, scope: &ServerScope, kind: OutgoingKind, text: &str) -> bool {
        self.transforms
            .iter()
            .any(|t| t.requires_confirmation(scope, kind, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Step {
        from: &'static str,
        to: &'static str,
        secret: &'static str,
        confirm: bool,
        reject: bool,
        calls: AtomicUsize,
        seen: std::sync::Mutex<Vec<String>>,
    }

    impl Step {
        fn new(from: &'static str, to: &'static str, secret: &'static str) -> Arc<Self> {
            Arc::new(Self {
                from,
                to,
                secret,
                confirm: false,
                reject: false,
                calls: AtomicUsize::new(0),
                seen: Default::default(),
            })
        }
        fn with(from: &'static str, confirm: bool, reject: bool) -> Arc<Self> {
            Arc::new(Self {
                from,
                to: "",
                secret: "",
                confirm,
                reject,
                calls: AtomicUsize::new(0),
                seen: Default::default(),
            })
        }
    }

    #[async_trait]
    impl OutgoingTransform for Step {
        async fn prepare_outgoing(
            &self,
            _scope: &ServerScope,
            _kind: OutgoingKind,
            text: &str,
        ) -> Result<Prepared, Rejection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().unwrap().push(text.to_string());
            if self.reject {
                return Err(Rejection {
                    message: format!("rejected by {}", self.from),
                });
            }
            Ok(Prepared::new(
                text.replace(self.from, self.to),
                vec![SecretString::from(self.secret.to_string())],
            ))
        }
        fn requires_confirmation(&self, _: &ServerScope, _: OutgoingKind, _: &str) -> bool {
            self.confirm
        }
    }

    fn scope() -> ServerScope {
        ServerScope {
            server_id: ServerId::default(),
            group_chain: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn composite(steps: &[&Arc<Step>]) -> CompositeOutgoingTransform {
        CompositeOutgoingTransform::new(
            steps
                .iter()
                .map(|s| Arc::clone(s) as Arc<dyn OutgoingTransform>)
                .collect(),
        )
    }

    #[tokio::test]
    async fn test_composite_chains_output_and_unions_secrets() {
        let a = Step::new("AAA", "BBB", "secret-one");
        let b = Step::new("BBB", "CCC", "secret-two");
        let c = composite(&[&a, &b]);
        let out = c
            .prepare_outgoing(&scope(), OutgoingKind::Command, "echo AAA")
            .await
            .unwrap();
        assert_eq!(b.seen.lock().unwrap().as_slice(), ["echo BBB"]);
        assert_eq!(out.text().expose_secret(), "echo CCC");
        let secrets: Vec<&str> = out.secrets().iter().map(|s| s.expose_secret()).collect();
        assert_eq!(secrets, ["secret-one", "secret-two"]);
        let shown = format!("{out:?} {c:?}");
        for needle in ["secret-one", "secret-two", "echo", "CCC"] {
            assert!(!shown.contains(needle), "{shown}");
        }
    }

    #[tokio::test]
    async fn test_composite_first_rejection_stops_chain() {
        let a = Step::with("first", false, true);
        let b = Step::new("x", "y", "s");
        let err = composite(&[&a, &b])
            .prepare_outgoing(&scope(), OutgoingKind::Command, "x")
            .await
            .unwrap_err();
        assert_eq!(err.message, "rejected by first");
        assert_eq!(b.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_composite_second_rejection_is_returned() {
        let a = Step::new("x", "y", "s");
        let b = Step::with("second", false, true);
        let err = composite(&[&a, &b])
            .prepare_outgoing(&scope(), OutgoingKind::Command, "x")
            .await
            .unwrap_err();
        assert_eq!(err.message, "rejected by second");
    }

    #[test]
    fn test_composite_requires_confirmation_if_any_does() {
        let yes = Step::with("a", true, false);
        let no = Step::with("a", false, false);
        let q = |c: &CompositeOutgoingTransform| {
            c.requires_confirmation(&scope(), OutgoingKind::Command, "t")
        };
        assert!(q(&composite(&[&yes, &no])));
        assert!(q(&composite(&[&no, &yes])));
        assert!(q(&composite(&[&yes, &yes])));
        assert!(!q(&composite(&[&no, &no])));
    }

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
