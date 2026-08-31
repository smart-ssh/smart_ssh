//! Tauri-App-Schicht von ssh-manager (Spec 0007). Dünner Wrapper: nur
//! Tauri-Commands, die Core-APIs aufrufen und DTOs zurückgeben, sowie der
//! `AppState`-Aufbau — keine fachliche Logik hier (Spec 0007, Abschnitt 3).

mod ai_provider_factory;
mod commands;
mod confirmation;
mod dto;
mod ephemeral_credentials;
mod error;
mod events;
mod groups;
mod host_key_store;
mod orchestration;
mod policy;
mod server_credentials;
mod session;
mod state;
mod test_connection;
#[cfg(test)]
mod test_support;

use std::sync::Arc;

use credentials_keyring::KeyringCredentialStore;
use persistence_sqlite::{default_db_path, SqliteProfileStore};

use crate::confirmation::ConfirmationRegistry;
use crate::host_key_store::FileHostKeyStore;
use crate::session::SessionManager;
use crate::state::AppState;

/// Baut den `AppState` einmalig beim App-Start auf. Synchron nach außen
/// (`run()` wird von `main.rs` ohne `#[tokio::main]` aufgerufen, wie im
/// Standard-Tauri-Bootstrap üblich) — `tauri::async_runtime::block_on`
/// überbrückt den einen async `SqliteProfileStore::connect`-Aufruf beim
/// Start; danach läuft alles über Tauris eigene, bereits laufende
/// Async-Runtime (jedes `#[tauri::command]` ist selbst `async fn`).
fn build_app_state() -> AppState {
    let db_path = default_db_path();
    let profile_store = tauri::async_runtime::block_on(SqliteProfileStore::connect(&db_path))
        .expect("SQLite-Datenbank konnte nicht geöffnet/migriert werden");
    let ai_provider_store = profile_store.ai_provider_store();

    // Host-Keys leben bewusst neben (nicht in) der SQLite-Datenbank — s.
    // `crate::host_key_store`-Modul-Kommentar zur Begründung (der
    // `HostKeyStore`-Trait ist absichtlich synchron, `sqlx` ist es nicht).
    let host_key_path = db_path
        .parent()
        .expect("db_path hat immer ein Elternverzeichnis (s. default_db_path)")
        .join("host_keys.json");
    let host_key_store = FileHostKeyStore::load(host_key_path)
        .expect("Host-Key-Speicher konnte nicht geladen werden");

    AppState {
        sessions: SessionManager::new(),
        profile_store: Arc::new(profile_store),
        credential_store: Arc::new(KeyringCredentialStore::new()),
        ai_provider_store: Arc::new(ai_provider_store),
        host_key_store: Arc::new(host_key_store),
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        pending_action_confirmations: ConfirmationRegistry::new(),
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(build_app_state())
        .invoke_handler(tauri::generate_handler![
            commands::list_servers,
            commands::list_ai_providers,
            commands::add_ai_provider,
            commands::update_ai_provider,
            commands::delete_ai_provider,
            commands::set_active_ai_provider,
            commands::connect,
            commands::confirm_host_key,
            commands::open_terminal,
            commands::terminal_input,
            commands::terminal_resize,
            commands::send_chat_message,
            commands::respond_to_action,
            commands::disconnect,
            commands::list_groups,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::get_server,
            commands::create_server,
            commands::update_server,
            commands::delete_server,
            commands::test_connection,
            commands::trust_host_key,
            commands::update_group_notes,
            commands::update_server_notes,
            commands::list_note_revisions,
            commands::rollback_note,
            commands::preview_effective_notes,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der Tauri-App");
}
