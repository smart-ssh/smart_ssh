//! MCP-Server-Integration, Free-Tier (Spec 0028, Abschnitt 2). Exponiert
//! kontrollierten Zugriff auf Smart-SSH-Server als MCP-Tools über einen
//! lokalen HTTP-Server (Streamable HTTP, Spec 0028 Abschnitt 8).
//!
//! Diese Crate enthält **keine** eigene Ausführungslogik — sie übersetzt
//! MCP-Tool-Calls in `AiAction`-Werte und reicht sie über den [`McpBackend`]
//! -Trait an eine externe Implementierung weiter (in der laufenden App:
//! `crates/app-tauri`, das direkt `orchestration::handle_action_proposed`
//! aufruft). Siehe [`backend`] für die Begründung der Abhängigkeitsrichtung.

mod auth;
mod backend;
mod config;
mod tool_server;

pub use auth::SharedToken;
pub use backend::{ActionOutcome, LookupError, McpBackend, ServerSummary};
pub use config::{serve, McpServerConfig, McpServerHandle, DEFAULT_CONFIRM_TIMEOUT, DEFAULT_PORT};
pub use tool_server::SmartSshMcpServer;
