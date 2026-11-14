//! Activity center model and registry (issue #1325): long-running work the
//! user started (downloads, installs) is tracked here and shown in one place
//! in the header. Pure logic, no Tauri: events go out through
//! [`EventEmitter`], so the registry is unit-testable with a collecting
//! emitter and a fake clock. `app-shell` wires it into Tauri.
//!
//! Not to be confused with `tasks` (the internal `TaskSupervisor`): that is
//! about task lifetimes, this is about what the user sees.
//!
//! Activities carry no secrets: titles, status keys and error messages are
//! shown to the user as they are, so callers must never put credentials or
//! URLs with tokens into them.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ssh_manager_core::sync::lock_tolerating_poison;
use uuid::Uuid;

use crate::error::{CommandError, CommandResult};
use crate::event_names as names;
use crate::events::EventEmitter;

/// Progress events per activity are at most this far apart (4 per second).
pub const PROGRESS_THROTTLE_MS: u64 = 250;

/// Error code of an activity whose handle was dropped without finishing.
pub const ERR_DROPPED: &str = "activity_dropped";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum ActivityState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Full state of one activity; also the payload of `activity-changed`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct Activity {
    pub id: String,
    /// What kind of work this is (e.g. `speech-model`), for lookups.
    pub kind: String,
    /// Identifies the subject within the kind (e.g. the model id).
    pub key: String,
    pub title_key: String,
    pub title_params: BTreeMap<String, String>,
    pub state: ActivityState,
    pub done: u64,
    pub total: Option<u64>,
    pub status_key: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub cancellable: bool,
    /// Unix milliseconds.
    pub started_at: u64,
    /// `true` on the final event after `dismiss`: remove the row.
    pub dismissed: bool,
}

/// What a caller gives to start an activity.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ActivitySpec {
    pub kind: String,
    #[serde(default)]
    pub key: String,
    pub title_key: String,
    #[serde(default)]
    pub title_params: BTreeMap<String, String>,
    #[serde(default)]
    pub cancellable: bool,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Partial update of a running activity as sent by the frontend
/// (command `activity_update`); absent fields stay unchanged. An empty
/// `status_key` clears the status text.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ActivityUpdate {
    #[serde(default)]
    pub done: Option<u64>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub status_key: Option<String>,
}

/// Internal form of an update where "clear" is expressible.
#[derive(Debug, Clone, Default)]
pub struct ActivityPatch {
    pub done: Option<u64>,
    pub total: Option<Option<u64>>,
    pub status_key: Option<Option<String>>,
}

impl From<ActivityUpdate> for ActivityPatch {
    fn from(u: ActivityUpdate) -> Self {
        Self {
            done: u.done,
            total: u.total.map(Some),
            status_key: u
                .status_key
                .map(|k| if k.is_empty() { None } else { Some(k) }),
        }
    }
}

/// How an activity ends (command `activity_finish`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum ActivityOutcome {
    Succeeded,
    #[serde(rename_all = "camelCase")]
    Failed {
        code: String,
        message: String,
    },
}

type CancelCallback = Box<dyn FnOnce() + Send>;
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

struct Entry {
    activity: Activity,
    on_cancel: Option<CancelCallback>,
    /// Clock value of the last progress event sent for this activity.
    last_emit: Option<u64>,
}

#[derive(Default)]
struct Inner {
    /// Insertion order = start order.
    order: Vec<String>,
    entries: HashMap<String, Entry>,
}

pub struct ActivityRegistry {
    emitter: Arc<dyn EventEmitter>,
    clock: Clock,
    inner: Mutex<Inner>,
}

impl ActivityRegistry {
    pub fn new(emitter: Arc<dyn EventEmitter>) -> Arc<Self> {
        Self::with_clock(emitter, Arc::new(unix_ms))
    }

    pub fn with_clock(emitter: Arc<dyn EventEmitter>, clock: Clock) -> Arc<Self> {
        Arc::new(Self {
            emitter,
            clock,
            inner: Mutex::new(Inner::default()),
        })
    }

