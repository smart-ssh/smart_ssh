//! Issue #1184 (Spec 0005, "Interactive and multi-step login"): the
//! interface through which the transport asks the user for answers during
//! `keyboard-interactive` and multi-step logins.
//!
//! The request carries what the server asked and which hop asks — never a
//! secret. The answers travel back as [`SecretString`] and are dropped by
//! the transport right after sending. Prompt texts come from the server and
//! are untrusted: [`AuthPromptRequest::keyboard_interactive`] reduces them
//! to plain, length-capped text.

use std::time::Duration;

use async_trait::async_trait;
use secrecy::SecretString;

/// Longest prompt, name or instruction text kept, in bytes (1 KiB).
pub const MAX_PROMPT_TEXT_BYTES: usize = 1024;
/// Most prompts accepted in one info request.
pub const MAX_PROMPTS_PER_REQUEST: usize = 16;
/// A prompter that has not answered after this long counts as cancelled.
pub const AUTH_PROMPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// One question of a `keyboard-interactive` info request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPromptItem {
    pub text: String,
    /// `true` when the answer may be shown while typing (a one-time code),
    /// `false` for a password.
    pub echo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPromptKind {
    KeyboardInteractive {
        name: String,
        instructions: String,
        prompts: Vec<AuthPromptItem>,
    },
    /// The password of the hop, asked for a multi-step login (`publickey,password`).
    Password,
}

/// What the user is asked. Contains nothing secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPromptRequest {
    /// `user@host:port` of the hop that asks.
    pub hop: String,
    pub kind: AuthPromptKind,
}

impl AuthPromptRequest {
    pub fn password(hop: impl Into<String>) -> Self {
        Self {
            hop: hop.into(),
            kind: AuthPromptKind::Password,
        }
    }

    /// Builds the request from server-supplied texts. Returns `None` when the
    /// server sent more than [`MAX_PROMPTS_PER_REQUEST`] prompts (the
    /// answers could not be matched to truncated prompts).
    pub fn keyboard_interactive(
        hop: impl Into<String>,
        name: &str,
        instructions: &str,
        prompts: &[(String, bool)],
    ) -> Option<Self> {
        if prompts.len() > MAX_PROMPTS_PER_REQUEST {
            return None;
        }
        Some(Self {
            hop: hop.into(),
            kind: AuthPromptKind::KeyboardInteractive {
                name: sanitize_server_text(name),
                instructions: sanitize_server_text(instructions),
                prompts: prompts
                    .iter()
                    .map(|(text, echo)| AuthPromptItem {
                        text: sanitize_server_text(text),
                        echo: *echo,
                    })
                    .collect(),
            },
        })
    }
}

/// Plain text only: control characters (except line breaks and tabs, which
/// become spaces/line breaks) are dropped, the result is cut at
/// [`MAX_PROMPT_TEXT_BYTES`] on a character boundary.
pub fn sanitize_server_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = match c {
            '\n' | '\r' => '\n',
            '\t' => ' ',
            c if c.is_control() => continue,
            c => c,
        };
        if out.len() + c.len_utf8() > MAX_PROMPT_TEXT_BYTES {
            break;
        }
        out.push(c);
    }
    out
}

/// The prompter gave up (the user cancelled, nobody can be asked, or
/// [`AUTH_PROMPT_TIMEOUT`] passed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptCancelled;

/// Asks the user for answers during login. One answer per prompt, in order
/// (exactly one for [`AuthPromptKind::Password`]).
#[async_trait]
pub trait AuthPrompter: Send + Sync {
    async fn prompt(
        &self,
        request: AuthPromptRequest,
    ) -> Result<Vec<SecretString>, PromptCancelled>;
}

/// For callers without a user interface: cancels at once.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPrompter;

#[async_trait]
impl AuthPrompter for NoPrompter {
    async fn prompt(
        &self,
        _request: AuthPromptRequest,
    ) -> Result<Vec<SecretString>, PromptCancelled> {
        Err(PromptCancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_text_is_plain_and_capped() {
        assert_eq!(sanitize_server_text("a\x1b[31mb\x07c"), "a[31mbc");
        let long = "ä".repeat(2000);
        let cut = sanitize_server_text(&long);
        assert!(cut.len() <= MAX_PROMPT_TEXT_BYTES);
        assert!(cut.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn test_too_many_prompts_are_refused() {
        let many: Vec<(String, bool)> = (0..17).map(|i| (format!("p{i}"), false)).collect();
        assert!(AuthPromptRequest::keyboard_interactive("h", "", "", &many).is_none());
        assert!(AuthPromptRequest::keyboard_interactive("h", "", "", &many[..16]).is_some());
    }

    #[tokio::test]
    async fn test_no_prompter_cancels_immediately() {
        let r = NoPrompter.prompt(AuthPromptRequest::password("h")).await;
        assert_eq!(r.unwrap_err(), PromptCancelled);
    }
}
