//! Issue #879: wiring of the process-wide `TaskSupervisor` into Tauri.

use std::sync::Arc;
use std::time::Duration;

use app_logic::tasks::{TaskScope, TaskSupervisor};

/// One supervisor per process. Spawns onto the Tauri runtime, because setup
/// code runs outside a Tokio context.
pub(crate) fn new_supervisor() -> Arc<TaskSupervisor> {
    Arc::new(TaskSupervisor::with_handle(tauri::async_runtime::block_on(
        async { tokio::runtime::Handle::current() },
    )))
}

/// The app scope: tasks spawned here end at app exit.
pub(crate) fn app_scope(app: &tauri::AppHandle) -> TaskScope {
    use tauri::Manager;
    app.state::<Arc<TaskSupervisor>>().app_scope()
}

/// Exit steps: stop background services (issue #669; bounded by the
/// per-service timeout), then cancel and await the app-scope tasks. Both are
/// bounded, so a hanging one cannot block exit.
pub(crate) fn shutdown_at_exit(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(host) = app.try_state::<crate::ServiceHost>() {
        tauri::async_runtime::block_on(host.stop_all());
    }
    if let Some(tasks) = app.try_state::<Arc<TaskSupervisor>>() {
        tauri::async_runtime::block_on(tasks.shutdown(Duration::from_secs(3)));
    }
}
