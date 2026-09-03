//! Einstellungen und Lebenszyklus des lokalen MCP-Servers (Spec 0028,
//! Abschnitt 9). Persistiert über denselben `tauri-plugin-store` wie
//! andere reine UI-/App-Einstellungen (Spec 0024-Muster, s.
//! `crate::risk_second_opinion`) — anders als dort aber **nicht** direkt
//! vom Frontend geschrieben: Aktivieren/Token-Rotation/Allow-Liste haben
//! eine sofortige Live-Wirkung (laufenden Server starten/stoppen, Token im
//! laufenden Server austauschen, s. Spec Abschnitt 9: "invalidiert das
//! alte Token sofort"), die nur über einen Tauri-Command konsistent mit dem
//! persistierten Wert bleibt — ein reiner JS-seitiger Store-Schreibzugriff
//! hätte keine Wirkung auf einen bereits laufenden Server.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_store::StoreExt;

use ssh_manager_core::shared::ServerId;

use crate::error::CommandResult;
use crate::mcp_backend::AppMcpBackend;
use crate::state::AppState;

const SETTINGS_STORE_FILE: &str = "settings.json";
const ENABLED_KEY: &str = "mcpServerEnabled";
const TOKEN_KEY: &str = "mcpServerToken";
const ALLOWED_SERVERS_KEY: &str = "mcpServerAllowedServerIds";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSettingsDto {
    pub enabled: bool,
    /// `http://127.0.0.1:<port>` — der Wert, den der Nutzer in seinen
    /// externen Client (z. B. Claude Codes MCP-Konfiguration) einträgt.
    pub endpoint: String,
    pub token: String,
    pub allowed_server_ids: Vec<String>,
}

fn generate_token() -> String {
    // `.simple()`: keine Bindestriche — etwas kürzer zum Abtippen/Einfügen
    // in eine externe Client-Konfiguration, ohne an kryptographischer
    // Stärke zu verlieren (UUID v4 bleibt 122 Bit Zufall, nur die
    // Textdarstellung ändert sich).
    uuid::Uuid::new_v4().simple().to_string()
}

/// Unabhängiger Review-Pass (Spec 0028): das MCP-Bearer-Token — das einzige
/// Geheimnis, das die gesamte MCP-Angriffsfläche freischaltet — landet über
/// `tauri-plugin-store` mit OS-Standardrechten (typ. 0644, weltlesbar) in
/// `settings.json`, während dasselbe Projekt Logs (`logging.rs`), die
/// SQLite-DB und `host_keys.json` bereits konsequent auf 0600/0700 härtet
/// und API-Keys in den OS-Keychain statt in den Store legt. Jeder
/// mitlaufende Prozess desselben Nutzerkontos, der die Datei lesen darf,
/// wird damit zu einem vollwertigen MCP-Client. Härtet die Datei nach jedem
/// Token-Schreibzugriff best-effort auf 0600 — löst NICHT das größere,
/// hier bewusst nicht angegangene Problem (Klartext auf der Platte bleibt,
/// root/derselbe Nutzer kann weiterhin lesen; die vollständige Lösung wäre
/// das Token in den `CredentialStore` zu verschieben und nur ein Handle in
/// `settings.json` zu halten — größere Änderung, siehe Abschlussbericht).
fn harden_settings_store_permissions(app: &AppHandle) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(config_dir) = app.path().app_config_dir() else {
            return;
        };
        let path = config_dir.join(SETTINGS_STORE_FILE);
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = app;
    }
}

/// `None`, falls noch nie eines generiert wurde (erster Aufruf überhaupt).
fn stored_token(app: &AppHandle) -> CommandResult<Option<String>> {
    let store = app.store(SETTINGS_STORE_FILE)?;
    Ok(store
        .get(TOKEN_KEY)
        .and_then(|v| v.as_str().map(str::to_string)))
}

