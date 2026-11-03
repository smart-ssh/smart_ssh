//! DTOs für die Tauri-IPC-Grenze (Spec 0007, Abschnitt 8.2). Bewusst
//! getrennt von den `core`/`persistence-sqlite`-Domänentypen: das Frontend
//! soll nie mehr sehen als es braucht (insbesondere nie `credential_ref`
//! oder gar den API-Key selbst, s. `AiProviderConfigDto`-Doc-Kommentar
//! unten) und nie an interne Persistenz-Repräsentation gekoppelt sein.
//!
//! Issue #868: nach Domänen aufgeteilt; jeder öffentliche Pfad
//! `app_logic::dto::X` bleibt über die Re-Exports unten erhalten. `dto`
//! hängt nur von Kern-Typen und den Blattmodulen `ids`, `status` und
//! `credential_slots` ab, nicht von `state`, `events` oder
//! `server_credentials`.

mod ai_provider;
mod chat;
mod elevation;
mod groups;
mod meta;
mod notes;
mod rules;
mod server;
mod sftp;

pub use ai_provider::*;
pub use chat::*;
pub use elevation::*;
pub use groups::*;
pub use meta::*;
pub use notes::*;
pub use rules::*;
pub use server::*;
pub use sftp::*;

#[cfg(test)]
use sftp::format_unix_permissions;

mod redacted_debug;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_sudo_password_state;

#[cfg(test)]
mod tests_identity_file;
