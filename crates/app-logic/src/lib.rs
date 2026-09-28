//! `app-logic` (Spec 0084): die Tauri-freie Anwendungslogik von Smart SSH —
//! Chat-Turn-Orchestrierung, Kompaktierung, Bestätigung, Verbindungstest,
//! Server-/Gruppen-/Credential-Verwaltung, DTOs, gemeinsamer App-Zustand.
//! Diese Crate kennt `tauri`/`tauri-plugin-*`/`wry`/`tao`/`rfd` nicht — weder
//! direkt noch transitiv über ein eigenes Feature (A3, geprüft per CI-Schritt
//! in `.github/workflows/community.yml`).
//!
//! `app-shell` hängt von dieser Crate ab, nie umgekehrt. Was hier bleibt und
//! was in `app-shell` bleibt, entscheidet ausschließlich, ob ein Modul Tauri
//! selbst braucht (`tauri::`/`tauri_plugin_*::`/`AppHandle`/`rfd::`/
//! `#[tauri::command]`) — s. `docs/architecture.md`.
//!
//! Der erhöhte SFTP-Kanal (Spec 0067 A) ist die eine bewusste Ausnahme in
//! die andere Richtung: Obwohl er keine Tauri-Abhängigkeit hat, bleibt er in
//! `app-shell` (dort: `elevated_sftp`, `event_emitter`,
//! `risk_second_opinion`, `local_server`, `commands`, `mcp_backend`,
//! `mcp_settings`, `first_run_notice`, `chat_retention`, `ssh_config_apply`,
//! `ssh_config_export`, `startup_dialog`, `wiring`) — diese Crate
//! (`app-logic`) enthält keinen Typ, keine Funktion und kein Feld, über das
//! er erreichbar ist (A1).

pub mod ai_provider_factory;
pub mod compaction;
pub mod confirmation;
pub mod diagnostics;
pub mod document_export;
pub mod dto;
pub mod ephemeral_credentials;
pub mod error;
pub mod events;
pub mod filter_rules;
pub mod groups;
pub mod host_key_store;
pub mod identity_file;
pub mod key_files;
pub mod logging;
pub mod orchestration;
/// Spec 0084, A5: `NoRulesPolicyStore` ist ein Testdouble, das sowohl
/// innerhalb dieser Crate (`session.rs`, `orchestration::test_support`) als
/// auch von `app-shell`s Tests (`commands::chat`) gebraucht wird — deshalb
/// hinter demselben Feature wie `test_support`, nicht reinem
/// `#[cfg(test)]`.
#[cfg(any(test, feature = "test-support"))]
pub mod policy;
pub mod rule_suggestions;
pub mod second_opinion;
pub mod server_credentials;
pub mod servers;
pub mod session;
pub mod ssh_config_import;
pub mod startup_error_messages;
pub mod state;
pub mod test_connection;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod version;