/// Liefert das persistierte Token, generiert und speichert bei Bedarf eins
/// — so gibt es immer einen Wert zum Anzeigen, sobald der Einstellungen-
/// Screen einmal geöffnet wurde, unabhängig davon, ob MCP bereits aktiviert
/// wurde (Spec 0028, Abschnitt 9: Token wird immer angezeigt).
fn load_or_init_token(app: &AppHandle) -> CommandResult<String> {
    if let Some(token) = stored_token(app)? {
        return Ok(token);
    }
    let token = generate_token();
    let store = app.store(SETTINGS_STORE_FILE)?;
    store.set(TOKEN_KEY, serde_json::json!(token));
    store.save()?;
    harden_settings_store_permissions(app);
    Ok(token)
}

fn load_allowed_servers(app: &AppHandle) -> CommandResult<HashSet<ServerId>> {
    let store = app.store(SETTINGS_STORE_FILE)?;
    let Some(value) = store.get(ALLOWED_SERVERS_KEY) else {
        return Ok(HashSet::new());
    };
    let Some(array) = value.as_array() else {
        return Ok(HashSet::new());
    };
    Ok(array
        .iter()
        .filter_map(|v| v.as_str())
        .filter_map(|s| uuid::Uuid::parse_str(s).ok())
        .map(ServerId)
        .collect())
}

fn store_allowed_servers(app: &AppHandle, ids: &HashSet<ServerId>) -> CommandResult<()> {
    let store = app.store(SETTINGS_STORE_FILE)?;
    let ids_json: Vec<String> = ids.iter().map(|id| id.0.to_string()).collect();
    store.set(ALLOWED_SERVERS_KEY, serde_json::json!(ids_json));
    store.save()?;
    Ok(())
}

