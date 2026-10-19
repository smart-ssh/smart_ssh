//! Issue #546: running connect attempts that the frontend can cancel.
//!
//! The frontend generates an attempt id and passes it to `connect`; the
//! command registers it here for the duration of the attempt.
//! `cancel_connect(attemptId)` flips the flag, the running attempt notices
//! and aborts. Cancelling an unknown or finished attempt is a no-op.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

#[derive(Default)]
pub struct ConnectAttempts {
    inner: Arc<Mutex<HashMap<String, (u64, watch::Sender<bool>)>>>,
}

/// Held by the running connect; unregisters the attempt on drop.
pub struct ConnectAttemptGuard {
    id: String,
    token: u64,
    rx: watch::Receiver<bool>,
    registry: Arc<Mutex<HashMap<String, (u64, watch::Sender<bool>)>>>,
}

impl ConnectAttempts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `attempt_id`. A second registration under the same id
    /// replaces the first (the old one can then no longer be cancelled).
    pub fn register(&self, attempt_id: &str) -> ConnectAttemptGuard {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = watch::channel(false);
        self.lock().insert(attempt_id.to_string(), (token, tx));
        ConnectAttemptGuard {
            id: attempt_id.to_string(),
            token,
            rx,
            registry: self.inner.clone(),
        }
    }

    /// Cancels the attempt. Returns whether a running attempt was found.
    pub fn cancel(&self, attempt_id: &str) -> bool {
        match self.lock().get(attempt_id) {
            Some((_, tx)) => {
                tx.send_replace(true);
                true
            }
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (u64, watch::Sender<bool>)>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl ConnectAttemptGuard {
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Resolves once the attempt is cancelled (immediately if it already
    /// is). Level-triggered, so a cancel that arrived earlier is not missed.
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        // The sender lives in the registry until this guard drops; an error
        // would mean it was replaced — then never resolve.
        if rx.wait_for(|c| *c).await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

impl Drop for ConnectAttemptGuard {
    fn drop(&mut self) {
        let mut map = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        // Only remove our own entry (not one a newer registration replaced).
        if map.get(&self.id).is_some_and(|(t, _)| *t == self.token) {
            map.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_resolves_waiting_guard_and_unknown_is_noop() {
        let reg = ConnectAttempts::new();
        assert!(!reg.cancel("nope"));
        let guard = reg.register("a1");
        assert!(!guard.is_cancelled());
        assert!(reg.cancel("a1"));
        tokio::time::timeout(std::time::Duration::from_secs(1), guard.cancelled())
            .await
            .expect("cancel must resolve");
        drop(guard);
        assert!(!reg.cancel("a1"), "finished attempt is a no-op");
    }
}
