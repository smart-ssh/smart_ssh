//! Issue #356, Spec 0107 §8: automatic reconnect after a connection loss.
//!
//! This module holds the parts that do not need a Tauri runtime: the
//! setting, the retry schedule, the loop itself (driven by an `attempt`
//! closure, so it can be tested with a paused clock) and the per-session
//! state (stop handle, "an AI turn was interrupted" flag, loss hook).
//! The concrete attempt (host key policy, credentials, transport) lives in
//! `app_shell`.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::events::EventEmitter;
use crate::state::SessionId;

/// Key in `settings.json` (written by the frontend, read here at every drop,
/// so a change applies from the next drop on). The value is the number of
/// seconds; `0` means off.
pub const SETTING_KEY: &str = "autoReconnectSeconds";

/// Default when the key is missing or holds anything that is not one of the
/// offered values.
pub const DEFAULT_SECONDS: u64 = 60;

/// Waits before attempt 1, 2, 3, 4; every later wait is the last value.
const WAITS_SECS: [u64; 4] = [2, 4, 8, 15];

/// The allowed values of the setting: Off, 30 s, 60 s, 5 min.
pub fn budget_from_stored(value: Option<&serde_json::Value>) -> Option<Duration> {
    let seconds = match value.and_then(serde_json::Value::as_u64) {
        Some(0) => return None,
        Some(s @ (30 | 60 | 300)) => s,
        _ => DEFAULT_SECONDS,
    };
    Some(Duration::from_secs(seconds))
}

/// Wait before attempt `attempt` (1-based).
pub fn wait_before_attempt(attempt: u32) -> Duration {
    let index = (attempt.max(1) as usize - 1).min(WAITS_SECS.len() - 1);
    Duration::from_secs(WAITS_SECS[index])
}

/// Error codes after which another try may succeed: the server or the
/// network is not (yet) reachable. Everything else (host key, credentials,
/// keychain, authentication, configuration) needs the user and stops the
/// automatic attempts, so this list is an allow-list: an unknown code stops.
pub fn is_retryable_code(code: Option<&str>) -> bool {
    matches!(
        code,
        Some(
            "SSH_CONNECTION_FAILED"
                | "SSH_TIMEOUT"
                | "SSH_CONNECTION_REFUSED"
                | "SSH_HOST_NOT_FOUND"
                | "SSH_HOST_UNREACHABLE"
                | "SSH_CONNECTION_CLOSED"
                | "SSH_CHANNEL_ERROR"
                | "SSH_SESSION_CLOSED"
        )
    )
}

/// Result of one automatic attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptResult {
    Connected,
    /// Failed in a way waiting may fix.
    Retry,
    /// Failed in a way that needs the user; stop.
    Blocked,
}

/// How the loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoReconnectEnd {
    Reconnected,
    /// The configured time ran out.
    GaveUp,
    /// "Stop", closing the tab or a manual reconnect.
    Stopped,
    /// An attempt needs the user.
    Blocked,
}

/// Lets the user (or a closing tab) end the attempts at once.
#[derive(Default)]
pub struct StopHandle {
    stopped: AtomicBool,
    notify: Notify,
}

impl StopHandle {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            if self.is_stopped() {
                return;
            }
            notified.await;
        }
    }
}

/// Runs the attempts: waits 2, 4, 8, 15, 15 … s, one attempt at a time,
/// until `budget` (measured from the call) has passed. `on_attempt(n)` is
/// called when attempt `n` starts.
pub async fn run_auto_reconnect<F, Fut>(
    budget: Duration,
    stop: &StopHandle,
    mut on_attempt: impl FnMut(u32),
    mut attempt: F,
) -> AutoReconnectEnd
where
    F: FnMut() -> Fut,
    Fut: Future<Output = AttemptResult>,
{
    let started = Instant::now();
    let deadline = started + budget;
    let mut n: u32 = 0;
    let mut next_at = started;
    loop {
        n += 1;
        next_at += wait_before_attempt(n);
        if next_at > deadline {
            // The next attempt would start after the time ran out: wait for
            // the end of the time, then give up.
            tokio::select! {
                () = stop.wait() => return AutoReconnectEnd::Stopped,
                () = tokio::time::sleep_until(deadline) => return AutoReconnectEnd::GaveUp,
            }
        }
        tokio::select! {
            () = stop.wait() => return AutoReconnectEnd::Stopped,
            () = tokio::time::sleep_until(next_at) => {}
        }
        if stop.is_stopped() {
            return AutoReconnectEnd::Stopped;
        }
        on_attempt(n);
        let result = tokio::select! {
            () = stop.wait() => return AutoReconnectEnd::Stopped,
            result = attempt() => result,
        };
        match result {
            AttemptResult::Connected => return AutoReconnectEnd::Reconnected,
            AttemptResult::Blocked => return AutoReconnectEnd::Blocked,
            AttemptResult::Retry => {}
        }
        if Instant::now() >= deadline {
            return AutoReconnectEnd::GaveUp;
        }
    }
}

