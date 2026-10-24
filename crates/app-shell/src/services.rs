//! Issue #669 (Spec 0038, section 2): background services from `Wiring`.
//!
//! A service is started once per unlock — after the database is open and the
//! database extensions are ready — and stopped when the application locks
//! and when it exits. Nothing here runs while the app is locked in
//! master-password mode. A service that fails to start is logged and
//! skipped; it never stops the others or the app.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tauri::AppHandle;

/// How long `stop()` of one service may take before the core gives up on it.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// What a service gets when it starts: the app handle, through which the
/// managed `AppState` is reachable (`try_state`) and events can be emitted.
#[derive(Clone)]
pub struct ServiceContext {
    pub app: AppHandle,
}

/// A started service. `stop` is awaited by the core with a timeout.
#[async_trait]
pub trait RunningService: Send {
    async fn stop(self: Box<Self>);
}

/// A background service registered in `Wiring::services`. The Community
/// edition registers none.
#[async_trait]
pub trait BackgroundService<C = ServiceContext>: Send + Sync {
    /// A name for the log; never put secrets in it.
    fn name(&self) -> &str;
    /// Starts the service. The error text is logged, so it must not contain
    /// secrets.
    async fn start(&self, ctx: C) -> Result<Box<dyn RunningService>, String>;
}

/// Owns the registered services and the handles of the running ones.
pub struct ServiceHost<C = ServiceContext> {
    services: Vec<Arc<dyn BackgroundService<C>>>,
    running: tokio::sync::Mutex<Vec<(String, Box<dyn RunningService>)>>,
}

impl<C: Clone + Send + 'static> ServiceHost<C> {
    pub fn new(services: Vec<Arc<dyn BackgroundService<C>>>) -> Self {
        Self {
            services,
            running: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Starts every service that is not running yet. Calling it while the
    /// services run is a no-op (at most once per unlock).
    pub async fn start_all(&self, ctx: C) {
        let mut running = self.running.lock().await;
        if !running.is_empty() {
            return;
        }
        for service in &self.services {
            let name = service.name().to_owned();
            match service.start(ctx.clone()).await {
                Ok(handle) => {
                    tracing::info!(service = %name, "background service started");
                    running.push((name, handle));
                }
                Err(err) => {
                    tracing::warn!(
                        service = %name,
                        error = %err,
                        "background service not started; the core continues without it"
                    );
                }
            }
        }
    }

    /// Stops all running services concurrently, each bounded by `timeout`.
    /// A service that does not stop in time is logged and abandoned.
    pub async fn stop_all_with_timeout(&self, timeout: Duration) {
        let handles = std::mem::take(&mut *self.running.lock().await);
        futures::future::join_all(handles.into_iter().map(|(name, handle)| async move {
            match tokio::time::timeout(timeout, handle.stop()).await {
                Ok(()) => tracing::info!(service = %name, "background service stopped"),
                Err(_) => tracing::warn!(
                    service = %name,
                    "background service did not stop in time; continuing"
                ),
            }
        }))
        .await;
    }

    /// For lock and exit.
    pub async fn stop_all(&self) {
        self.stop_all_with_timeout(STOP_TIMEOUT).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    type Log = Arc<Mutex<Vec<String>>>;

    struct Svc {
        name: &'static str,
        log: Log,
        fail_start: bool,
        hang_stop: bool,
    }
    struct Handle {
        name: &'static str,
        log: Log,
        hang: bool,
    }

    #[async_trait]
    impl RunningService for Handle {
        async fn stop(self: Box<Self>) {
            if self.hang {
                std::future::pending::<()>().await;
            }
            self.log.lock().unwrap().push(format!("stop {}", self.name));
        }
    }

    #[async_trait]
    impl BackgroundService<()> for Svc {
        fn name(&self) -> &str {
            self.name
        }
        async fn start(&self, _ctx: ()) -> Result<Box<dyn RunningService>, String> {
            if self.fail_start {
                return Err("boom".into());
            }
            self.log
                .lock()
                .unwrap()
                .push(format!("start {}", self.name));
            Ok(Box::new(Handle {
                name: self.name,
                log: self.log.clone(),
                hang: self.hang_stop,
            }))
        }
    }

    fn svc(name: &'static str, log: &Log, fail_start: bool, hang_stop: bool) -> Arc<Svc> {
        Arc::new(Svc {
            name,
            log: log.clone(),
            fail_start,
            hang_stop,
        })
    }

    #[tokio::test]
    async fn test_start_once_stop_on_lock_restart_and_stop_on_exit() {
        let log: Log = Arc::default();
        let host = ServiceHost::new(vec![svc("a", &log, false, false)]);
        // Nothing starts before `start_all` (= before unlock).
        assert!(log.lock().unwrap().is_empty());
        host.start_all(()).await;
        host.start_all(()).await; // second call: no second start
        host.stop_all().await; // lock
        host.start_all(()).await; // next unlock
        host.stop_all().await; // exit
        assert_eq!(
            *log.lock().unwrap(),
            ["start a", "stop a", "start a", "stop a"]
        );
    }

    #[tokio::test]
    async fn test_a_failing_start_does_not_stop_the_others() {
        let log: Log = Arc::default();
        let host = ServiceHost::new(vec![
            svc("bad", &log, true, false),
            svc("good", &log, false, false),
        ]);
        host.start_all(()).await;
        host.stop_all().await;
        assert_eq!(*log.lock().unwrap(), ["start good", "stop good"]);
    }

    #[tokio::test(start_paused = true)]
    async fn test_a_hanging_stop_delays_at_most_the_timeout() {
        let log: Log = Arc::default();
        let host = ServiceHost::new(vec![
            svc("hang", &log, false, true),
            svc("ok", &log, false, false),
        ]);
        host.start_all(()).await;
        let t0 = tokio::time::Instant::now();
        host.stop_all().await;
        assert!(t0.elapsed() <= STOP_TIMEOUT + Duration::from_millis(50));
        assert!(log.lock().unwrap().contains(&"stop ok".to_owned()));
    }
}
