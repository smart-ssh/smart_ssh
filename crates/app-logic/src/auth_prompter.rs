//! Issue #1185 (Spec 0005, "Interactive and multi-step login"): the
//! [`AuthPrompter`] of interactive connects. It shows the server's login
//! questions to the user through events and waits for
//! `answer_auth_prompt` / `cancel_auth_prompt`.
//!
//! The answers are secrets: they travel only in the one answer command,
//! have a redacting `Debug`, are never logged and are dropped by the
//! transport right after sending.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use secrecy::SecretString;
use serde::Deserialize;
use ssh_manager_core::ssh::{
    AuthPromptKind, AuthPromptRequest, AuthPrompter, PromptCancelled, AUTH_PROMPT_TIMEOUT,
};

use crate::auth_prompt_events::{
    emit_auth_prompt_ended, emit_auth_prompt_needed, AuthPromptEndReason,
};
use crate::confirmation::ConfirmationRegistry;
use crate::events::EventEmitter;

/// Registry of the login prompts the user has not answered yet, by prompt id.
pub type AuthPromptRegistry = ConfirmationRegistry<String, AuthPromptResponse>;

/// Code of the error `answer_auth_prompt` / `cancel_auth_prompt` return when
/// no prompt with that id is waiting (answered, expired or aborted).
pub const AUTH_PROMPT_NOT_PENDING: &str = "AUTH_PROMPT_NOT_PENDING";

/// The answers typed by the user, in prompt order. Secret: `Debug` prints
/// only the count.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct AuthPromptAnswers(Vec<String>);

impl AuthPromptAnswers {
    pub fn new(answers: Vec<String>) -> Self {
        Self(answers)
    }
}

impl fmt::Debug for AuthPromptAnswers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AuthPromptAnswers([REDACTED; {}])", self.0.len())
    }
}

#[derive(Debug)]
pub enum AuthPromptResponse {
    Answers(AuthPromptAnswers),
    Cancel,
}

/// Resolves a waiting prompt; the error is the stable code
/// [`AUTH_PROMPT_NOT_PENDING`].
pub fn resolve_auth_prompt(
    registry: &AuthPromptRegistry,
    prompt_id: &str,
    response: AuthPromptResponse,
) -> Result<(), &'static str> {
    registry
        .resolve(&prompt_id.to_string(), response)
        .map_err(|_| AUTH_PROMPT_NOT_PENDING)
}

/// Emits a prompt event per request and waits for the user.
pub struct EventAuthPrompter {
    registry: Arc<AuthPromptRegistry>,
    emitter: Arc<dyn EventEmitter>,
    timeout: std::time::Duration,
}

impl EventAuthPrompter {
    pub fn new(registry: Arc<AuthPromptRegistry>, emitter: Arc<dyn EventEmitter>) -> Self {
        Self::with_timeout(registry, emitter, AUTH_PROMPT_TIMEOUT)
    }

    pub fn with_timeout(
        registry: Arc<AuthPromptRegistry>,
        emitter: Arc<dyn EventEmitter>,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            registry,
            emitter,
            timeout,
        }
    }
}

/// Ends the prompt on every exit, including the future being dropped (the
/// connect was cancelled): removes the registration and tells the frontend
/// to close the dialog.
struct PromptGuard<'a> {
    prompter: &'a EventAuthPrompter,
    id: String,
    reason: AuthPromptEndReason,
}

impl Drop for PromptGuard<'_> {
    fn drop(&mut self) {
        self.prompter.registry.cancel(&self.id);
        emit_auth_prompt_ended(self.prompter.emitter.as_ref(), &self.id, self.reason);
    }
}

