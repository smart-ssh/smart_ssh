//! KI-Provider-Abstraktion — Traits (Spec 0006, Abschnitt 3/5/6) und reine
//! Logik (Default-Redactor).
//!
//! **Wichtige Abgrenzung** (Spec 0006, Abschnitt 1): dieses Modul schlägt
//! Aktionen nur vor. Es führt nichts aus und umgeht nie die Filter-Engine
//! (`crate::filter`, Spec 0002).
//!
//! Die konkreten Provider-Implementierungen (OpenAI-kompatibel, Anthropic)
//! leben bewusst in einer eigenen Crate `crates/ai-providers` (Spec 0006,
//! Abschnitt 2) — dasselbe Prinzip wie `core::profiles`/`persistence-sqlite`
//! (Spec 0004) und `core::ssh`/`ssh-transport` (Spec 0005): `core` bleibt
//! frei von I/O-Abhängigkeiten (kein HTTP-Client hier) und schnell über
//! Mock-Implementierungen testbar.

mod extension_content;
mod extension_part;
mod fencing;
pub mod keywords;
mod live_redaction;
mod provider;
mod redactor;
#[cfg(test)]
mod redactor_generators;
#[cfg(test)]
mod redactor_property;
#[cfg(test)]
mod redactor_reference;
mod second_opinion;
mod types;

#[cfg(test)]
mod tests;

pub use extension_content::{
    seal_pending, ContentLimitError, RedactedBlock, ResolvedBlock, SealedContent, SealedPart,
    BINARY_BLOCK_TOKEN_ESTIMATE, CONTENT_UNAVAILABLE, EXTENSION_SECURITY_LINE, MAX_BLOCKS_PER_PART,
    MAX_BLOCK_BYTES, MAX_EXTENSION_PARTS_PER_MESSAGE, MAX_REQUEST_BINARY_BYTES,
};
pub use extension_part::{ExtensionMedia, ExtensionPart, ExtensionSource};
pub use fencing::{fence_markers, fence_untrusted, UntrustedKind};
pub use live_redaction::LiveOutputRedactor;
pub use provider::AiProvider;
pub use redactor::{
    default_log_redactor, DefaultOutputRedactor, OutputRedactor, REDACTED_PLACEHOLDER,
};
pub use second_opinion::{truncate_for_second_opinion, DEFAULT_SECOND_OPINION_MAX_LEN};
pub use types::{
    default_action_schemas, english_action_schemas, sanitize_tool_name, ActionParameter,
    ActionParameterKind, ActionSchema, AiError, AiEvent, ChatMessage, InvalidToolCallReason,
    MediaClass, MessageContent, ProviderId, ProviderType, RejectionReason, Role, SessionContext,
    WebActivity, WebActivityKind, WebSource, WEB_CONTENT_MAX_CHARS,
};
