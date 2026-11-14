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

use std::sync::atomic::{AtomicBool, Ordering};

pub use app_logic::activities::{
    Activity, ActivityHandle, ActivityOutcome, ActivityRegistry, ActivitySpec, ActivityState,
    ActivityUpdate, ResumeSpec,
};
use app_logic::activities::{
    DiscardFn, InterruptedActivity, QuitRequestedPayload, ResumeFn, ResumeRegistry,
};
use app_logic::error::{CommandError, CommandResult};
use tauri::Emitter;

use crate::event_emitter::TauriEventEmitter;

/// Managed state; the registry is created on first use because it needs the
/// `AppHandle` for its event emitter. Also holds the resume handlers (issue
/// #1327).
#[derive(Default)]
pub struct Activities(OnceLock<Arc<ActivityRegistry>>, ResumeHandlers);

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

/// Settings-store key of the activities a quit interrupted (issue #1327).
/// Entries hold kind, key, title, done and total; no URLs, no secrets.
const INTERRUPTED_KEY: &str = "interruptedActivities";

/// Resume handlers by kind, and whether the user already confirmed the quit.
#[derive(Default)]
pub(crate) struct ResumeHandlers {
    registry: ResumeRegistry,
    quit_confirmed: AtomicBool,
}

/// Registers the resume (and optional discard) handler of an activity kind.
/// Public for extension crates; call it during start-up. The first
/// registration of a kind wins.
pub fn register_resume_handler<R: Runtime>(
    app: &AppHandle<R>,
    kind: &str,
    resume: ResumeFn,
    discard: Option<DiscardFn>,
) -> bool {
    app.state::<Activities>()
        .1
        .registry
        .register(kind, resume, discard)
}

fn read_interrupted<R: Runtime>(app: &AppHandle<R>) -> Vec<InterruptedActivity> {
    use tauri_plugin_store::StoreExt;
    let Some(value) = app
        .store(crate::settings_store::settings_store_file())
        .ok()
        .and_then(|s| s.get(INTERRUPTED_KEY))
    else {
        return Vec::new();
    };
    // Entries that do not parse are dropped: they cannot be shown or resumed.
    serde_json::from_value::<Vec<serde_json::Value>>(value)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

fn write_interrupted<R: Runtime>(
    app: &AppHandle<R>,
    entries: &[InterruptedActivity],
) -> CommandResult<()> {
    crate::settings_store::update(app, |changes| {
        if entries.is_empty() {
            changes.delete(INTERRUPTED_KEY);
        } else {
            changes.set(
                INTERRUPTED_KEY,
                serde_json::to_value(entries).unwrap_or_default(),
            );
        }
    })
}

/// Whether a quit from outside the app (window close, menu, Cmd+Q) needs the
/// user's confirmation first. If so, asks the frontend and returns `true`
/// (the caller keeps the app open). An exit the app requests itself (update
/// restart, `app.exit`) never goes through here.
pub(crate) fn ask_before_quit<R: Runtime>(app: &AppHandle<R>) -> bool {
    if app
        .state::<Activities>()
        .1
        .quit_confirmed
        .load(Ordering::SeqCst)
    {
        return false;
    }
    let blockers = activities(app).quit_blockers();
    if blockers.is_empty() {
        return false;
    }
    if let Err(err) = app.emit(
        app_logic::event_names::QUIT_REQUESTED,
        QuitRequestedPayload {
            activities: blockers,
        },
    ) {
        // Without the dialog the user could never confirm; do not trap them:
        // quit as before.
        tracing::warn!(error = %err, "quit confirmation could not be requested");
        return false;
    }
    true
}

/// The user chose "Quit anyway": remember the resumable activities, then
/// exit. Cancel callbacks are not run: they would delete the partial files
/// that resuming needs, and the process ends right here anyway.
#[tauri::command]
pub fn quit_confirmed(app: AppHandle) {
    store_interrupted_snapshot(&app);
    app.state::<Activities>()
        .1
        .quit_confirmed
        .store(true, Ordering::SeqCst);
    app.exit(0);
}

pub(crate) fn store_interrupted_snapshot<R: Runtime>(app: &AppHandle<R>) {
    let snapshot = activities(app).interrupted_snapshot();
    if snapshot.is_empty() {
        return;
    }
    let mut all = read_interrupted(app);
    all.extend(snapshot);
    if let Err(err) = write_interrupted(app, &all) {
        tracing::warn!(code = ?err.code, "interrupted activities could not be stored");
    }
}

/// An interrupted activity as the activity center lists it. `native` is
/// `true` when a Rust handler owns the kind; otherwise the frontend's.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedRow {
    #[serde(flatten)]
    pub entry: InterruptedActivity,
    pub native: bool,
}