#[async_trait]
impl AuthPrompter for EventAuthPrompter {
    async fn prompt(
        &self,
        request: AuthPromptRequest,
    ) -> Result<Vec<SecretString>, PromptCancelled> {
        let expected = match &request.kind {
            AuthPromptKind::KeyboardInteractive { prompts, .. } => prompts.len(),
            AuthPromptKind::Password => 1,
        };
        let id = uuid::Uuid::new_v4().to_string();
        let rx = self.registry.register(id.clone());
        let mut guard = PromptGuard {
            prompter: self,
            id: id.clone(),
            reason: AuthPromptEndReason::Abandoned,
        };
        // Only the id and the hop are logged — never texts or answers.
        tracing::info!(prompt_id = %id, "login prompt shown");
        emit_auth_prompt_needed(self.emitter.as_ref(), &id, &request);
        let outcome = tokio::time::timeout(self.timeout, rx).await;
        match outcome {
            Err(_) => {
                guard.reason = AuthPromptEndReason::TimedOut;
                Err(PromptCancelled)
            }
            Ok(Err(_)) => {
                guard.reason = AuthPromptEndReason::Abandoned;
                Err(PromptCancelled)
            }
            Ok(Ok(AuthPromptResponse::Cancel)) => {
                guard.reason = AuthPromptEndReason::Cancelled;
                Err(PromptCancelled)
            }
            Ok(Ok(AuthPromptResponse::Answers(answers))) => {
                guard.reason = AuthPromptEndReason::Answered;
                if answers.0.len() != expected {
                    // Fail closed: answers that do not match the questions
                    // are never sent.
                    tracing::warn!(prompt_id = %id, "login prompt answered with the wrong count");
                    return Err(PromptCancelled);
                }
                Ok(answers.0.into_iter().map(SecretString::from).collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct Collect(Mutex<Vec<(String, serde_json::Value)>>);
    impl EventEmitter for Collect {
        fn emit_event(&self, event: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().push((event.to_string(), payload));
        }
    }

    fn request() -> AuthPromptRequest {
        AuthPromptRequest::keyboard_interactive(
            "u@h:22",
            "n",
            "<b>i</b>",
            &[("Password: ".into(), false), ("Code: ".into(), true)],
        )
        .unwrap()
    }

    fn setup() -> (
        Arc<AuthPromptRegistry>,
        Arc<Collect>,
        Arc<EventAuthPrompter>,
    ) {
        let reg = Arc::new(AuthPromptRegistry::new());
        let em = Arc::new(Collect::default());
        let p = Arc::new(EventAuthPrompter::new(reg.clone(), em.clone()));
        (reg, em, p)
    }

    fn first_id(em: &Collect) -> String {
        em.0.lock().unwrap()[0].1["promptId"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn test_answers_reach_the_prompter_and_event_has_no_secret() {
        let (reg, em, p) = setup();
        let task = tokio::spawn({
            let p = p.clone();
            async move { p.prompt(request()).await }
        });
        while em.0.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let id = first_id(&em);
        resolve_auth_prompt(
            &reg,
            &id,
            AuthPromptResponse::Answers(AuthPromptAnswers::new(vec![
                "pw-secret".into(),
                "123".into(),
            ])),
        )
        .unwrap();
        let answers = task.await.unwrap().unwrap();
        assert_eq!(answers[0].expose_secret(), "pw-secret");
        assert_eq!(answers[1].expose_secret(), "123");
        let events = em.0.lock().unwrap();
        let all = format!("{events:?}");
        assert!(!all.contains("pw-secret"));
        assert_eq!(events[0].0, "auth-prompt-needed");
        assert_eq!(events[0].1["prompts"][0]["echo"], false);
        assert_eq!(events[0].1["prompts"][1]["echo"], true);
        assert_eq!(events[0].1["instructions"], "<b>i</b>");
        assert_eq!(events[1].0, "auth-prompt-ended");
        assert_eq!(events[1].1["reason"], "answered");
        assert!(!reg.contains(&id));
    }

    #[tokio::test]
    async fn test_cancel_and_wrong_count_and_unknown_id_fail_closed() {
        let (reg, em, p) = setup();
        let task = tokio::spawn({
            let p = p.clone();
            async move { p.prompt(request()).await }
        });
        while em.0.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let id = first_id(&em);
        resolve_auth_prompt(&reg, &id, AuthPromptResponse::Cancel).unwrap();
        assert!(task.await.unwrap().is_err());
        assert_eq!(em.0.lock().unwrap()[1].1["reason"], "cancelled");
        assert_eq!(
            resolve_auth_prompt(&reg, &id, AuthPromptResponse::Cancel),
            Err(AUTH_PROMPT_NOT_PENDING)
        );

        let (reg, em, p) = setup();
        let task = tokio::spawn({
            let p = p.clone();
            async move { p.prompt(request()).await }
        });
        while em.0.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let id = first_id(&em);
        resolve_auth_prompt(
            &reg,
            &id,
            AuthPromptResponse::Answers(AuthPromptAnswers::new(vec!["only-one".into()])),
        )
        .unwrap();
        assert!(task.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn test_timeout_and_drop_end_the_prompt() {
        let reg = Arc::new(AuthPromptRegistry::new());
        let em = Arc::new(Collect::default());
        let p = EventAuthPrompter::with_timeout(reg.clone(), em.clone(), Duration::from_millis(20));
        assert!(p.prompt(request()).await.is_err());
        assert_eq!(em.0.lock().unwrap()[1].1["reason"], "timed_out");

        let (reg, em, p) = setup();
        let fut = p.prompt(request());
        let _ = tokio::time::timeout(Duration::from_millis(20), fut).await;
        let id = first_id(&em);
        assert!(!reg.contains(&id));
        assert_eq!(em.0.lock().unwrap()[1].1["reason"], "abandoned");
    }

    #[test]
    fn test_answers_debug_is_redacted() {
        let a = AuthPromptAnswers::new(vec!["hunter2".into()]);
        let first = format!("{a:?}");
        let shown = format!("{first} {:?}", AuthPromptResponse::Answers(a));
        assert!(!shown.contains("hunter2"));
        assert!(shown.contains("REDACTED"));
    }
}