/// Callback the session calls once when its connection is lost.
pub type ConnectionLostHook = Arc<dyn Fn(SessionId) + Send + Sync>;

/// Per-session state of the automatic reconnect.
#[derive(Default)]
pub struct AutoReconnectState {
    /// Set by `app_shell` once per session.
    hook: OnceLock<ConnectionLostHook>,
    /// An AI turn was active when the connection was lost.
    ai_turn_interrupted: AtomicBool,
    stop: Mutex<Option<Arc<StopHandle>>>,
}

impl AutoReconnectState {
    pub fn set_hook(&self, hook: ConnectionLostHook) {
        let _ = self.hook.set(hook);
    }

    pub(crate) fn fire_hook(&self, session_id: SessionId) {
        if let Some(hook) = self.hook.get() {
            hook(session_id);
        }
    }

    /// Records, at every loss, whether an AI turn was active (overwrites
    /// the value of an earlier drop).
    pub fn set_ai_turn_interrupted(&self, interrupted: bool) {
        self.ai_turn_interrupted
            .store(interrupted, Ordering::SeqCst);
    }

    /// Returns and clears the flag: the continuation happens at most once
    /// per drop.
    pub fn take_ai_turn_interrupted(&self) -> bool {
        self.ai_turn_interrupted.swap(false, Ordering::SeqCst)
    }

    /// Starts a new run; an older one still registered is stopped.
    pub fn begin_run(&self) -> Arc<StopHandle> {
        let handle = Arc::new(StopHandle::default());
        let mut slot = crate::poison::lock_tolerating_poison(&self.stop);
        if let Some(old) = slot.replace(handle.clone()) {
            old.stop();
        }
        handle
    }

    /// Ends the running attempts, if any.
    pub fn stop(&self) {
        if let Some(handle) = crate::poison::lock_tolerating_poison(&self.stop).take() {
            handle.stop();
        }
    }

    pub fn is_running(&self) -> bool {
        crate::poison::lock_tolerating_poison(&self.stop)
            .as_ref()
            .is_some_and(|h| !h.is_stopped())
    }

    /// Forgets the handle of a finished run (only if it is still the same).
    pub fn finish_run(&self, handle: &Arc<StopHandle>) {
        let mut slot = crate::poison::lock_tolerating_poison(&self.stop);
        if slot.as_ref().is_some_and(|h| Arc::ptr_eq(h, handle)) {
            *slot = None;
        }
    }
}

/// What the header shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoReconnectPhase {
    /// Attempts run (the payload carries the number of the next/current one).
    Running,
    Reconnected,
    GaveUp,
    Stopped,
    Blocked,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoReconnectChangedPayload {
    session_id: SessionId,
    phase: AutoReconnectPhase,
    attempt: u32,
}

pub fn emit_auto_reconnect_changed(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    phase: AutoReconnectPhase,
    attempt: u32,
) {
    if let Ok(value) = serde_json::to_value(AutoReconnectChangedPayload {
        session_id,
        phase,
        attempt,
    }) {
        emitter.emit_event("auto-reconnect-changed", value);
    }
}

