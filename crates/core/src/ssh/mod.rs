//! SSH-Verbindungsmanagement — Traits (Spec 0005, Abschnitt 4-7) und reine
//! Logik (Jump-Host-Auflösung, Auth-Resolution).
//!
//! Die konkrete `russh`-basierte Implementierung lebt bewusst in einer
//! eigenen Crate `crates/ssh-transport` (Spec 0005, Abschnitt 2) — dasselbe
//! Prinzip wie die Trennung `core::profiles`/`persistence-sqlite` aus Spec
//! 0004: `core` bleibt frei von I/O-Abhängigkeiten und schnell über
//! Mock-Implementierungen testbar.

mod auth;
mod error;
mod host_key;
mod jump_host;
#[cfg(any(test, feature = "test-support"))]
pub mod mock;
mod transport;
mod types;

#[cfg(test)]
mod tests;

pub use auth::{resolve_auth, ResolvedAuth};
pub use error::SshError;
pub use host_key::HostKeyStore;
pub use jump_host::resolve_connection_target;
pub use transport::{InteractiveShell, SftpSession, SshTransport};
pub use types::{CommandOutput, ConnectionTarget, Hop, HostKeyDecision, PtySize, RemoteEntry};
