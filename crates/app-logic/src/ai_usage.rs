//! Extension point: observe the AI usage of every completed AI request of a
//! session (Issue #610, Spec 0038 section 2).
//!
//! An edition registers [`AiUsageObserver`]s through `Wiring`. Whenever an AI
//! request that belongs to a session reports its [`AiUsage`], every observer
//! is told once. The observer is strictly passive: it returns nothing and has
//! no influence on filter, confirmation or execution. The record carries ids,
//! the model and the numbers only, never prompt, response or command content.
//! The core itself stores nothing.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ssh_manager_core::ai::{AiUsage, ProviderType};

/// Why the request was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiUsagePurpose {
    ChatTurn,
    RiskSecondOpinion,
    InjectionCheck,
    SessionTitle,
    NoteSuggestion,
    Other,
}

/// One completed AI request of a session. No content fields.
#[derive(Debug, Clone, PartialEq)]
pub struct AiUsageRecord {
    /// `None` for the local pseudo-server and for sessions without a saved chat.
    pub chat_session_id: Option<String>,
    pub runtime_session_id: String,
    /// `None` for the local pseudo-server.
    pub server_id: Option<String>,
    pub provider_config_id: String,
    pub provider_type: ProviderType,
    pub configured_model: String,
    pub purpose: AiUsagePurpose,
    pub usage: AiUsage,
    pub completed_at: DateTime<Utc>,
}

/// Registered through `Wiring`; called after a request completed, off the
/// chat flow. A panic is caught and logged; the chat turn is not affected.
pub trait AiUsageObserver: Send + Sync {
    fn on_usage(&self, record: &AiUsageRecord);
}

/// Which configured provider served a request.
#[derive(Debug, Clone)]
pub struct AiUsageSource {
    pub provider_config_id: String,
    pub provider_type: ProviderType,
    pub configured_model: String,
}

/// Per-session reporting setup (set once when the session is built).
#[derive(Clone)]
pub struct AiUsageReporting {
    pub observers: Arc<Vec<Arc<dyn AiUsageObserver>>>,
    pub runtime_session_id: String,
    pub main: AiUsageSource,
    /// Provider of the second-opinion and injection-check side calls.
    pub side_call: Option<AiUsageSource>,
}

impl AiUsageReporting {
    pub(crate) fn source_for(&self, purpose: AiUsagePurpose) -> Option<&AiUsageSource> {
        match purpose {
            AiUsagePurpose::RiskSecondOpinion | AiUsagePurpose::InjectionCheck => {
                self.side_call.as_ref()
            }
            _ => Some(&self.main),
        }
    }
}

/// Hands the record to every observer on the blocking pool, so a slow
/// observer neither delays the caller nor blocks the async runtime. A
/// panicking observer is logged (no content) and does not stop the others.
pub(crate) fn notify(observers: &Arc<Vec<Arc<dyn AiUsageObserver>>>, record: AiUsageRecord) {
    if observers.is_empty() {
        return;
    }
    let observers = Arc::clone(observers);
    let work = move || {
        for observer in observers.iter() {
            if catch_unwind(AssertUnwindSafe(|| observer.on_usage(&record))).is_err() {
                tracing::warn!("an AI usage observer panicked");
            }
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            drop(handle.spawn_blocking(work));
        }
        Err(_) => {
            std::thread::spawn(work);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time guard: adding a field breaks this destructuring, so a
    /// content field cannot slip in unnoticed. Strings are ids/model only.
    #[test]
    fn test_record_has_no_content_fields() {
        let AiUsageRecord {
            chat_session_id: _chat_session_id,
            runtime_session_id: _runtime_session_id,
            server_id: _server_id,
            provider_config_id: _provider_config_id,
            provider_type: _provider_type,
            configured_model: _configured_model,
            purpose: _purpose,
            usage: _usage,
            completed_at: _completed_at,
        } = AiUsageRecord {
            chat_session_id: None,
            runtime_session_id: String::new(),
            server_id: None,
            provider_config_id: String::new(),
            provider_type: ProviderType::OpenAi,
            configured_model: String::new(),
            purpose: AiUsagePurpose::Other,
            usage: AiUsage::default(),
            completed_at: Utc::now(),
        };
    }
}
