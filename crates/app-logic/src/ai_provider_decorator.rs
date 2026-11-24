//! Extension point: wrap every AI provider the app constructs (issue #1246,
//! Spec 0038, section 2).
//!
//! An integration registers an [`AiProviderDecorator`] through
//! `app_shell::Wiring::ai_provider_decorators`. Each provider built by the
//! factory (chat session connect, notes, risk second opinion, credential
//! test) is passed through all registered decorators in registration order.
//! Without a decorator the provider is returned unchanged.

use std::sync::Arc;

use ssh_manager_core::ai::{AiProvider, ProviderType};

/// What a decorator may know about the provider it wraps. Deliberately has
/// no field for the API key or extra headers (Spec 0022, section 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiProviderInfo {
    pub provider_type: ProviderType,
    /// The effective endpoint (the per-type default when none is configured).
    pub base_url: String,
    pub model: String,
}

/// Wraps a freshly built provider.
///
/// A wrapper only ever sees the `RedactedContext` that `send` receives; it
/// cannot add unredacted content to the path. **A wrapper must forward
/// `disable_web_research` and `last_request_id` to the wrapped provider**:
/// the trait defaults do nothing, so a wrapper that omits them silently
/// breaks web-research disabling after a rejection and request-id logging.
pub trait AiProviderDecorator: Send + Sync {
    fn decorate(&self, info: &AiProviderInfo, provider: Box<dyn AiProvider>)
        -> Box<dyn AiProvider>;
}

/// The one helper that applies the decorators, in registration order.
pub fn apply_decorators(
    decorators: &[Arc<dyn AiProviderDecorator>],
    info: &AiProviderInfo,
    provider: Box<dyn AiProvider>,
) -> Box<dyn AiProvider> {
    decorators
        .iter()
        .fold(provider, |provider, d| d.decorate(info, provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_debug_has_no_secret_fields() {
        let info = AiProviderInfo {
            provider_type: ProviderType::OpenAi,
            base_url: "https://x.example".into(),
            model: "m".into(),
        };
        let dbg = format!("{info:?}");
        assert!(!dbg.to_lowercase().contains("key"));
        assert!(!dbg.to_lowercase().contains("header"));
    }
}