    /// Starts an activity and returns a handle for the owning Rust code.
    pub fn start(self: &Arc<Self>, spec: ActivitySpec) -> ActivityHandle {
        let id = self.start_raw(spec);
        ActivityHandle {
            registry: Arc::clone(self),
            id,
        }
    }

    /// Starts an activity driven from elsewhere (the frontend); returns its id.
    pub fn start_raw(&self, spec: ActivitySpec) -> String {
        let now = (self.clock)();
        let activity = Activity {
            id: Uuid::new_v4().to_string(),
            kind: spec.kind,
            key: spec.key,
            title_key: spec.title_key,
            title_params: spec.title_params,
            state: ActivityState::Running,
            done: 0,
            total: spec.total,
            status_key: None,
            error_code: None,
            error_message: None,
            cancellable: spec.cancellable,
            started_at: now,
            dismissed: false,
        };
        let id = activity.id.clone();
        {
            let mut inner = lock_tolerating_poison(&self.inner);
            inner.order.push(id.clone());
            inner.entries.insert(
                id.clone(),
                Entry {
                    activity: activity.clone(),
                    on_cancel: None,
                    last_emit: Some(now),
                },
            );
        }
        self.emit(&activity);
        id
    }

    /// Snapshot in start order, for a window that opens later.
    pub fn list(&self) -> Vec<Activity> {
        let inner = lock_tolerating_poison(&self.inner);
        inner
            .order
            .iter()
            .filter_map(|id| inner.entries.get(id).map(|e| e.activity.clone()))
            .collect()
    }

    /// Applies a partial update to a running activity. Progress-only changes
    /// are throttled; a changed status text is sent at once. Updates to an
    /// activity that is not running (or unknown) are ignored.
    pub fn update(&self, id: &str, update: ActivityPatch) {
        let now = (self.clock)();
        let to_emit = {
            let mut inner = lock_tolerating_poison(&self.inner);
            let Some(entry) = inner.entries.get_mut(id) else {
                return;
            };
            if entry.activity.state != ActivityState::Running {
                return;
            }
            let a = &mut entry.activity;
            let mut status_changed = false;
            if let Some(done) = update.done {
                a.done = done;
            }
            if let Some(total) = update.total {
                a.total = total;
            }
            if let Some(status) = update.status_key {
                status_changed = a.status_key != status;
                a.status_key = status;
            }
            let due = match entry.last_emit {
                // A clock that went backwards counts as due.
                Some(last) => now < last || now - last >= PROGRESS_THROTTLE_MS,
                None => true,
            };
            if status_changed || due {
                entry.last_emit = Some(now);
                Some(entry.activity.clone())
            } else {
                None
            }
        };
        if let Some(a) = to_emit {
            self.emit(&a);
        }
    }

    /// Registers the callback run when the user cancels. Marks the activity
    /// cancellable (and tells the UI).
    pub fn set_cancel_callback(&self, id: &str, callback: CancelCallback) {
        let to_emit = {
            let mut inner = lock_tolerating_poison(&self.inner);
            let Some(entry) = inner.entries.get_mut(id) else {
                return;
            };
            if entry.activity.state != ActivityState::Running {
                return;
            }
            entry.on_cancel = Some(callback);
            if entry.activity.cancellable {
                None
            } else {
                entry.activity.cancellable = true;
                entry.last_emit = Some((self.clock)());
                Some(entry.activity.clone())
            }
        };
        if let Some(a) = to_emit {
            self.emit(&a);
        }
    }

    /// Ends a running activity. The final state is always sent.
    pub fn finish(&self, id: &str, outcome: ActivityOutcome) {
        let to_emit = {
            let mut inner = lock_tolerating_poison(&self.inner);
            let Some(entry) = inner.entries.get_mut(id) else {
                return;
            };
            if entry.activity.state != ActivityState::Running {
                return;
            }
            let a = &mut entry.activity;
            match outcome {
                ActivityOutcome::Succeeded => {
                    a.state = ActivityState::Succeeded;
                    if let Some(total) = a.total {
                        a.done = total;
                    }
                }
                ActivityOutcome::Failed { code, message } => {
                    a.state = ActivityState::Failed;
                    a.error_code = Some(code);
                    a.error_message = Some(message);
                }
            }
            a.status_key = None;
            a.cancellable = false;
            entry.on_cancel = None;
            entry.activity.clone()
        };
        self.emit(&to_emit);
    }

