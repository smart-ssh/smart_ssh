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

mod fencing;
mod provider;
mod redactor;
mod types;

#[cfg(test)]
mod tests;

pub use fencing::{fence_markers, fence_untrusted, UntrustedKind};
pub use provider::AiProvider;
pub use redactor::{DefaultOutputRedactor, OutputRedactor};
pub use types::{
    default_action_schemas, ActionParameter, ActionParameterKind, ActionSchema, AiError, AiEvent,
    ChatMessage, MessageContent, ProviderId, ProviderType, RejectionReason, Role, SessionContext,
};
