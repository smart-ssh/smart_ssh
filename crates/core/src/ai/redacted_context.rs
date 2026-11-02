//! [`RedactedContext`]: the only request type an [`AiProvider`](super::AiProvider)
//! accepts (issue #833, security invariant 11).
//!
//! A raw [`SessionContext`] has public fields and can be built anywhere. This
//! wrapper has private fields and no `Deserialize`/`Default`, so a request can
//! only come from one of two constructors:
//!
//! - [`RedactedContext::seal`]: the general entry point. Seals extension
//!   parts, then re-applies redaction to every part of the history.
//! - [`RedactedContext::unredacted_command_for_second_opinion`]: the single
//!   named exception (see its doc comment).
//!
//! ```compile_fail
//! use ssh_manager_core::ai::{RedactedContext, SessionContext};
//! // private fields: no struct literal outside the core
//! let _ = RedactedContext {
//!     inner: SessionContext {
//!         system_context: String::new(),
//!         history: Vec::new(),
//!         available_actions: Vec::new(),
//!         max_tokens_hint: None,
//!     },
//! };
//! ```

use std::convert::Infallible;

use super::fence_markers;
use super::redactor::OutputRedactor;
use super::types::{
    ActionSchema, ChatMessage, MessageContent, SessionContext, WebActivity, WebSource,
};

/// Turns extension parts of a send copy into sealed content (in the app layer,
/// which owns the extension registry). It may only replace `Extension` parts;
/// whatever it returns is redacted again by [`RedactedContext::seal`], and
/// `Sealed` content is constructible only in the core.
pub trait ExtensionSealer {
    /// Why the send must stop (nothing is sent).
    type Error;

    fn seal_extensions(
        &self,
        history: Vec<ChatMessage>,
        redactor: &dyn OutputRedactor,
    ) -> Result<Vec<ChatMessage>, Self::Error>;
}

/// Sealer for requests that carry no extension content: history passes
/// through unchanged (extension parts, if any, still get the placeholder-only
/// label redaction in [`RedactedContext::seal`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExtensionSealer;

impl ExtensionSealer for NoExtensionSealer {
    type Error = Infallible;

    fn seal_extensions(
        &self,
        history: Vec<ChatMessage>,
        _redactor: &dyn OutputRedactor,
    ) -> Result<Vec<ChatMessage>, Infallible> {
        Ok(history)
    }
}

/// A provider request whose content went through the redaction and sealing
/// entry point. See the module documentation.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactedContext {
    inner: SessionContext,
}

impl RedactedContext {
    /// The general entry point: call it as the **last** step before
    /// `AiProvider::send`, after any app-authored instruction message was
    /// appended. Order: extension parts are sealed first, then every part of
    /// the history is redacted again (additive only, invariant 11).
    pub fn seal<S: ExtensionSealer + ?Sized>(
        context: SessionContext,
        redactor: &dyn OutputRedactor,
        sealer: &S,
    ) -> Result<Self, S::Error> {
        let SessionContext {
            system_context,
            history,
            available_actions,
            max_tokens_hint,
        } = context;
        let history = sealer.seal_extensions(history, redactor)?;
        Ok(Self {
            inner: SessionContext {
                // Deliberately not redacted (THREAT-MODEL.md §2).
                system_context,
                history: reapply_redaction_for_send(history, redactor),
                available_actions,
                max_tokens_hint,
            },
        })
    }

    /// The single named exception to [`RedactedContext::seal`]: the risk second
    /// opinion sends the command / path text **as is**, because redacting it
    /// would change what the model is asked to judge (THREAT-MODEL.md §1,
    /// "What is sent to the AI provider"). Only `second_opinion.rs` may call
    /// this; every other request goes through `seal`.
    pub fn unredacted_command_for_second_opinion(
        system_prompt: String,
        text: String,
        max_tokens_hint: Option<u32>,
    ) -> Self {
        Self {
            inner: SessionContext {
                system_context: system_prompt,
                history: vec![ChatMessage {
                    role: super::Role::User,
                    content: MessageContent::Text(text),
                }],
                available_actions: Vec::new(),
                max_tokens_hint,
            },
        }
    }

