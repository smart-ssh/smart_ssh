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
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_store::StoreExt;

use ssh_manager_core::shared::ServerId;

use crate::mcp_backend::AppMcpBackend;
use crate::settings_store::{self, SETTINGS_STORE_FILE};
use app_logic::error::CommandResult;
use app_logic::state::AppState;

const ENABLED_KEY: &str = "mcpServerEnabled";
const TOKEN_KEY: &str = "mcpServerToken";
const ALLOWED_SERVERS_KEY: &str = "mcpServerAllowedServerIds";
/// Spec 0028, Abschnitt 7: "konfigurierbares Timeout (Default 5 Minuten)" —
/// bislang nur als Konstante (`mcp_server::DEFAULT_CONFIRM_TIMEOUT`)
/// vorhanden, aber `start_server_if_not_running` konstruierte den Server
/// immer mit `McpServerConfig::default()`, sodass der Wert faktisch nicht
/// änderbar war (unabhängiger Review-Pass, Spec-Audit-Fund).
const CONFIRM_TIMEOUT_SECS_KEY: &str = "mcpServerConfirmTimeoutSecs";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSettingsDto {
    pub enabled: bool,
    /// `http://127.0.0.1:<port>` — der Wert, den der Nutzer in seinen
    /// externen Client (z. B. Claude Codes MCP-Konfiguration) einträgt.
    pub endpoint: String,
    pub token: String,
    pub allowed_server_ids: Vec<String>,
    /// Spec 0028, Abschnitt 7. Wirkt erst auf den **nächsten** Serverstart
    /// (wie der Port auch, s. `McpServerConfig`) — ein bereits laufender
    /// Server wird beim Ändern deshalb neu gestartet (s.
    /// `set_mcp_server_confirm_timeout_secs`), analog zu einem geänderten
    /// Port.
    pub confirm_timeout_secs: u64,
}

fn generate_token() -> String {
    // `.simple()`: keine Bindestriche — etwas kürzer zum Abtippen/Einfügen
    // in eine externe Client-Konfiguration, ohne an kryptographischer
    // Stärke zu verlieren (UUID v4 bleibt 122 Bit Zufall, nur die
    // Textdarstellung ändert sich).
    uuid::Uuid::new_v4().simple().to_string()
}

/// Spec 0101, A12: `settings.json` als **alter** Ablageort des Tokens.
///
/// Nur noch Lesen und Entfernen — geschrieben wird dorthin nicht mehr. Die
/// Reihenfolge des Umzugs (schreiben, zurücklesen, vergleichen, dann
/// entfernen) steckt in `app_logic::mcp_token`, damit T12 ohne Fenster
/// läuft; hier bleibt nur der Zugriff auf die Datei, der ohne Tauri nicht
/// geht.
struct SettingsJsonToken<'a> {
    app: &'a AppHandle,
}

impl app_logic::mcp_token::LegacyMcpTokenFile for SettingsJsonToken<'_> {
    fn read_token(&self) -> CommandResult<Option<String>> {
        let store = self.app.store(SETTINGS_STORE_FILE)?;
        Ok(store
            .get(TOKEN_KEY)
            .and_then(|v| v.as_str().map(str::to_string)))
    }

    fn remove_token(&self) -> CommandResult<()> {
        // `settings_store::update` puts the key back in memory when the save
        // fails (spec-reviewer round 3): without that the token would stay in
        // plaintext in the file while every later `read_token` returned
        // `None`, and the plaintext copy A12 removes would survive silently.
        settings_store::update(self.app, |changes| changes.delete(TOKEN_KEY))
    }
}

