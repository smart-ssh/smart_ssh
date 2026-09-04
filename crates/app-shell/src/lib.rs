//! App-Shell (Spec 0038): Library, die den bisherigen `app-tauri`-Aufbau
//! (Tauri-Commands, `AppState`-Aufbau, Event-Handling — alles seit Spec
//! 0007) unverändert in der Substanz bereitstellt, jetzt aber
//! Edition-parametrisiert über [`Wiring`] statt fest verdrahtet. Ein
//! konkretes Binary (aktuell nur `apps/smart-ssh-community`, künftig auch
//! ein privates `Official`-Pendant, s. Spec 0038 Abschnitt 3) unterscheidet
//! sich nur im übergebenen `Wiring` und ruft [`run`] auf.
//!
//! Weiterhin: nur Tauri-Commands, die Core-APIs aufrufen und DTOs
//! zurückgeben, keine fachliche Logik hier (Spec 0007, Abschnitt 3) — diese
//! Abgrenzung gilt unverändert, nur die Crate-Grenze hat sich verschoben.

mod ai_provider_factory;
mod chat_context_truncation;
mod chat_retention;
mod commands;
mod confirmation;
mod document_export;
mod dto;
mod ephemeral_credentials;
mod error;
mod events;
mod filter_rules;
mod first_run_notice;
mod groups;
mod host_key_store;
mod local_server;
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
mod wiring;

pub use wiring::{Edition, Wiring};

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
fn build_app_state(wiring: &Wiring) -> AppState {
    let db_path = default_db_path();
    let profile_store = tauri::async_runtime::block_on(SqliteProfileStore::connect(&db_path))
        .expect("SQLite-Datenbank konnte nicht geöffnet/migriert werden");
    let ai_provider_store = profile_store.ai_provider_store();
    let policy_store = profile_store.policy_store();

    // Spec 0036, Abschnitt 4: einmalig bei App-Start aufgelöst (kein
    // "erster Schreibzugriff" im wörtlichen Sinn, aber dieselbe Wirkung:
    // der Schlüssel existiert garantiert, bevor irgendein Schreibzugriff
    // stattfinden kann — s. Kommentar unten).
    //
    // Spec 0040, Abschnitt 7: KEIN `.expect(...)` mehr — ein gesperrter
    // oder vom Nutzer verweigerter OS-Schlüsselbund (z. B. macOS-Keychain-
    // Dialog abgebrochen) darf den App-Start nicht verhindern. Anders als
    // die DB-Verbindung/den Host-Key-Speicher oben/unten ist ein
    // funktionierender Verschlüsselungsschlüssel keine Voraussetzung für
    // die App selbst, nur für EINE optionale Komfortfunktion (Chat-
    // Persistenz/Prompt-Historie) — degradiert bei einem Fehler zu `None`
    // für beide betroffenen Stores (s. `AppState::prompt_history_store`/
    // `chat_session_store`-Doc-Kommentare) statt die ganze App abzubrechen.
    let credential_store = KeyringCredentialStore::new();
    let (prompt_history_store, chat_session_store) =
        match ssh_manager_core::crypto::resolve_or_generate_key(&credential_store) {
            Ok(chat_content_key) => {
                let chat_content_cipher: Arc<dyn ssh_manager_core::crypto::ContentCipher> =
                    Arc::new(ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(
                        &chat_content_key,
                    ));
                // Spec 0040, Abschnitt 3: derselbe Cipher (und damit derselbe
                // Schlüssel) für beide Stores — kein zweiter
                // Verschlüsselungsmechanismus für `prompt_history`.
                (
                    Some(profile_store.prompt_history_store(chat_content_cipher.clone())),
                    Some(profile_store.chat_session_store(chat_content_cipher)),
                )
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "encryption key for chat content unavailable — chat persistence and \
                     prompt history disabled for this app run",
                );
                (None, None)
            }
        };

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
        credential_store: Arc::new(credential_store),
        ai_provider_store: Arc::new(ai_provider_store),
        host_key_store: Arc::new(host_key_store),
        policy_store,
        prompt_history_store,
        chat_session_store,
        // Spec 0038, Abschnitt 2: aus dem übergebenen `Wiring` gelesen statt
        // hier fest verdrahtet (s. `Wiring::community`-Doc-Kommentar zum
        // Scope dieses Refactorings).
        entitlements: wiring.entitlements.clone(),
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        pending_action_confirmations: ConfirmationRegistry::new(),
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        mcp: crate::state::McpState::default(),
    }
}

