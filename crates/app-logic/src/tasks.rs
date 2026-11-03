//! Issue #879: owner for long-lived background tasks.
//!
//! Every background task the app starts belongs to a *scope* of the
//! [`TaskSupervisor`]. A scope can cancel its tasks and wait for them with a
//! deadline. This issue only uses the **app scope** (ends at app exit); the
//! scope key leaves room for per-session scopes.
//!
//! A task spawned with [`TaskScope::spawn`] is raced against the scope's
//! cancellation token, so cancelling drops the future at its next `.await`.
//! Dropping runs every `Drop` guard in it — e.g. a pending confirmation
//! (`RegisteredConfirmation`) is removed from the registry exactly as if the
//! dialog had been closed; cancellation never turns into an approval.
//!
//! Blocking work ([`TaskScope::spawn_blocking`]) is tracked and awaited at
//! shutdown up to the deadline, but cannot be cancelled.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use uuid::Uuid;

use ssh_manager_core::sync::lock_tolerating_poison;

/// Identifies a scope. The app scope is one key; session scopes follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKey {
    /// Lives until the app exits (or, later, until it is locked again).
    App,
    /// Tasks of one open session; ends when the session is closed
    /// ([`TaskSupervisor::close_session_scope`]).
    Session(Uuid),
}

/// Outcome of a scope shutdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    /// `true` when the deadline passed with tasks still running.
    pub timed_out: bool,
    /// Names of the tasks still running at the deadline (empty if none).
    pub still_running: Vec<&'static str>,
}

/// One scope: a cancellation token plus a tracker for its tasks.
#[derive(Clone)]
pub struct TaskScope {
    key: ScopeKey,
    token: CancellationToken,
    tracker: TaskTracker,
    running: Arc<Mutex<HashMap<u64, &'static str>>>,
    next_id: Arc<AtomicU64>,
    handle: Handle,
}

/// Removes the task's name from the running list when it ends — normally,
/// by cancellation or by panic.
struct RunningEntry {
    running: Arc<Mutex<HashMap<u64, &'static str>>>,
    id: u64,
}

impl Drop for RunningEntry {
    fn drop(&mut self) {
        lock_tolerating_poison(&self.running).remove(&self.id);
    }
}

impl TaskScope {
    fn new(key: ScopeKey, parent: &CancellationToken, handle: Handle) -> Self {
        Self {
            key,
            token: parent.child_token(),
            tracker: TaskTracker::new(),
            running: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(0)),
            handle,
        }
    }

    fn register(&self, name: &'static str) -> RunningEntry {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        lock_tolerating_poison(&self.running).insert(id, name);
        RunningEntry {
            running: Arc::clone(&self.running),
            id,
        }
    }

    /// Runs `fut` until it ends or the scope is cancelled. Returns `false`
    /// (and does not run `fut`) when the scope is already cancelled or closed.
    pub fn spawn<F>(&self, name: &'static str, fut: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if self.token.is_cancelled() || self.tracker.is_closed() {
            return false;
        }
        let entry = self.register(name);
        let token = self.token.clone();
        self.tracker.spawn_on(
            async move {
                let _entry = entry;
                tokio::select! {
                    // Cancellation wins ties: a cancelled task must not
                    // start another step.
                    biased;
                    () = token.cancelled() => {}
                    () = fut => {}
                }
            },
            &self.handle,
        );
        true
    }

    /// Tracked blocking work: not cancellable, awaited at shutdown up to the
    /// deadline. Returns `false` (not run) when the scope is cancelled.
    pub fn spawn_blocking<F>(&self, name: &'static str, work: F) -> bool
    where
        F: FnOnce() + Send + 'static,
    {
        if self.token.is_cancelled() || self.tracker.is_closed() {
            return false;
        }
        let entry = self.register(name);
        self.tracker.spawn_blocking_on(
            move || {
                let _entry = entry;
                work();
            },
            &self.handle,
        );
        true
    }

    /// Cancels all tasks of the scope; new spawns are refused.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Names of tasks that have not ended yet (sorted).
    pub fn running_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = lock_tolerating_poison(&self.running)
            .values()
            .copied()
            .collect();
        names.sort_unstable();
        names
    }

    /// Cancel, close, wait up to `timeout`; logs names of laggards at `warn`
    /// (names only, no payloads).
    pub async fn shutdown(&self, timeout: Duration) -> ShutdownReport {
        self.token.cancel();
        self.tracker.close();
        let timed_out = tokio::time::timeout(timeout, self.tracker.wait())
            .await
            .is_err();
        let still_running = if timed_out {
            self.running_names()
        } else {
            Vec::new()
        };
        if timed_out {
            tracing::warn!(
                scope = ?self.key,
                tasks = ?still_running,
                "background tasks still running at shutdown deadline"
            );
        }
        ShutdownReport {
            timed_out,
            still_running,
        }
    }
}

/// One per process. Hands out scopes by key.
pub struct TaskSupervisor {
    root: CancellationToken,
    handle: Handle,
    scopes: Mutex<HashMap<ScopeKey, TaskScope>>,
}

impl TaskSupervisor {
    /// Uses the runtime of the caller; panics outside a Tokio runtime.
    pub fn new() -> Self {
        Self::with_handle(Handle::current())
    }

    /// Spawns onto `handle` (the app passes the Tauri async runtime, because
    /// setup code runs outside a Tokio context).
    pub fn with_handle(handle: Handle) -> Self {
        Self {
            root: CancellationToken::new(),
            handle,
            scopes: Mutex::new(HashMap::new()),
        }
    }