fn is_enabled_setting(app: &AppHandle) -> CommandResult<bool> {
    let store = app.store(SETTINGS_STORE_FILE)?;
    Ok(store
        .get(ENABLED_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

/// Synct `state.mcp.token`/`allowed_servers` mit dem persistierten Stand —
/// nötig, weil `AppState`/`McpState::default()` beim App-Start mit einem
/// bedeutungslosen Platzhalter-Token startet (s. dortiger Doc-Kommentar);
/// jeder Command hier ruft das zuerst auf, damit die Live-Werte auch nach
/// einem Neustart wieder mit dem zuletzt gespeicherten Stand übereinstimmen.
fn sync_live_state_from_store(app: &AppHandle, state: &AppState) -> CommandResult<()> {
    let token = load_or_init_token(app)?;
    state.mcp.token.set(token);
    let allowed = load_allowed_servers(app)?;
    *state.mcp.allowed_servers.lock().expect("Mutex vergiftet") = allowed;
    Ok(())
}

async fn build_dto(app: &AppHandle, state: &AppState) -> CommandResult<McpServerSettingsDto> {
    let enabled = state.mcp.runtime.lock().await.is_some();
    let token = load_or_init_token(app)?;
    let allowed_server_ids = state
        .mcp
        .allowed_servers
        .lock()
        .expect("Mutex vergiftet")
        .iter()
        .map(|id| id.0.to_string())
        .collect();
    Ok(McpServerSettingsDto {
        enabled,
        // `/mcp` — der tatsächliche Streamable-HTTP-Pfad, s.
        // `mcp_server::config::serve`s `nest_service("/mcp", ...)`. Ohne
        // diesen Suffix wäre der angezeigte Wert nicht direkt in eine
        // externe Client-Konfiguration einsetzbar.
        endpoint: format!(
            "http://{}/mcp",
            mcp_server::McpServerConfig::default().bind_addr
        ),
        token,
        allowed_server_ids,
    })
}

/// Startet den Server, falls nicht schon einer läuft — ein zweiter
/// `set_mcp_server_enabled(true)`-Aufruf (z. B. durch Doppelklick im UI)
/// ist damit ein No-Op statt eines zweiten Listeners auf demselben Port.
async fn start_server_if_not_running(app: &AppHandle, state: &AppState) -> CommandResult<()> {
    let mut runtime = state.mcp.runtime.lock().await;
    if runtime.is_some() {
        return Ok(());
    }
    let backend: Arc<dyn mcp_server::McpBackend> = Arc::new(AppMcpBackend::new(app.clone()));
    let config = mcp_server::McpServerConfig::default();
    let handle = mcp_server::serve(config, backend, state.mcp.token.clone()).await?;
    tracing::info!(origin = "mcp", addr = %handle.local_addr, "mcp server started");
    *runtime = Some(handle);
    Ok(())
}

async fn stop_server_if_running(state: &AppState) {
    let mut runtime = state.mcp.runtime.lock().await;
    if let Some(handle) = runtime.take() {
        handle.shutdown().await;
        tracing::info!(origin = "mcp", "mcp server stopped");
    }
}

#[tauri::command]
pub async fn get_mcp_server_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<McpServerSettingsDto> {
    sync_live_state_from_store(&app, &state)?;
    build_dto(&app, &state).await
}

#[tauri::command]
pub async fn set_mcp_server_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<McpServerSettingsDto> {
    sync_live_state_from_store(&app, &state)?;
    let store = app.store(SETTINGS_STORE_FILE)?;
    store.set(ENABLED_KEY, serde_json::json!(enabled));
    store.save()?;

    if enabled {
        start_server_if_not_running(&app, &state).await?;
    } else {
        stop_server_if_running(&state).await;
    }
    build_dto(&app, &state).await
}

#[tauri::command]
pub async fn regenerate_mcp_server_token(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<McpServerSettingsDto> {
    let new_token = generate_token();
    let store = app.store(SETTINGS_STORE_FILE)?;
    store.set(TOKEN_KEY, serde_json::json!(new_token));
    store.save()?;
    harden_settings_store_permissions(&app);
    // Live-Effekt sofort, unabhängig davon, ob der Server gerade läuft
    // (Spec 0028, Abschnitt 9: "invalidiert das alte Token sofort") — ein
    // laufender Server prüft bei jedem Tool-Call gegen `state.mcp.token`,
    // eine explizite Benachrichtigung des Servers ist nicht nötig.
    state.mcp.token.set(new_token);
    build_dto(&app, &state).await
}

#[tauri::command]
pub async fn set_mcp_server_allowed_servers(
    app: AppHandle,
    state: State<'_, AppState>,
    server_ids: Vec<ServerId>,
) -> CommandResult<McpServerSettingsDto> {
    let ids: HashSet<ServerId> = server_ids.into_iter().collect();
    store_allowed_servers(&app, &ids)?;
    *state.mcp.allowed_servers.lock().expect("Mutex vergiftet") = ids;
    build_dto(&app, &state).await
}

/// Beim App-Start aufgerufen (s. `lib.rs::run`s `.setup(...)`): startet den
/// Server automatisch, falls er beim letzten Beenden aktiviert war — ein
/// einmal aktivierter externer Zugriff soll nicht bei jedem Neustart
/// erneut manuell angestoßen werden müssen. Fehler beim Start werden nur
/// geloggt, nicht dem App-Start in den Weg gestellt (derselbe
/// Fail-safe-Gedanke wie bei `resolve_second_opinion_provider`: lieber kein
/// MCP-Server als ein blockierter App-Start).
pub async fn autostart_if_enabled(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match sync_live_state_from_store(app, &state).and(is_enabled_setting(app)) {
        Ok(true) => {
            if let Err(err) = start_server_if_not_running(app, &state).await {
                tracing::warn!(origin = "mcp", error = %err.message, "mcp autostart failed");
            }
        }
        Ok(false) => {}
        Err(err) => {
            tracing::warn!(origin = "mcp", error = %err.message, "mcp autostart settings read failed");
        }
    }
}
