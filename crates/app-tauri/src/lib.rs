//! Tauri-App-Schicht von ssh-manager (Spec 0007). Dünner Wrapper: nur
//! Tauri-Commands, die Core-APIs aufrufen und DTOs zurückgeben, sowie der
//! `AppState`-Aufbau — keine fachliche Logik hier (Spec 0007, Abschnitt 3).

mod ai_provider_factory;
mod commands;
mod confirmation;
mod document_export;
mod dto;
mod ephemeral_credentials;
mod error;
mod events;
mod filter_rules;
mod groups;
mod host_key_store;
mod logging;
mod mcp_backend;
mod mcp_settings;
mod orchestration;
#[cfg(test)]
mod policy;
mod risk_second_opinion;
mod rule_suggestions;
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
    let policy_store = profile_store.policy_store();
    let prompt_history_store = profile_store.prompt_history_store();

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
        policy_store,
        prompt_history_store,
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        pending_action_confirmations: ConfirmationRegistry::new(),
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        mcp: crate::state::McpState::default(),
    }
}

pub fn run() {
    // Spec 0016, Abschnitt 2/3: so früh wie möglich, damit auch Fehler beim
    // App-Setup selbst (z. B. `build_app_state()`s DB-Verbindungsaufbau)
    // bereits strukturiert geloggt würden. `_log_guard` muss über die
    // gesamte App-Laufzeit am Leben bleiben (s. `crate::logging::
    // init_logging`-Doc-Kommentar) — `run()` unten blockiert bis zum
    // Beenden der App, danach ist ein finaler Flush ohnehin irrelevant.
    let _log_guard = crate::logging::init_logging();
    tracing::info!("Smart SSH startet");

    tauri::Builder::default()
        // Für die Key-/Zertifikat-Datei-Auswahl im Server-Formular (Spec
        // 0008, Abschnitt 6) — Dateiinhalt wird clientseitig gelesen
        // (`plugin-fs`) und geht direkt an `create_server`/`update_server`,
        // der Pfad selbst wird nie gespeichert (Spec Abschnitt 8).
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        // Spec 0024, Abschnitt 4: Speicherort für die gewählte UI-Sprache
        // (und künftige reine UI-Einstellungen wie ein Theme) — bewusst kein
        // sekundärer SQLite-Migrationspfad für eine einzelne, nicht
        // sicherheitsrelevante Einstellung.
        .plugin(tauri_plugin_store::Builder::new().build())
        // Spec 0024, Abschnitt 4: liefert die System-Locale für die
        // Sprachermittlung beim ersten Start (`frontend/src/i18n.ts`).
        .plugin(tauri_plugin_os::init())
        // Entscheidung für tauri-plugin-decoration statt tauri-plugin-decorum:
        // tauri-plugin-decorum (v0.1.6) wird nicht mehr aktiv gepflegt und wirft Build-Fehler
        // bei modernen Rust-Toolchains/macOS-SDKs. tauri-plugin-decoration (v3.0.5) ist aktiv
        // gepflegt, unterstützt Tauri v2.10+ und verwaltet native macOS-Ampel-Insets sowie Windows Snap Layouts.
        .plugin(tauri_plugin_decoration::init())
        // Spec 0028, Abschnitt 9a: native Toast-Benachrichtigung bei einer
        // wartenden MCP-Bestätigung (s. `crate::mcp_backend`).
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            use tauri::Manager;
            use tauri_plugin_decoration::WebviewWindowExt;

            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = window_clone.activate_decoration().await;
                    #[cfg(target_os = "macos")]
                    {
                        // Spec 0014 Abschnitt 3 & 6: macOS Ampel-Inset (Startwert 12.0, 16.0)
                        let _ = window_clone.set_traffic_lights_inset(12.0, 16.0).await;
                    }
                });
            }

            // Spec 0028, Abschnitt 9: ein beim letzten Beenden aktivierter
            // MCP-Server bleibt über einen Neustart hinweg aktiv, ohne
            // manuelles erneutes Anschalten.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::mcp_settings::autostart_if_enabled(&handle).await;
            });

            Ok(())
        })
        .manage(build_app_state())
        .invoke_handler(tauri::generate_handler![
            commands::list_servers,
            commands::list_ai_providers,
            commands::add_ai_provider,
            commands::update_ai_provider,
            commands::delete_ai_provider,
            commands::set_active_ai_provider,
            commands::discover_models,
            commands::fetch_attestation_info,
            commands::connect,
            commands::confirm_host_key,
            commands::open_terminal,
            commands::terminal_input,
            commands::terminal_resize,
            commands::send_chat_message,
            commands::respond_to_action,
            commands::cancel_running_command,
            commands::stop_auto_continuation,
            commands::disconnect,
            commands::list_sessions,
            commands::list_groups,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::get_server,
            commands::create_server,
            commands::update_server,
            commands::delete_server,
            commands::clear_server_sudo_password,
            commands::test_connection,
            commands::trust_host_key,
            commands::update_group_notes,
            commands::update_server_notes,
            commands::list_note_revisions,
            commands::rollback_note,
            commands::preview_effective_notes,
            commands::list_rules,
            commands::create_rule,
            commands::update_rule,
            commands::delete_rule,
            commands::list_hard_blacklist,
            commands::list_known_tags,
            commands::evaluate_explained,
            commands::suggest_rule_patterns,
            commands::accept_and_create_rule,
            commands::export_document,
            commands::read_credential_file,
            commands::get_platform,
            commands::create_overlay_titlebar,
            commands::list_prompt_history,
            commands::open_log_directory,
            commands::sftp_list,
            commands::sftp_download,
            commands::sftp_upload,
            commands::sftp_delete,
            commands::sftp_rename,
            commands::sftp_mkdir,
            mcp_settings::get_mcp_server_settings,
            mcp_settings::set_mcp_server_enabled,
            mcp_settings::regenerate_mcp_server_token,
            mcp_settings::set_mcp_server_allowed_servers,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der Tauri-App");
}
