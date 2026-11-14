//! Activity center (issue #1325, Spec 0116): thin Tauri layer over
//! `app_logic::activities`. The registry holds every long-running activity
//! the user can see in the header; Rust code (including extension crates)
//! starts one with [`activities`]`(&app).start(spec)`, frontend code through
//! the `activity_*` commands.
//!
//! Not the `TaskSupervisor` (`background_tasks`): that is about task
//! lifetimes, this is about what the user sees. An activity may spawn its
//! work through `app_scope()`.

use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Manager, Runtime};

pub use app_logic::activities::{
    Activity, ActivityHandle, ActivityOutcome, ActivityRegistry, ActivitySpec, ActivityState,
    ActivityUpdate,
};
use app_logic::error::CommandResult;

use crate::event_emitter::TauriEventEmitter;

/// Managed state; the registry is created on first use because it needs the
/// `AppHandle` for its event emitter.
#[derive(Default)]
pub struct Activities(OnceLock<Arc<ActivityRegistry>>);

/// The process-wide activity registry. Public for extension crates.
pub fn activities<R: Runtime>(app: &AppHandle<R>) -> Arc<ActivityRegistry> {
    let state = app.state::<Activities>();
    Arc::clone(
        state
            .0
            .get_or_init(|| ActivityRegistry::new(Arc::new(TauriEventEmitter(app.clone())))),
    )
}

/// Snapshot for a window that opens later or a remounted UI.
#[tauri::command]
pub fn activities_list(app: AppHandle) -> Vec<Activity> {
    activities(&app).list()
}

#[tauri::command]
pub fn activity_cancel(app: AppHandle, id: String) -> CommandResult<()> {
    activities(&app).cancel(&id)
}

#[tauri::command]
pub fn activity_dismiss(app: AppHandle, id: String) -> CommandResult<()> {
    activities(&app).dismiss(&id)
}

/// For activities whose work runs in the frontend; returns the new id.
#[tauri::command]
pub fn activity_start(app: AppHandle, spec: ActivitySpec) -> String {
    activities(&app).start_raw(spec)
}

#[tauri::command]
pub fn activity_update(app: AppHandle, id: String, update: ActivityUpdate) {
    activities(&app).update(&id, update.into());
}

#[tauri::command]
pub fn activity_finish(app: AppHandle, id: String, outcome: ActivityOutcome) {
    activities(&app).finish(&id, outcome);
}