/// Startet die App mit der übergebenen [`Wiring`]/[`tauri::Context`].
///
/// Der `Context` wird bewusst vom Aufrufer (`main.rs` des jeweiligen
/// Binaries, z. B. `apps/smart-ssh-community`) übergeben statt hier per
/// `tauri::generate_context!()` erzeugt: das Makro liest `tauri.conf.json`
/// relativ zum `CARGO_MANIFEST_DIR` der Aufrufstelle zur Kompilierzeit —
/// stünde der Makroaufruf hier in `app-shell`, müssten `tauri.conf.json`/
/// Icons/Frontend dieser Library-Crate zugeordnet sein, obwohl `app-shell`
/// künftig mehrere Binaries (Community/Official) mit je eigener Config
/// bedienen soll (Spec 0038, Abschnitt 3).
pub fn run(wiring: Wiring, context: tauri::Context<tauri::Wry>) {
    // Spec 0016, Abschnitt 2/3: so früh wie möglich, damit auch Fehler beim
    // App-Setup selbst (z. B. `build_app_state()`s DB-Verbindungsaufbau)
    // bereits strukturiert geloggt würden. `_log_guard` muss über die
    // gesamte App-Laufzeit am Leben bleiben (s. `crate::logging::
    // init_logging`-Doc-Kommentar) — `run()` unten blockiert bis zum
    // Beenden der App, danach ist ein finaler Flush ohnehin irrelevant.
    let _log_guard = crate::logging::init_logging();
    tracing::info!("Smart SSH startet");

    let app_state = build_app_state(&wiring);
    let plugins = wiring.plugins;

    let builder = tauri::Builder::default();
    let builder = plugins
        .into_iter()
        .fold(builder, |builder, plugin| plugin(builder));

    builder
        // Für die Key-/Zertifikat-Datei-Auswahl im Server-Formular (Spec
        // 0008, Abschnitt 6): der native Dialog läuft im Backend
        // (`commands::read_credential_file`, Spec 0013 SEC-06 — unabhängiger
        // Review-Pass ersetzte hier den vorherigen `plugin-fs`-basierten
        // Ansatz, bei dem das Webview die Datei clientseitig gelesen hätte),
        // der Pfad selbst wird nie gespeichert (Spec 0008 Abschnitt 8). Kein
        // `tauri_plugin_fs` mehr registriert — nichts nutzt es noch, und die
        // `capabilities/default.json` gewährt ohnehin keine `fs:*`-Rechte;
        // ein ungenutztes, geladenes Plugin ist unnötige Angriffsfläche.
        .plugin(tauri_plugin_dialog::init())
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

            // Spec 0034, Abschnitt 5: Aufbewahrungs-Aufräum-Job beim
            // App-Start — No-op, solange keine Aufbewahrungsdauer
            // konfiguriert ist (Default).
            let handle_for_retention = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle_for_retention.state::<AppState>();
                crate::chat_retention::cleanup_old_chat_sessions_on_startup(
                    &handle_for_retention,
                    &state,
                )
                .await;
            });

            // Spec 0040, Abschnitt 3: einmalige, idempotente Migration
            // bestehender Klartext-Zeilen in `prompt_history` — No-op,
            // sobald alle Zeilen bereits verschlüsselt sind (jeder Start
            // danach prüft erneut, findet aber nichts mehr zu tun).
            let handle_for_prompt_history_migration = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle_for_prompt_history_migration.state::<AppState>();
                // Spec 0040, Abschnitt 7: `None`, wenn der Verschlüsselungs-
                // schlüssel beim Start nicht aufgelöst werden konnte (s.
                // `build_app_state`) — dann gibt es nichts zu migrieren.
                let Some(store) = &state.prompt_history_store else {
                    return;
                };
                match store.migrate_legacy_plaintext_content().await {
                    Ok(count) if count > 0 => {
                        tracing::info!(count, "legacy plaintext prompt_history rows encrypted");
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, "prompt_history encryption migration failed");
                    }
                }
            });

            Ok(())
        })
        .manage(app_state)
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
            commands::take_chat_content_into_note,
            commands::respond_to_action,
            commands::cancel_running_command,
            commands::stop_auto_continuation,
            commands::disconnect,
            commands::list_chat_sessions,
            commands::resume_chat_session,
            commands::rename_chat_session,
            commands::delete_chat_session,
            chat_retention::get_chat_session_retention_days,
            chat_retention::set_chat_session_retention_days,
            commands::list_sessions,
            commands::get_chat_history,
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
            commands::update_local_server_notes,
            commands::update_local_server_tags,
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
            mcp_settings::set_mcp_server_confirm_timeout_secs,
        ])
        .run(context)
        .expect("Fehler beim Starten der Tauri-App");
}