    /// The scope for `key`, created on first use.
    pub fn scope(&self, key: ScopeKey) -> TaskScope {
        lock_tolerating_poison(&self.scopes)
            .entry(key)
            .or_insert_with(|| TaskScope::new(key, &self.root, self.handle.clone()))
            .clone()
    }

    /// The scope that ends at app exit.
    pub fn app_scope(&self) -> TaskScope {
        self.scope(ScopeKey::App)
    }

    /// The scope of session `session_id`, created on first use (a child of
    /// the root: it also ends at app exit).
    pub fn session_scope(&self, session_id: Uuid) -> TaskScope {
        self.scope(ScopeKey::Session(session_id))
    }

    /// Number of tasks of the session scope that have not ended yet (0 when
    /// there is no such scope).
    pub fn running_in(&self, session_id: Uuid) -> usize {
        let scope = lock_tolerating_poison(&self.scopes)
            .get(&ScopeKey::Session(session_id))
            .cloned();
        scope.map_or(0, |scope| scope.running_names().len())
    }

    /// Cancels the session's tasks, waits up to `timeout` for them to end and
    /// removes the scope. While waiting, the scope refuses new tasks.
    pub async fn close_session_scope(&self, session_id: Uuid, timeout: Duration) -> ShutdownReport {
        let scope = lock_tolerating_poison(&self.scopes)
            .get(&ScopeKey::Session(session_id))
            .cloned();
        let Some(scope) = scope else {
            return ShutdownReport {
                timed_out: false,
                still_running: Vec::new(),
            };
        };
        let report = scope.shutdown(timeout).await;
        lock_tolerating_poison(&self.scopes).remove(&ScopeKey::Session(session_id));
        report
    }

    /// Shuts down every scope within one shared deadline.
    pub async fn shutdown(&self, timeout: Duration) -> Vec<ShutdownReport> {
        let deadline = Instant::now() + timeout;
        let scopes: Vec<TaskScope> = lock_tolerating_poison(&self.scopes)
            .values()
            .cloned()
            .collect();
        let mut reports = Vec::new();
        for scope in scopes {
            let left = deadline.saturating_duration_since(Instant::now());
            reports.push(scope.shutdown(left).await);
        }
        self.root.cancel();
        reports
    }
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test(start_paused = true)]
    async fn test_task_stops_after_cancel() {
        let sup = TaskSupervisor::new();
        let scope = sup.app_scope();
        let (tx, rx) = oneshot::channel::<()>();
        // `tx` is dropped when the future is dropped by cancellation.
        assert!(scope.spawn("sleeper", async move {
            let _tx = tx;
            std::future::pending::<()>().await;
        }));
        scope.cancel();
        assert!(rx.await.is_err(), "future must be dropped");
        let report = scope.shutdown(Duration::from_secs(3)).await;
        assert!(!report.timed_out);
    }

    #[tokio::test(start_paused = true)]
    async fn test_shutdown_returns_when_all_tasks_ended() {
        let sup = TaskSupervisor::new();
        let scope = sup.app_scope();
        scope.spawn("a", std::future::pending());
        scope.spawn("b", std::future::pending());
        let reports = sup.shutdown(Duration::from_secs(3)).await;
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].timed_out);
        assert!(scope.running_names().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn test_shutdown_returns_at_deadline_and_names_laggard() {
        let sup = TaskSupervisor::new();
        let scope = sup.app_scope();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (started_tx, started_rx) = oneshot::channel::<()>();
        // Blocking work cannot be cancelled: it ignores cancellation.
        scope.spawn_blocking("stubborn", move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        started_rx.await.expect("blocking task started");
        // Virtual time does not auto-advance while a blocking task is
        // alive, so move the clock explicitly past the deadline.
        let (report, ()) = tokio::join!(scope.shutdown(Duration::from_secs(3)), async {
            tokio::time::advance(Duration::from_secs(4)).await;
        });
        assert!(report.timed_out);
        assert_eq!(report.still_running, vec!["stubborn"]);
        release_tx.send(()).expect("release");
    }

    #[tokio::test(start_paused = true)]
    async fn test_spawn_into_cancelled_scope_does_not_run() {
        let sup = TaskSupervisor::new();
        let scope = sup.app_scope();
        scope.cancel();
        let (tx, rx) = oneshot::channel::<()>();
        let spawned = scope.spawn("late", async move {
            let _ = tx.send(());
        });
        assert!(!spawned);
        // The future was dropped without running: the sender is gone.
        assert!(rx.await.is_err());
        assert!(!scope.spawn_blocking("late-blocking", || {}));
    }

    #[tokio::test(start_paused = true)]
    async fn test_close_session_scope_cancels_and_removes() {
        let sup = TaskSupervisor::new();
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        let scope = sup.session_scope(id);
        scope.spawn("a", std::future::pending());
        sup.session_scope(other).spawn("b", std::future::pending());
        assert_eq!(sup.running_in(id), 1);
        let report = sup.close_session_scope(id, Duration::from_secs(2)).await;
        assert!(!report.timed_out);
        assert_eq!(sup.running_in(id), 0);
        assert_eq!(sup.running_in(other), 1);
        assert!(!scope.spawn("late", async {}), "closed scope refuses tasks");
        // Closing an unknown session is a no-op.
        assert!(
            !sup.close_session_scope(Uuid::new_v4(), Duration::from_secs(2))
                .await
                .timed_out
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_scopes_are_independent_but_share_the_root() {
        let sup = TaskSupervisor::new();
        let app = sup.app_scope();
        let session = sup.scope(ScopeKey::Session(Uuid::new_v4()));
        session.cancel();
        assert!(session.is_cancelled());
        assert!(!app.is_cancelled());
        sup.shutdown(Duration::from_secs(1)).await;
        assert!(app.is_cancelled());
    }
}
