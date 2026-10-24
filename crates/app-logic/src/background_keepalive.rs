//! Keeping sessions alive in the background (Spec 0114 §4, ADR 0149).
//!
//! Platform-free decision logic: a foreground service runs while at least
//! one session is open, and a wake lock is held only while a command is
//! running or a transfer is active. The platform side (Android) implements
//! [`ForegroundService`]; on other platforms nothing is installed and every
//! call here is a no-op.

use std::sync::{Arc, Mutex, OnceLock};

/// The platform side of the keep-alive.
pub trait ForegroundService: Send + Sync {
    /// Starts the service showing `sessions` active sessions.
    fn start(&self, sessions: usize) -> Result<(), String>;
    /// Updates the shown session count of a running service.
    fn update(&self, sessions: usize) -> Result<(), String>;
    /// Stops the service and removes its notification.
    fn stop(&self) -> Result<(), String>;
    /// Acquires or releases the partial wake lock.
    fn set_wake_lock(&self, held: bool) -> Result<(), String>;
}

#[derive(Default)]
struct State {
    sessions: usize,
    running: bool,
    activities: usize,
}

pub struct KeepAliveController {
    service: Arc<dyn ForegroundService>,
    state: Mutex<State>,
}

fn log(what: &str, result: Result<(), String>) {
    if let Err(error) = result {
        tracing::warn!(%error, "background keep-alive: {what} failed");
    }
}

impl KeepAliveController {
    pub fn new(service: Arc<dyn ForegroundService>) -> Arc<Self> {
        Arc::new(Self {
            service,
            state: Mutex::new(State::default()),
        })
    }

    /// Called whenever the number of open sessions changes.
    pub fn on_session_count(&self, count: usize) {
        let mut st = self.state.lock().unwrap();
        if count == st.sessions && (count > 0) == st.running {
            return;
        }
        st.sessions = count;
        match (count > 0, st.running) {
            (true, false) => {
                log("start", self.service.start(count));
                st.running = true;
            }
            (true, true) => log("update", self.service.update(count)),
            (false, true) => {
                if st.activities > 0 {
                    st.activities = 0;
                    log("release wake lock", self.service.set_wake_lock(false));
                }
                log("stop", self.service.stop());
                st.running = false;
            }
            (false, false) => {}
        }
    }

    /// Marks a running command or transfer; the wake lock is held while at
    /// least one guard lives.
    pub fn begin_activity(self: &Arc<Self>) -> ActivityGuard {
        let mut st = self.state.lock().unwrap();
        st.activities += 1;
        if st.activities == 1 {
            log("acquire wake lock", self.service.set_wake_lock(true));
        }
        ActivityGuard(Some(Arc::clone(self)))
    }

    fn end_activity(&self) {
        let mut st = self.state.lock().unwrap();
        if st.activities == 0 {
            return; // already reset by the last session closing
        }
        st.activities -= 1;
        if st.activities == 0 {
            log("release wake lock", self.service.set_wake_lock(false));
        }
    }
}

/// Holds the wake lock while alive. Inert without an installed controller.
pub struct ActivityGuard(Option<Arc<KeepAliveController>>);

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        if let Some(c) = self.0.take() {
            c.end_activity();
        }
    }
}

static INSTALLED: OnceLock<Arc<KeepAliveController>> = OnceLock::new();

/// Installs the process-wide controller (Android only; once).
pub fn install(controller: Arc<KeepAliveController>) {
    let _ = INSTALLED.set(controller);
}

/// Guard for a running command or an active transfer; a no-op unless a
/// controller is installed.
pub fn background_activity() -> ActivityGuard {
    match INSTALLED.get() {
        Some(c) => c.begin_activity(),
        None => ActivityGuard(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Rec(Mutex<Vec<String>>);
    impl ForegroundService for Rec {
        fn start(&self, n: usize) -> Result<(), String> {
            self.0.lock().unwrap().push(format!("start {n}"));
            Ok(())
        }
        fn update(&self, n: usize) -> Result<(), String> {
            self.0.lock().unwrap().push(format!("update {n}"));
            Ok(())
        }
        fn stop(&self) -> Result<(), String> {
            self.0.lock().unwrap().push("stop".into());
            Ok(())
        }
        fn set_wake_lock(&self, h: bool) -> Result<(), String> {
            self.0.lock().unwrap().push(format!("wake {h}"));
            Ok(())
        }
    }

    fn setup() -> (Arc<Rec>, Arc<KeepAliveController>) {
        let rec = Arc::new(Rec::default());
        (rec.clone(), KeepAliveController::new(rec))
    }

    fn log_of(rec: &Rec) -> Vec<String> {
        rec.0.lock().unwrap().clone()
    }

    #[test]
    fn first_open_starts_last_close_stops() {
        let (rec, c) = setup();
        c.on_session_count(0);
        c.on_session_count(1);
        c.on_session_count(2);
        c.on_session_count(2);
        c.on_session_count(1);
        c.on_session_count(0);
        c.on_session_count(0);
        assert_eq!(log_of(&rec), ["start 1", "update 2", "update 1", "stop"]);
    }

    #[test]
    fn wake_lock_only_while_active_and_released_on_stop() {
        let (rec, c) = setup();
        c.on_session_count(1);
        let a = c.begin_activity();
        let b = c.begin_activity();
        drop(a);
        drop(b);
        let _c = c.begin_activity();
        c.on_session_count(0);
        assert_eq!(
            log_of(&rec),
            [
                "start 1",
                "wake true",
                "wake false",
                "wake true",
                "wake false",
                "stop"
            ]
        );
    }

    #[test]
    fn idle_sessions_never_take_the_wake_lock() {
        let (rec, c) = setup();
        c.on_session_count(3);
        assert_eq!(log_of(&rec), ["start 3"]);
    }

    #[test]
    fn activity_guard_without_controller_is_inert() {
        drop(background_activity());
    }
}
