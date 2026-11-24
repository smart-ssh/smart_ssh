//! Issue #1193: automatic switch-off of the elevated file-browser channel
//! after a period without elevated activity (Spec 0067, A5).
//!
//! The timer lives in the backend, one task per channel
//! ([`ElevatedSftpRegistry::start_idle_timer`]), so it works regardless of
//! what the webview does. When it expires the channel is revoked through the
//! same path as a manual switch-off. The timer never interrupts a running
//! operation: while one runs, the task waits for it and then starts over.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::Value;
use tauri_plugin_store::StoreExt;
use tokio::sync::watch;
use tokio::time::Instant;

use ssh_manager_core::sync::lock_tolerating_poison;

use super::ElevatedSftpSlot;
use crate::file_ops::{FileOps, StopReason};
use app_logic::state::SessionId;

/// Key in `settings.json`. A security setting: written only by
/// [`set_elevated_idle_timeout`], listed in `ui_preferences::SECURITY_KEYS`.
pub(crate) const IDLE_TIMEOUT_KEY: &str = "elevatedIdleTimeoutMinutes";
/// Default (and fallback for a missing or invalid stored value).
pub(crate) const DEFAULT_IDLE_MINUTES: u32 = 15;
/// The stored value for "never".
pub(crate) const NEVER: u32 = 0;
const ALLOWED_MINUTES: [u32; 5] = [5, 15, 30, 60, NEVER];

pub(crate) fn is_allowed(minutes: u32) -> bool {
    ALLOWED_MINUTES.contains(&minutes)
}

/// Evaluates a stored value. Fail-safe towards "switch off": a missing,
/// non-numeric or unknown value yields the default, never "never".
pub(crate) fn from_stored(value: Option<Value>) -> u32 {
    value
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .filter(|m| is_allowed(*m))
        .unwrap_or(DEFAULT_IDLE_MINUTES)
}

type Notifier = Arc<dyn Fn(SessionId, u32) + Send + Sync>;
/// Starts a timer future in the task scope of its session.
pub(crate) type Spawner = Arc<
    dyn Fn(SessionId, std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>)
        + Send
        + Sync,
>;

/// Shared by the registry and all channel timers.
pub(crate) struct IdleShared {
    minutes: watch::Sender<u32>,
    notifier: StdMutex<Option<Notifier>>,
    spawner: StdMutex<Option<Spawner>>,
}

impl Default for IdleShared {
    fn default() -> Self {
        Self {
            minutes: watch::channel(DEFAULT_IDLE_MINUTES).0,
            notifier: StdMutex::new(None),
            spawner: StdMutex::new(None),
        }
    }
}

impl IdleShared {
    pub(crate) fn set_minutes(&self, minutes: u32) {
        self.minutes.send_replace(minutes);
    }

    pub(crate) fn set_notifier(&self, notifier: Notifier) {
        *lock_tolerating_poison(&self.notifier) = Some(notifier);
    }

    pub(crate) fn set_spawner(&self, spawner: Spawner) {
        *lock_tolerating_poison(&self.spawner) = Some(spawner);
    }

    pub(crate) fn spawner(&self) -> Option<Spawner> {
        lock_tolerating_poison(&self.spawner).clone()
    }

    fn notify(&self, session_id: SessionId, minutes: u32) {
        let notifier = lock_tolerating_poison(&self.notifier).clone();
        if let Some(notifier) = notifier {
            notifier(session_id, minutes);
        }
    }
}

/// Time of the last elevated activity of one channel.
#[derive(Clone)]
pub(crate) struct Activity(Arc<StdMutex<Instant>>);

impl Default for Activity {
    fn default() -> Self {
        Self(Arc::new(StdMutex::new(Instant::now())))
    }
}

impl Activity {
    pub(crate) fn touch(&self) {
        *lock_tolerating_poison(&self.0) = Instant::now();
    }

    fn last(&self) -> Instant {
        *lock_tolerating_poison(&self.0)
    }
}

pub(super) struct IdleTimer {
    pub(super) slots: Arc<StdMutex<HashMap<SessionId, ElevatedSftpSlot>>>,
    pub(super) ops: Arc<FileOps>,
    pub(super) shared: Arc<IdleShared>,
    pub(super) session_id: SessionId,
    pub(super) slot: ElevatedSftpSlot,
}

