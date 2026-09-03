//! Konkrete `AiProvider`-Implementierungen (Spec 0006) gegen echte HTTP-APIs.
//!
//! Analog zu `persistence-sqlite` (Spec 0004) und `ssh-transport` (Spec
//! 0005): `ssh-manager-core::ai` definiert nur die Traits, alles I/O
//! (HTTP-Requests, SSE-Streaming) lebt hier.

mod action;
mod anthropic;
mod discovery;
mod error;
mod fallback;
mod openai_compatible;
mod prompt_escape;
mod request_logging;
mod sse;

pub use anthropic::AnthropicProvider;
pub use discovery::{discover_models, fetch_attestation_info};
pub use openai_compatible::OpenAiCompatibleProvider;