    /// Test-only: wraps a context without redaction.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests(context: SessionContext) -> Self {
        Self { inner: context }
    }

    pub fn system_context(&self) -> &str {
        &self.inner.system_context
    }

    pub fn history(&self) -> &[ChatMessage] {
        &self.inner.history
    }

    pub fn available_actions(&self) -> &[ActionSchema] {
        &self.inner.available_actions
    }

    pub fn max_tokens_hint(&self) -> Option<u32> {
        self.inner.max_tokens_hint
    }

    /// Read-only view as a [`SessionContext`] (for token estimates).
    pub fn as_session_context(&self) -> &SessionContext {
        &self.inner
    }

    /// Consumes the wrapper for the provider's request builder. One-way: there
    /// is no way back to a `RedactedContext`.
    pub fn into_parts(self) -> SessionContext {
        self.inner
    }
}

/// Issue #162: redacts every text field of a web research — the query and
/// URLs can carry secrets inserted by the AI, titles and page text come from
/// the open web. Runs before storing and again before every send.
pub fn redact_web_activity(activity: WebActivity, redactor: &dyn OutputRedactor) -> WebActivity {
    let redact_sources = |sources: Vec<WebSource>| {
        sources
            .into_iter()
            .map(|source| WebSource {
                title: redactor.redact_text(&source.title),
                url: redactor.redact_text(&source.url),
            })
            .collect()
    };
    WebActivity {
        kind: activity.kind,
        input: redactor.redact_text(&activity.input),
        results: redact_sources(activity.results),
        cited: redact_sources(activity.cited),
        content: activity.content.map(|c| redactor.redact_text(&c)),
        content_truncated: activity.content_truncated,
        error_code: activity.error_code,
    }
}

/// Splits `text` at every known fence marker, redacts only the segments in
/// between and re-inserts the markers unchanged, so a redaction pattern can
/// never match across a fence boundary. Without a marker this equals a single
/// `redactor.redact_text(text)`.
fn redact_text_preserving_fence_markers(text: &str, redactor: &dyn OutputRedactor) -> String {
    let markers = fence_markers();
    let mut result = String::new();
    let mut remaining = text;

    loop {
        let earliest = markers
            .iter()
            .filter_map(|marker| remaining.find(marker.as_str()).map(|idx| (idx, marker)))
            .min_by_key(|(idx, _)| *idx);

        match earliest {
            Some((idx, marker)) => {
                let (before, after) = remaining.split_at(idx);
                result.push_str(&redactor.redact_text(before));
                result.push_str(marker);
                remaining = &after[marker.len()..];
            }
            None => {
                result.push_str(&redactor.redact_text(remaining));
                break;
            }
        }
    }

    result
}