    /// The user cancelled: runs the cancel callback (outside the lock) and
    /// ends the activity as `cancelled`. Not cancellable ⇒ error.
    pub fn cancel(&self, id: &str) -> CommandResult<()> {
        let (callback, activity) = {
            let mut inner = lock_tolerating_poison(&self.inner);
            let entry = inner
                .entries
                .get_mut(id)
                .ok_or_else(|| CommandError::with_code("unknown activity", "activity_unknown"))?;
            if entry.activity.state != ActivityState::Running || !entry.activity.cancellable {
                return Err(CommandError::with_code(
                    "activity cannot be cancelled",
                    "activity_not_cancellable",
                ));
            }
            entry.activity.state = ActivityState::Cancelled;
            entry.activity.cancellable = false;
            entry.activity.status_key = None;
            (entry.on_cancel.take(), entry.activity.clone())
        };
        self.emit(&activity);
        if let Some(cb) = callback {
            cb();
        }
        Ok(())
    }

    /// Removes a finished activity and tells the UI to drop the row.
    /// A running activity cannot be dismissed.
    pub fn dismiss(&self, id: &str) -> CommandResult<()> {
        let removed = {
            let mut inner = lock_tolerating_poison(&self.inner);
            match inner.entries.get(id) {
                None => return Ok(()),
                Some(e) if e.activity.state == ActivityState::Running => {
                    return Err(CommandError::with_code(
                        "activity is still running",
                        "activity_running",
                    ));
                }
                Some(_) => {}
            }
            inner.order.retain(|x| x != id);
            inner.entries.remove(id)
        };
        if let Some(mut e) = removed {
            e.activity.dismissed = true;
            self.emit(&e.activity);
        }
        Ok(())
    }

    fn emit(&self, activity: &Activity) {
        match serde_json::to_value(activity) {
            Ok(v) => self.emitter.emit_event(names::ACTIVITY_CHANGED, v),
            Err(err) => tracing::warn!(error = %err, "activity event not serialisable"),
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Owner's handle for Rust code (also for extension crates). Dropping it
/// while the activity still runs marks the activity failed.
pub struct ActivityHandle {
    registry: Arc<ActivityRegistry>,
    id: String,
}

impl ActivityHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn progress(&self, done: u64, total: Option<u64>) {
        self.registry.update(
            &self.id,
            ActivityPatch {
                done: Some(done),
                total: Some(total),
                status_key: None,
            },
        );
    }

    /// Sets the status text key (e.g. "Verifying"); `None` clears it.
    pub fn status(&self, key: Option<&str>) {
        self.registry.update(
            &self.id,
            ActivityPatch {
                done: None,
                total: None,
                status_key: Some(key.map(str::to_owned)),
            },
        );
    }

    /// Runs `callback` when the user cancels; makes the activity cancellable.
    pub fn on_cancel(&self, callback: impl FnOnce() + Send + 'static) {
        self.registry
            .set_cancel_callback(&self.id, Box::new(callback));
    }

    pub fn succeed(self) {
        self.registry.finish(&self.id, ActivityOutcome::Succeeded);
    }

    pub fn fail(self, code: &str, message: &str) {
        self.registry.finish(
            &self.id,
            ActivityOutcome::Failed {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        );
    }
}

impl Drop for ActivityHandle {
    fn drop(&mut self) {
        // No-op when already finished or cancelled (`finish` checks the state).
        self.registry.finish(
            &self.id,
            ActivityOutcome::Failed {
                code: ERR_DROPPED.to_owned(),
                message: "The activity ended unexpectedly.".to_owned(),
            },
        );
    }
}

#[cfg(test)]
mod tests;