/// Stored interrupted activities with a handler. Entries whose kind has
/// neither a Rust handler nor one of `frontend_kinds` are removed silently.
#[tauri::command]
pub fn activities_interrupted(
    app: AppHandle,
    frontend_kinds: Vec<String>,
) -> CommandResult<Vec<InterruptedRow>> {
    interrupted_rows(&app, &frontend_kinds)
}

pub(crate) fn interrupted_rows<R: Runtime>(
    app: &AppHandle<R>,
    frontend_kinds: &[String],
) -> CommandResult<Vec<InterruptedRow>> {
    let state = app.state::<Activities>();
    let handlers = &state.1;
    let all = read_interrupted(app);
    let total = all.len();
    let rows: Vec<InterruptedRow> = all
        .into_iter()
        .filter_map(|entry| {
            let native = handlers.registry.has(&entry.kind);
            (native || frontend_kinds.contains(&entry.kind))
                .then_some(InterruptedRow { entry, native })
        })
        .collect();
    if rows.len() != total {
        let kept: Vec<InterruptedActivity> = rows.iter().map(|r| r.entry.clone()).collect();
        write_interrupted(app, &kept)?;
    }
    Ok(rows)
}

fn take_interrupted<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
) -> CommandResult<InterruptedActivity> {
    read_interrupted(app)
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| CommandError::with_code("unknown interrupted activity", "activity_unknown"))
}

fn remove_interrupted<R: Runtime>(app: &AppHandle<R>, id: &str) -> CommandResult<()> {
    let rest: Vec<_> = read_interrupted(app)
        .into_iter()
        .filter(|e| e.id != id)
        .collect();
    write_interrupted(app, &rest)
}

pub(crate) fn resume_interrupted<R: Runtime>(app: &AppHandle<R>, id: &str) -> CommandResult<()> {
    let entry = take_interrupted(app, id)?;
    app.state::<Activities>().1.registry.resume(&entry)?;
    remove_interrupted(app, id)
}

pub(crate) fn discard_interrupted<R: Runtime>(app: &AppHandle<R>, id: &str) -> CommandResult<()> {
    let entry = take_interrupted(app, id)?;
    app.state::<Activities>().1.registry.discard(&entry);
    remove_interrupted(app, id)
}

/// What to do with a stored interrupted activity.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterruptedAction {
    /// Rust handler: starts a fresh activity, then removes the entry.
    Resume,
    /// Rust handler: runs its discard handler (if any), then removes the entry.
    Discard,
    /// Frontend handler already ran: only removes the entry.
    Remove,
}

#[tauri::command]
pub fn activity_interrupted_resolve(
    app: AppHandle,
    id: String,
    action: InterruptedAction,
) -> CommandResult<()> {
    match action {
        InterruptedAction::Resume => resume_interrupted(&app, &id),
        InterruptedAction::Discard => discard_interrupted(&app, &id),
        InterruptedAction::Remove => remove_interrupted(&app, &id),
    }
}

