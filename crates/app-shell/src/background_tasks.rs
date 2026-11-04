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

/// The app scope: tasks spawned here end at app exit. Exported from the crate
/// root so extension crates can spawn supervised tasks (Spec 0038).
pub fn app_scope<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> TaskScope {
    use tauri::Manager;
    app.state::<Arc<TaskSupervisor>>().app_scope()
}

/// Issue #881: the scope of one session. Tasks spawned here end when the
/// session is closed (`CloseContext::close`).
pub(crate) fn session_scope<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: app_logic::state::SessionId,
) -> TaskScope {
    use tauri::Manager;
    app.state::<Arc<TaskSupervisor>>().session_scope(session_id)
}

/// Hands the session scope to a transport (SFTP tap pumps), see
/// `SshTransport::set_task_spawner`.
pub(crate) fn session_spawner<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: app_logic::state::SessionId,
) -> ssh_manager_core::ssh::TaskSpawner {
    let scope = session_scope(app, session_id);
    ssh_manager_core::ssh::TaskSpawner::new(move |name, fut| scope.spawn(name, fut))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::test::{mock_builder, mock_context, noop_assets};

    /// Issue #1164: an extension reaches the app scope from an `AppHandle`
    /// and its task is cancelled by `TaskSupervisor::shutdown`.
    #[test]
    fn test_app_scope_task_is_cancelled_by_supervisor_shutdown() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let supervisor = new_supervisor();
        tauri::Manager::manage(&app, supervisor.clone());

        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        tauri::async_runtime::block_on(async {
            assert!(app_scope(app.handle()).spawn("ext-probe-1164", async move {
                struct Guard(Arc<AtomicBool>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _g = Guard(flag);
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            }));
            started_rx.await.expect("task started");
            let reports = supervisor.shutdown(Duration::from_secs(5)).await;
            assert!(reports.iter().all(|r| !r.timed_out));
        });
        assert!(cancelled.load(Ordering::SeqCst));
    }
}