impl From<AutoReconnectEnd> for AutoReconnectPhase {
    fn from(end: AutoReconnectEnd) -> Self {
        match end {
            AutoReconnectEnd::Reconnected => Self::Reconnected,
            AutoReconnectEnd::GaveUp => Self::GaveUp,
            AutoReconnectEnd::Stopped => Self::Stopped,
            AutoReconnectEnd::Blocked => Self::Blocked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn test_setting_values() {
        assert_eq!(budget_from_stored(None), Some(Duration::from_secs(60)));
        assert_eq!(budget_from_stored(Some(&json!(0))), None);
        assert_eq!(
            budget_from_stored(Some(&json!(30))),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            budget_from_stored(Some(&json!(300))),
            Some(Duration::from_secs(300))
        );
        // Anything else falls back to the default, never to "unlimited".
        assert_eq!(
            budget_from_stored(Some(&json!(99999))),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            budget_from_stored(Some(&json!("off"))),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn test_waits_are_2_4_8_then_15() {
        let waits: Vec<u64> = (1..=7).map(|n| wait_before_attempt(n).as_secs()).collect();
        assert_eq!(waits, vec![2, 4, 8, 15, 15, 15, 15]);
    }

    #[test]
    fn test_only_network_codes_are_retryable() {
        assert!(is_retryable_code(Some("SSH_HOST_UNREACHABLE")));
        assert!(is_retryable_code(Some("SSH_CONNECTION_REFUSED")));
        for code in [
            "SSH_AUTH_FAILED",
            "SSH_HOST_KEY_NOT_TRUSTED",
            "SSH_HOST_KEY_REJECTED",
            "SECRET_STORE_FAILED",
            "SSH_CREDENTIAL_RESOLUTION_FAILED",
            "SOMETHING_NEW",
        ] {
            assert!(!is_retryable_code(Some(code)), "{code}");
        }
        assert!(!is_retryable_code(None));
    }

    /// Records the offsets (in seconds from the start) of every attempt.
    async fn run_with_results(
        budget_secs: u64,
        results: Vec<AttemptResult>,
    ) -> (AutoReconnectEnd, Vec<u64>) {
        let start = Instant::now();
        let stop = StopHandle::default();
        let times = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicU32::new(0));
        let (t, c) = (times.clone(), calls.clone());
        let end = run_auto_reconnect(
            Duration::from_secs(budget_secs),
            &stop,
            |_| {},
            move || {
                let i = c.fetch_add(1, Ordering::SeqCst) as usize;
                t.lock().unwrap().push(start.elapsed().as_secs());
                let r = results.get(i).cloned().unwrap_or(AttemptResult::Retry);
                async move { r }
            },
        )
        .await;
        let times = times.lock().unwrap().clone();
        (end, times)
    }

    #[tokio::test(start_paused = true)]
    async fn test_default_budget_attempts_at_2_6_14_29_44_59_then_gives_up() {
        let (end, times) = run_with_results(60, vec![]).await;
        assert_eq!(end, AutoReconnectEnd::GaveUp);
        assert_eq!(times, vec![2, 6, 14, 29, 44, 59]);
    }

    #[tokio::test(start_paused = true)]
    async fn test_server_back_in_time_reconnects_and_stops_trying() {
        let (end, times) = run_with_results(
            60,
            vec![
                AttemptResult::Retry,
                AttemptResult::Retry,
                AttemptResult::Connected,
            ],
        )
        .await;
        assert_eq!(end, AutoReconnectEnd::Reconnected);
        assert_eq!(times, vec![2, 6, 14]);
    }

    #[tokio::test(start_paused = true)]
    async fn test_blocked_attempt_stops_at_once() {
        let (end, times) = run_with_results(60, vec![AttemptResult::Blocked]).await;
        assert_eq!(end, AutoReconnectEnd::Blocked);
        assert_eq!(times, vec![2]);
    }

    #[tokio::test(start_paused = true)]
    async fn test_gives_up_when_the_time_has_run_out_exactly_at_the_budget() {
        let start = Instant::now();
        let stop = StopHandle::default();
        let end = run_auto_reconnect(
            Duration::from_secs(30),
            &stop,
            |_| {},
            || async { AttemptResult::Retry },
        )
        .await;
        assert_eq!(end, AutoReconnectEnd::GaveUp);
        assert_eq!(start.elapsed(), Duration::from_secs(30));
    }

    #[tokio::test(start_paused = true)]
    async fn test_stop_ends_the_wait_without_another_attempt() {
        let stop = Arc::new(StopHandle::default());
        let attempts = Arc::new(AtomicU32::new(0));
        let (s, a) = (stop.clone(), attempts.clone());
        let task = tokio::spawn(async move {
            run_auto_reconnect(
                Duration::from_secs(300),
                &s,
                |_| {},
                move || {
                    a.fetch_add(1, Ordering::SeqCst);
                    async { AttemptResult::Retry }
                },
            )
            .await
        });
        tokio::time::sleep(Duration::from_secs(5)).await; // attempt 1 at 2 s
        stop.stop();
        assert_eq!(task.await.unwrap(), AutoReconnectEnd::Stopped);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn test_stop_during_a_running_attempt_ends_it() {
        let stop = Arc::new(StopHandle::default());
        let s = stop.clone();
        let task = tokio::spawn(async move {
            run_auto_reconnect(
                Duration::from_secs(300),
                &s,
                |_| {},
                || async {
                    tokio::time::sleep(Duration::from_secs(1000)).await;
                    AttemptResult::Connected
                },
            )
            .await
        });
        tokio::time::sleep(Duration::from_secs(3)).await;
        stop.stop();
        assert_eq!(task.await.unwrap(), AutoReconnectEnd::Stopped);
    }

    #[test]
    fn test_interrupted_flag_is_taken_once() {
        let state = AutoReconnectState::default();
        assert!(!state.take_ai_turn_interrupted());
        state.set_ai_turn_interrupted(true);
        assert!(state.take_ai_turn_interrupted());
        assert!(!state.take_ai_turn_interrupted());
    }

    #[test]
    fn test_a_new_run_stops_the_old_one() {
        let state = AutoReconnectState::default();
        let first = state.begin_run();
        assert!(state.is_running());
        let _second = state.begin_run();
        assert!(first.is_stopped());
        state.stop();
        assert!(!state.is_running());
    }
}
