//! Optionale Aufbewahrungsdauer für persistente Chat-Sitzungen (Spec 0034,
//! Abschnitt 5) — "keine Ablaufdatum"-Grundsatz plus eine **optionale**,
//! global über `tauri-plugin-store` gespeicherte Einstellung (Spec-Text:
//! "global ... gespeichert, nicht pro Server"), Default `None` = niemals
//! automatisch löschen. Selbes Store-Muster wie
//! `crate::risk_second_opinion` (s. dortiger Kommentar).

use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use crate::error::CommandResult;
use crate::state::AppState;

const SETTINGS_STORE_FILE: &str = "settings.json";
const RETENTION_DAYS_KEY: &str = "chatSessionRetentionDays";

fn read_retention_days<R: Runtime>(app: &AppHandle<R>) -> Option<u32> {
    app.store(SETTINGS_STORE_FILE)
        .ok()?
        .get(RETENTION_DAYS_KEY)?
        .as_u64()
        .map(|v| v as u32)
}

#[tauri::command]
pub fn get_chat_session_retention_days(app: AppHandle) -> CommandResult<Option<u32>> {
    Ok(read_retention_days(&app))
}

/// `None` schaltet die automatische Löschung wieder ab (Spec 0034,
/// Abschnitt 5: Default, "niemals automatisch löschen"). Generisch über
/// `R: Runtime` (statt direkt im `#[tauri::command]`), damit Tests sie mit
/// `tauri::test::MockRuntime` statt der echten `Wry`-Runtime aufrufen
/// können — derselbe Grund wie bei `commands::ensure_first_run_notice_
/// acknowledged`.
fn write_retention_days<R: Runtime>(app: &AppHandle<R>, days: Option<u32>) -> CommandResult<()> {
    let store = app.store(SETTINGS_STORE_FILE)?;
    match days {
        Some(days) => store.set(RETENTION_DAYS_KEY, serde_json::json!(days)),
        None => {
            store.delete(RETENTION_DAYS_KEY);
        }
    }
    store.save()?;
    Ok(())
}

#[tauri::command]
pub fn set_chat_session_retention_days(app: AppHandle, days: Option<u32>) -> CommandResult<()> {
    write_retention_days(&app, days)
}

/// Spec 0034, Abschnitt 5: "räumt ein Hintergrund-Job beim App-Start
/// Sitzungen auf, deren `ended_at` älter als der konfigurierte Zeitraum
/// ist" — No-op, solange keine Aufbewahrungsdauer konfiguriert ist (Default
/// `None`). Best-effort wie `mcp_settings::autostart_if_enabled` (dieselbe
/// "lieber kein Aufräumen als ein blockierter App-Start"-Haltung): ein
/// Fehler hier wird nur geloggt, verhindert den App-Start nicht.
pub async fn cleanup_old_chat_sessions_on_startup<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
) {
    let Some(days) = read_retention_days(app) else {
        return;
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
    match state
        .chat_session_store
        .delete_ended_sessions_before(cutoff)
        .await
    {
        Ok(count) if count > 0 => {
            tracing::info!(count, retention_days = days, "old chat sessions cleaned up");
        }
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(error = %err, "chat session retention cleanup failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, test_app};

    #[test]
    fn test_retention_days_defaults_to_none_without_stored_value() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();

        assert_eq!(read_retention_days(handle), None);
    }

    #[test]
    fn test_set_then_read_retention_days_round_trips() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();

        write_retention_days(handle, Some(30)).unwrap();
        assert_eq!(read_retention_days(handle), Some(30));

        // `None` schaltet wieder ab (Spec 0034, Abschnitt 5: Default).
        write_retention_days(handle, None).unwrap();
        assert_eq!(read_retention_days(handle), None);
    }
}