/// Liefert das persistierte Token, generiert bei Bedarf eins — so gibt es
/// immer einen Wert zum Anzeigen, sobald der Einstellungen-Screen einmal
/// geöffnet wurde, unabhängig davon, ob MCP bereits aktiviert wurde
/// (Spec 0028, Abschnitt 9: Token wird immer angezeigt).
///
/// Seit Spec 0101 A12 liegt das Token in der verschlüsselten Datenbank; ein
/// Token aus `settings.json` wird beim ersten Aufruf übernommen.
fn load_or_init_token(app: &AppHandle, state: &AppState) -> CommandResult<String> {
    // **Die Härtung bleibt an diesem Weg** (spec-reviewer Runde 3): Vorher
    // schrieb das erstmalige Öffnen des Einstellungsschirms das Token in
    // die Datei und härtete sie dabei. Dieser Weg schreibt nicht mehr —
    // ohne diesen Aufruf bliebe `settings.json` bei den Umask-Rechten
    // stehen, mit denen ein anderes Modul sie angelegt hat. Das wäre
    // gegenüber vorher ein Rückschritt, auch wenn das Geheimnis jetzt
    // nicht mehr darin steht.
    settings_store::harden_settings_store_permissions(app);
    app_logic::mcp_token::load_or_init_token(
        state.credential_store.as_ref(),
        &SettingsJsonToken { app },
        &generate_token,
    )
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
    let ids_json: Vec<String> = ids.iter().map(|id| id.0.to_string()).collect();
    settings_store::update(app, |changes| {
        changes.set(ALLOWED_SERVERS_KEY, serde_json::json!(ids_json))
    })
}

fn load_confirm_timeout_secs<R: Runtime>(app: &AppHandle<R>) -> CommandResult<u64> {
    let store = app.store(SETTINGS_STORE_FILE)?;
    Ok(store
        .get(CONFIRM_TIMEOUT_SECS_KEY)
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| mcp_server::DEFAULT_CONFIRM_TIMEOUT.as_secs()))
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
    let token = load_or_init_token(app, state)?;
    state.mcp.token.set(token);
    let allowed = load_allowed_servers(app)?;
    *state.mcp.allowed_servers.lock().expect("Mutex vergiftet") = allowed;
    Ok(())
}

async fn build_dto(app: &AppHandle, state: &AppState) -> CommandResult<McpServerSettingsDto> {
    let enabled = state.mcp.runtime.lock().await.is_some();
    let token = load_or_init_token(app, state)?;
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
        confirm_timeout_secs: load_confirm_timeout_secs(app)?,
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
    let config = mcp_server::McpServerConfig {
        confirm_timeout: std::time::Duration::from_secs(load_confirm_timeout_secs(app)?),
        ..mcp_server::McpServerConfig::default()
    };
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
    settings_store::update(&app, |changes| {
        changes.set(ENABLED_KEY, serde_json::json!(enabled))
    })?;

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
    // Spec 0101, A12: „Erzeugen und Erneuern schreiben nur in die
    // Datenbank." Das neue Token wird dort auch zurückgelesen — ein
    // Erneuern, das nicht ankommt, ließe den Nutzer mit einem Token
    // dastehen, mit dem sich kein Client anmelden kann.
    let new_token = app_logic::mcp_token::regenerate_token(
        state.credential_store.as_ref(),
        &SettingsJsonToken { app: &app },
        &generate_token,
    )?;
    // Live-Effekt sofort, unabhängig davon, ob der Server gerade läuft
    // (Spec 0028, Abschnitt 9: "invalidiert das alte Token sofort") — ein
    // laufender Server prüft bei jedem Tool-Call gegen `state.mcp.token`,
    // eine explizite Benachrichtigung des Servers ist nicht nötig.
    state.mcp.token.set(new_token);
    build_dto(&app, &state).await
}