/// Run-loop hook (issue #1327): quitting with running activities asks first.
/// Only a quit from outside (`code` is `None`: last window closed, Cmd+Q, menu)
/// is held back; `app.exit(code)` (update restart, "Quit anyway") carries a
/// code and goes through. Closing the last window is held back the same way.
pub(crate) fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } if ask_before_quit(app) => {
            api.prevent_exit();
        }
        tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if app.webview_windows().len() <= 1 && ask_before_quit(app) => {
            api.prevent_close();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, test_app};
    use std::sync::atomic::AtomicUsize;

    fn app_with_state() -> tauri::App<tauri::test::MockRuntime> {
        let app = test_app();
        app.manage(Activities::default());
        write_interrupted(app.handle(), &[]).unwrap();
        app
    }

    fn resumable_spec(kind: &str, key: &str) -> ActivitySpec {
        ActivitySpec {
            kind: kind.into(),
            key: key.into(),
            title_key: "t".into(),
            cancellable: true,
            resume: Some(ResumeSpec {
                kind: kind.into(),
                key: key.into(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn quit_without_running_activities_is_not_held_back() {
        let _g = lock();
        let app = app_with_state();
        assert!(!ask_before_quit(app.handle()));
    }

    #[test]
    fn quit_with_a_running_activity_is_held_back_until_confirmed() {
        let _g = lock();
        let app = app_with_state();
        let h = activities(app.handle()).start(resumable_spec("k", "a"));
        assert!(ask_before_quit(app.handle()));
        // A finished activity no longer blocks.
        h.succeed();
        assert!(!ask_before_quit(app.handle()));
    }

    #[test]
    fn quit_confirmed_flag_lets_the_exit_through() {
        let _g = lock();
        let app = app_with_state();
        let _h = activities(app.handle()).start(resumable_spec("k", "a"));
        app.state::<Activities>()
            .1
            .quit_confirmed
            .store(true, Ordering::SeqCst);
        assert!(!ask_before_quit(app.handle()));
    }

    #[test]
    fn snapshot_is_stored_and_unhandled_kinds_are_dropped_silently() {
        let _g = lock();
        let app = app_with_state();
        let handle = app.handle();
        let _a = activities(handle).start(resumable_spec("known", "m1"));
        let _b = activities(handle).start(resumable_spec("nobody-handles-this", "x"));
        store_interrupted_snapshot(handle);
        assert_eq!(read_interrupted(handle).len(), 2);

        assert!(register_resume_handler(
            handle,
            "known",
            Arc::new(|_| Ok(())),
            None
        ));
        let rows = interrupted_rows(handle, &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entry.kind, "known");
        assert!(rows[0].native);
        // The unhandled entry is gone from the store, not just hidden.
        let stored = read_interrupted(handle);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].kind, "known");
        write_interrupted(handle, &[]).unwrap();
    }

    #[test]
    fn frontend_kinds_keep_their_entries() {
        let _g = lock();
        let app = app_with_state();
        let handle = app.handle();
        let _a = activities(handle).start(resumable_spec("ext-kind", "x"));
        store_interrupted_snapshot(handle);
        let rows = interrupted_rows(handle, &["ext-kind".to_owned()]).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].native);
        write_interrupted(handle, &[]).unwrap();
    }

    #[test]
    fn resume_calls_the_handler_and_removes_the_entry_discard_calls_discard() {
        let _g = lock();
        let app = app_with_state();
        let handle = app.handle();
        let resumed = Arc::new(AtomicUsize::new(0));
        let discarded = Arc::new(AtomicUsize::new(0));
        let (r, d) = (Arc::clone(&resumed), Arc::clone(&discarded));
        register_resume_handler(
            handle,
            "known",
            Arc::new(move |_| {
                r.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            Some(Arc::new(move |_| {
                d.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let _a = activities(handle).start(resumable_spec("known", "m1"));
        let _b = activities(handle).start(resumable_spec("known", "m2"));
        store_interrupted_snapshot(handle);
        let ids: Vec<String> = read_interrupted(handle).into_iter().map(|e| e.id).collect();
        resume_interrupted(handle, &ids[0]).unwrap();
        discard_interrupted(handle, &ids[1]).unwrap();
        assert_eq!(resumed.load(Ordering::SeqCst), 1);
        assert_eq!(discarded.load(Ordering::SeqCst), 1);
        assert!(read_interrupted(handle).is_empty());
    }

    #[test]
    fn a_failing_resume_keeps_the_entry() {
        let _g = lock();
        let app = app_with_state();
        let handle = app.handle();
        register_resume_handler(
            handle,
            "known",
            Arc::new(|_| Err(CommandError::with_code("boom", "x"))),
            None,
        );
        let _a = activities(handle).start(resumable_spec("known", "m1"));
        store_interrupted_snapshot(handle);
        let id = read_interrupted(handle)[0].id.clone();
        assert!(resume_interrupted(handle, &id).is_err());
        assert_eq!(read_interrupted(handle).len(), 1);
        write_interrupted(handle, &[]).unwrap();
    }
}
