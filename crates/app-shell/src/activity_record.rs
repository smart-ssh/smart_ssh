//! Activity record glue of the app shell (issue #1447): the "Record active
//! minutes" setting, closing sessions a previous run left open, and the
//! `app_exit` entry at shutdown.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use app_logic::error::CommandResult;
use app_logic::session::activity::{new_active_minutes_flag, ActiveMinutesFlag};
use app_logic::state::AppState;
use ssh_manager_core::audit::LedgerConnectionEvent;

use crate::settings_store;

const RECORD_ACTIVE_MINUTES_KEY: &str = "recordActiveMinutes";

/// Upper bound for the exit-time writes; the app must not hang at exit.
const EXIT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// Managed wrapper so every session shares one switch.
pub(crate) struct RecordActiveMinutes(ActiveMinutesFlag);

/// Default on: a missing key, a non-boolean value or an unreadable store all
/// mean "on" (the setting only ever reduces what is recorded).
pub(crate) fn read_setting<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(store) = app.store(settings_store::settings_store_file()) else {
        return true;
    };
    store
        .get(RECORD_ACTIVE_MINUTES_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// The shared switch, created from the stored setting on first use.
pub(crate) fn flag<R: Runtime>(app: &AppHandle<R>) -> ActiveMinutesFlag {
    if let Some(state) = app.try_state::<RecordActiveMinutes>() {
        return state.0.clone();
    }
    let flag = new_active_minutes_flag();
    flag.store(read_setting(app), Ordering::Relaxed);
    app.manage(RecordActiveMinutes(flag.clone()));
    // Another thread may have managed first: use whatever is managed.
    app.try_state::<RecordActiveMinutes>()
        .map_or(flag, |s| s.0.clone())
}

#[tauri::command]
pub fn get_record_active_minutes(app: AppHandle) -> CommandResult<bool> {
    Ok(read_setting(&app))
}

/// Writes the setting, then applies it to open sessions at once.
pub(crate) fn write_setting<R: Runtime>(app: &AppHandle<R>, value: bool) -> CommandResult<()> {
    settings_store::update(app, |changes| {
        changes.set(RECORD_ACTIVE_MINUTES_KEY, serde_json::json!(value))
    })?;
    flag(app).store(value, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn set_record_active_minutes(app: AppHandle, value: bool) -> CommandResult<()> {
    write_setting(&app, value)
}

/// Closes recorded sessions a previous run left open (crash, killed
/// process). Best effort, logged.
pub(crate) async fn close_open_sessions_on_startup(
    state: &AppState,
    started: chrono::DateTime<chrono::Utc>,
) {
    match state
        .activity_session_store
        .close_open_sessions(started)
        .await
    {
        Ok(0) => {}
        Ok(count) => tracing::info!(
            count,
            "recorded sessions left open by a previous run closed"
        ),
        Err(err) => tracing::warn!(error = %err, "open recorded sessions could not be closed"),
    }
}

/// At app exit: every open, connected session gets `disconnected (app_exit)`
/// and its recorded session is ended. Bounded by [`EXIT_WRITE_TIMEOUT`].
pub(crate) async fn record_app_exit(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let work = async {
        for entry in state.sessions.snapshot() {
            let Some(session) = state.sessions.get(entry.session_id) else {
                continue;
            };
            // A lost connection already recorded its own disconnect.
            if entry.status != app_logic::events::ConnectionStatus::Disconnected {
                app_logic::session::activity::record_connection_event(
                    &session,
                    LedgerConnectionEvent::Disconnected,
                    Some("app_exit"),
                )
                .await;
            }
            app_logic::session::activity::end_recorded_session(&session).await;
        }
    };
    if tokio::time::timeout(EXIT_WRITE_TIMEOUT, work)
        .await
        .is_err()
    {
        tracing::warn!("activity record at exit timed out");
    }
}