impl IdleTimer {
    pub(super) async fn run(self) {
        let mut rx = self.shared.minutes.subscribe();
        loop {
            if self.slot.is_revoked() {
                return;
            }
            let minutes = *rx.borrow_and_update();
            if minutes == NEVER {
                tokio::select! {
                    changed = rx.changed() => if changed.is_err() { return; },
                    () = self.slot.revoked_wake().notified() => return,
                }
                continue;
            }
            if self.ops.has_elevated(self.session_id) {
                // A running operation is activity; the timer starts again
                // when it has ended.
                self.ops.wait_until_elevated_idle(self.session_id).await;
                self.slot.activity().touch();
                continue;
            }
            let deadline =
                self.slot.activity().last() + Duration::from_secs(u64::from(minutes) * 60);
            if Instant::now() >= deadline {
                if self.expire(minutes).await {
                    return;
                }
                continue;
            }
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => {}
                changed = rx.changed() => if changed.is_err() { return; },
                () = self.slot.revoked_wake().notified() => return,
            }
        }
    }

    /// Revokes the channel like a manual switch-off. `false`: not expired
    /// after all (an operation began meanwhile), keep watching.
    async fn expire(&self, minutes: u32) -> bool {
        {
            let mut slots = lock_tolerating_poison(&self.slots);
            if !slots
                .get(&self.session_id)
                .is_some_and(|s| s.same_channel(&self.slot))
            {
                // Already revoked or replaced by someone else.
                return true;
            }
            if self.ops.has_elevated(self.session_id) {
                return false;
            }
            slots.remove(&self.session_id);
            self.slot.mark_revoked();
            self.ops.stop_elevated(self.session_id, StopReason::Revoked);
        }
        if self.slot.revoke().await {
            tracing::info!(
                source = "automatic",
                reason = "inactivity",
                minutes,
                "file browser elevated rights disabled"
            );
            self.shared.notify(self.session_id, minutes);
        }
        true
    }
}

/// Reads the stored idle timeout (minutes, `0` = never).
pub(crate) fn read_stored<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> u32 {
    match app.store(crate::settings_store::settings_store_file()) {
        Ok(store) => from_stored(store.get(IDLE_TIMEOUT_KEY)),
        Err(_) => DEFAULT_IDLE_MINUTES,
    }
}

#[tauri::command]
pub fn get_elevated_idle_timeout(app: tauri::AppHandle) -> u32 {
    read_stored(&app)
}

#[tauri::command]
pub fn set_elevated_idle_timeout(
    app: tauri::AppHandle,
    registry: tauri::State<'_, super::ElevatedSftpRegistry>,
    minutes: u32,
) -> app_logic::error::CommandResult<()> {
    write_timeout(&app, &registry, minutes)
}

pub(crate) fn write_timeout<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    registry: &super::ElevatedSftpRegistry,
    minutes: u32,
) -> app_logic::error::CommandResult<()> {
    if !is_allowed(minutes) {
        return Err(app_logic::error::CommandError::with_code(
            format!("invalid idle timeout: {minutes}"),
            app_logic::error::UI_PREFERENCE_REJECTED,
        ));
    }
    crate::settings_store::update(app, |changes| {
        changes.set(IDLE_TIMEOUT_KEY, serde_json::json!(minutes))
    })?;
    registry.idle().set_minutes(minutes);
    Ok(())
}

/// Applies the stored value once the store plugin exists (after unlocking).
pub(crate) fn reload_stored<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let registry = app.state::<super::ElevatedSftpRegistry>();
    registry.idle().set_minutes(read_stored(app));
}

/// Wires the registry to the app at startup: the stored value and the event
/// that tells the webview about an automatic switch-off.
pub(crate) fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let registry = app.state::<super::ElevatedSftpRegistry>();
    // With a master password the store plugin exists only after unlocking
    // (Spec 0101, A16); `reload_stored` applies the value then.
    let pending = app.try_state::<crate::extension_plugins::UnlockedPluginsPendingSlot>();
    if !pending.is_some_and(|slot| slot.0) {
        registry.idle().set_minutes(read_stored(app));
    }
    let scopes = app.clone();
    registry
        .idle()
        .set_spawner(Arc::new(move |session_id, fut| {
            crate::background_tasks::session_scope(&scopes, session_id)
                .spawn("elevated-idle-timer", fut);
        }));
    let handle = app.clone();
    registry
        .idle()
        .set_notifier(Arc::new(move |session_id, minutes| {
            let emitter = crate::event_emitter::TauriEventEmitter(handle.clone());
            app_logic::elevation_events::emit_elevation_auto_disabled(
                &emitter, session_id, minutes,
            );
        }));
}

#[cfg(test)]
mod tests;