/// Spec 0028, Abschnitt 7/9. `confirm_timeout` wird beim Server-Start in den
/// `SmartSshMcpServer` einkompiliert (s. `mcp_server::config::serve`), lässt
/// sich also anders als das Token nicht am laufenden Server austauschen —
/// ein bereits laufender Server wird deshalb neu gestartet, damit die
/// Änderung ohne manuellen Aus-/Einschalt-Schritt sofort greift (statt nur
/// beim nächsten App-Start). Ein währenddessen wartender MCP-Tool-Call würde
/// dadurch mit einem Verbindungsfehler abbrechen — ein seltener,
/// vertretbarer Trade-off für eine vom Nutzer bewusst in den Einstellungen
/// vorgenommene Änderung, kein automatischer Hintergrundvorgang.
#[tauri::command]
pub async fn set_mcp_server_confirm_timeout_secs(
    app: AppHandle,
    state: State<'_, AppState>,
    secs: u64,
) -> CommandResult<McpServerSettingsDto> {
    settings_store::update(&app, |changes| {
        changes.set(CONFIRM_TIMEOUT_SECS_KEY, serde_json::json!(secs))
    })?;

    let was_running = state.mcp.runtime.lock().await.is_some();
    if was_running {
        stop_server_if_running(&state).await;
        start_server_if_not_running(&app, &state).await?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, test_app};
    use tauri_plugin_store::StoreExt;

    /// Spec 0028, Abschnitt 7: Regressionstest für den Audit-Fund, dass
    /// `start_server_if_not_running` den Server bislang immer mit
    /// `McpServerConfig::default()` (fest 5 Minuten) konstruierte, egal was
    /// in den Einstellungen stand. Ohne persistierten Wert muss weiterhin
    /// exakt der Backend-Default gelten.
    #[test]
    fn test_load_confirm_timeout_secs_defaults_to_backend_default_without_stored_value() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        if let Ok(store) = handle.store(SETTINGS_STORE_FILE) {
            store.delete(CONFIRM_TIMEOUT_SECS_KEY);
            let _ = store.save();
        }

        let secs = load_confirm_timeout_secs(handle).expect("laden darf nicht fehlschlagen");

        assert_eq!(secs, mcp_server::DEFAULT_CONFIRM_TIMEOUT.as_secs());

        if let Ok(store) = handle.store(SETTINGS_STORE_FILE) {
            store.delete(CONFIRM_TIMEOUT_SECS_KEY);
            let _ = store.save();
        }
    }

    /// Der eigentliche Kern des Fixes: ein zuvor gespeicherter Wert wird
    /// tatsächlich gelesen — das ist genau der Teil, der vorher nirgendwo
    /// ankam, weil `start_server_if_not_running` ihn nie abfragte.
    #[test]
    fn test_load_confirm_timeout_secs_returns_persisted_value() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        let store = handle.store(SETTINGS_STORE_FILE).expect("Store");
        store.set(CONFIRM_TIMEOUT_SECS_KEY, serde_json::json!(120u64));
        store.save().expect("Store konnte nicht gespeichert werden");

        let secs = load_confirm_timeout_secs(handle).expect("laden darf nicht fehlschlagen");

        assert_eq!(secs, 120);

        store.delete(CONFIRM_TIMEOUT_SECS_KEY);
        let _ = store.save();
    }

    /// Issue #40, regression: the hardening must hit the `settings.json`
    /// the store actually reads and writes. It used to build the path from
    /// `app_config_dir`, while `tauri-plugin-store` resolves against
    /// `BaseDirectory::AppData`. On Linux those are different directories
    /// (`~/.config/<id>` vs `~/.local/share/<id>`), so the real file kept
    /// its umask mode (typically 0644). On macOS both directories coincide,
    /// which is why this test only fails against the old code on Linux.
    #[cfg(unix)]
    #[test]
    fn test_harden_settings_store_permissions_targets_the_store_file() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        let store = handle.store(SETTINGS_STORE_FILE).expect("Store");
        store.set(CONFIRM_TIMEOUT_SECS_KEY, serde_json::json!(120u64));
        store.save().expect("Store konnte nicht gespeichert werden");

        let store_path = tauri_plugin_store::resolve_store_path(handle, SETTINGS_STORE_FILE)
            .expect("store path must resolve");
        std::fs::set_permissions(&store_path, std::fs::Permissions::from_mode(0o644))
            .expect("store file must exist after save");

        crate::settings_store::harden_settings_store_permissions(handle);

        let mode = std::fs::metadata(&store_path)
            .expect("store file must still exist")
            .permissions()
            .mode()
            & 0o777;

        store.delete(CONFIRM_TIMEOUT_SECS_KEY);
        let _ = store.save();

        assert_eq!(
            mode,
            0o600,
            "settings.json at {} must be hardened to 0600, was {mode:o}",
            store_path.display()
        );
    }
}