/// Nur-additive Re-Redaction der Historie unmittelbar vor dem Versand
/// (Spec 0040, Abschnitt 5; invariant 11). Exhaustive `match` over
/// `MessageContent`: a new variant forces a decision here.
///
/// Fencing safety: segments are redacted between fence markers only (see
/// [`redact_text_preserving_fence_markers`]). Known, deliberately unclosed
/// residual gap: plain chat text that happens to contain a marker-like
/// substring in the middle of an unterminated fail-safe match (e.g. a
/// truncated private-key block without `END`) is split at that marker, so the
/// part behind it is no longer matched. Redaction never reveals anything that
/// was hidden before; only its reach is smaller in this edge case.
pub fn reapply_redaction_for_send(
    history: Vec<ChatMessage>,
    redactor: &dyn OutputRedactor,
) -> Vec<ChatMessage> {
    history
        .into_iter()
        .map(|message| ChatMessage {
            role: message.role,
            content: match message.content {
                MessageContent::Text(text) => {
                    MessageContent::Text(redact_text_preserving_fence_markers(&text, redactor))
                }
                MessageContent::CommandResult {
                    command,
                    output,
                    cancelled,
                    waiting_for_input,
                } => MessageContent::CommandResult {
                    command,
                    output: redactor.redact(&output),
                    cancelled,
                    waiting_for_input,
                },
                other @ MessageContent::ActionRejected { .. } => other,
                // Issue #162: additive only; the web content is fenced when
                // the request is built (`WebActivity::to_model_text`).
                MessageContent::WebActivity(activity) => {
                    MessageContent::WebActivity(redact_web_activity(activity, redactor))
                }
                // Issue #504: the label goes into the fence, so it is
                // redacted again on every send.
                MessageContent::Extension(mut part) => {
                    part.label = redactor.redact_text(&part.label);
                    MessageContent::Extension(part)
                }
                // Issue #505: already sealed by the core.
                sealed @ MessageContent::Sealed(_) => sealed,
                // Issue #545: redacted again on every send (invariant 11).
                MessageContent::Question {
                    question_id,
                    question,
                    options,
                    multi_select,
                } => MessageContent::Question {
                    question_id,
                    question: redact_text_preserving_fence_markers(&question, redactor),
                    options: options
                        .into_iter()
                        .map(|o| crate::profiles::AskUserOption {
                            label: redact_text_preserving_fence_markers(&o.label, redactor),
                            description: o
                                .description
                                .map(|d| redact_text_preserving_fence_markers(&d, redactor)),
                        })
                        .collect(),
                    multi_select,
                },
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{DefaultOutputRedactor, Role};

    fn context(history: Vec<ChatMessage>) -> SessionContext {
        SessionContext {
            system_context: "system password=hunter2geheim".to_string(),
            history,
            available_actions: Vec::new(),
            max_tokens_hint: Some(7),
        }
    }

    fn text(t: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text(t.to_string()),
        }
    }

    /// A sealer that misbehaves and hands back raw text: whatever it returns
    /// is redacted afterwards (issue #833, order "seal, then re-redact").
    struct RawTextSealer;
    impl ExtensionSealer for RawTextSealer {
        type Error = ();
        fn seal_extensions(
            &self,
            _history: Vec<ChatMessage>,
            _redactor: &dyn OutputRedactor,
        ) -> Result<Vec<ChatMessage>, ()> {
            Ok(vec![text("leak password=hunter2geheim")])
        }
    }

    struct FailingSealer;
    impl ExtensionSealer for FailingSealer {
        type Error = &'static str;
        fn seal_extensions(
            &self,
            _history: Vec<ChatMessage>,
            _redactor: &dyn OutputRedactor,
        ) -> Result<Vec<ChatMessage>, &'static str> {
            Err("nope")
        }
    }

    #[test]
    fn test_seal_redacts_the_result_of_the_sealer() {
        let sealed = RedactedContext::seal(
            context(vec![text("x")]),
            &DefaultOutputRedactor::new(),
            &RawTextSealer,
        )
        .unwrap();
        let dump = format!("{:?}", sealed.history());
        assert!(!dump.contains("hunter2geheim"), "{dump}");
        assert_eq!(sealed.max_tokens_hint(), Some(7));
    }

    #[test]
    fn test_seal_keeps_system_context_unredacted_and_propagates_sealer_error() {
        let Ok(sealed) = RedactedContext::seal(
            context(Vec::new()),
            &DefaultOutputRedactor::new(),
            &NoExtensionSealer,
        );
        assert!(sealed.system_context().contains("hunter2geheim"));
        let failed = RedactedContext::seal(
            context(Vec::new()),
            &DefaultOutputRedactor::new(),
            &FailingSealer,
        );
        assert_eq!(failed.unwrap_err(), "nope");
    }

    #[test]
    fn test_second_opinion_constructor_sends_the_text_as_is() {
        let ctx = RedactedContext::unredacted_command_for_second_opinion(
            "prompt".to_string(),
            "curl -u a:password=hunter2geheim x".to_string(),
            Some(3),
        );
        assert_eq!(ctx.system_context(), "prompt");
        assert_eq!(ctx.history().len(), 1);
        assert!(format!("{:?}", ctx.history()).contains("hunter2geheim"));
        assert!(ctx.available_actions().is_empty());
    }
}
