//! Extension point: observe the AI usage of every completed AI request of a
//! session (Issue #610, Spec 0038 section 2).
//!
//! An edition registers [`AiUsageObserver`]s through `Wiring`. Whenever an AI
//! request that belongs to a session reports its [`AiUsage`], every observer
//! is told once. The observer is strictly passive: it returns nothing and has
//! no influence on filter, confirmation or execution. The record carries ids,
//! the model and the numbers only, never prompt, response or command content.
//! The core itself stores nothing.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use ssh_manager_core::ai::{AiUsage, ProviderType};

use crate::tasks::TaskScope;

/// Why the request was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiUsagePurpose {
    ChatTurn,
    RiskSecondOpinion,
    InjectionCheck,
    SessionTitle,
    NoteSuggestion,
    Other,
}

/// One completed AI request of a session. No content fields.
#[derive(Debug, Clone, PartialEq)]
pub struct AiUsageRecord {
    /// `None` for the local pseudo-server and for sessions without a saved chat.
    pub chat_session_id: Option<String>,
    pub runtime_session_id: String,
    /// `None` for the local pseudo-server.
    pub server_id: Option<String>,
    pub provider_config_id: String,
    pub provider_type: ProviderType,
    pub configured_model: String,
    pub purpose: AiUsagePurpose,
    pub usage: AiUsage,
    pub completed_at: DateTime<Utc>,
}

/// Registered through `Wiring`; called after a request completed, off the
/// chat flow. A panic is caught and logged; the chat turn is not affected.
pub trait AiUsageObserver: Send + Sync {
    fn on_usage(&self, record: &AiUsageRecord);
}

/// Which configured provider served a request.
#[derive(Debug, Clone)]
pub struct AiUsageSource {
    pub provider_config_id: String,
    pub provider_type: ProviderType,
    pub configured_model: String,
}

/// Per-session reporting setup (set once when the session is built).
#[derive(Clone)]
pub struct AiUsageReporting {
    pub observers: Arc<Vec<Arc<dyn AiUsageObserver>>>,
    pub runtime_session_id: String,
    pub main: AiUsageSource,
    /// Provider of the second-opinion and injection-check side calls.
    pub side_call: Option<AiUsageSource>,
}

impl AiUsageReporting {
    pub(crate) fn source_for(&self, purpose: AiUsagePurpose) -> Option<&AiUsageSource> {
        match purpose {
            AiUsagePurpose::RiskSecondOpinion | AiUsagePurpose::InjectionCheck => {
                self.side_call.as_ref()
            }
            _ => Some(&self.main),
        }
    }
}

/// Records waiting for the worker. Beyond this, new records are dropped.
const QUEUE_CAPACITY: usize = 1024;
/// How often an idle worker looks at its scope's cancellation.
const WORKER_POLL: std::time::Duration = std::time::Duration::from_millis(250);

type UsageJob = (Arc<Vec<Arc<dyn AiUsageObserver>>>, AiUsageRecord);

/// Issue #881: the one queue (and one worker) that delivers usage records to
/// observers, instead of a blocking task or a thread per record.
pub(crate) struct UsageQueue {
    tx: std::sync::mpsc::SyncSender<UsageJob>,
}

impl UsageQueue {
    fn new(capacity: usize) -> (Self, std::sync::mpsc::Receiver<UsageJob>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity);
        (Self { tx }, rx)
    }

    /// Never blocks. A full queue drops the record (logged, no content).
    fn push(&self, job: UsageJob) {
        use std::sync::mpsc::TrySendError;
        match self.tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!("AI usage queue full; a usage record was dropped");
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::warn!("AI usage worker has ended; a usage record was dropped");
            }
        }
    }
}

/// Delivers jobs until the senders are gone or `scope` is cancelled. A
/// panicking observer is logged (no content) and does not stop the others.
fn run_worker(rx: std::sync::mpsc::Receiver<UsageJob>, scope: Option<&TaskScope>) {
    use std::sync::mpsc::RecvTimeoutError;
    loop {
        if scope.is_some_and(TaskScope::is_cancelled) {
            return;
        }
        match rx.recv_timeout(WORKER_POLL) {
            Ok((observers, record)) => {
                for observer in observers.iter() {
                    if catch_unwind(AssertUnwindSafe(|| observer.on_usage(&record))).is_err() {
                        tracing::warn!("an AI usage observer panicked");
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

static QUEUE: OnceLock<UsageQueue> = OnceLock::new();

/// Starts the one usage worker as a tracked blocking task of `scope` (the
/// app scope). Called once at startup; a second call does nothing.
pub fn start_worker(scope: &TaskScope) {
    QUEUE.get_or_init(|| {
        let (queue, rx) = UsageQueue::new(QUEUE_CAPACITY);
        let worker_scope = scope.clone();
        if !scope.spawn_blocking("ai-usage-worker", move || {
            run_worker(rx, Some(&worker_scope))
        }) {
            tracing::warn!("AI usage worker not started: scope already closed");
        }
        queue
    });
}

/// In unit tests nothing binds a scope; one detached worker serves the
/// whole test binary.
#[cfg(test)]
fn queue() -> Option<&'static UsageQueue> {
    Some(QUEUE.get_or_init(|| {
        let (queue, rx) = UsageQueue::new(QUEUE_CAPACITY);
        std::thread::spawn(move || run_worker(rx, None));
        queue
    }))
}

#[cfg(not(test))]
fn queue() -> Option<&'static UsageQueue> {
    QUEUE.get()
}

/// Hands the record to the usage worker, so a slow observer neither delays
/// the caller nor blocks the async runtime. Never blocks; a full queue drops
/// the record.
pub(crate) fn notify(observers: &Arc<Vec<Arc<dyn AiUsageObserver>>>, record: AiUsageRecord) {
    if observers.is_empty() {
        return;
    }
    match queue() {
        Some(queue) => queue.push((Arc::clone(observers), record)),
        None => tracing::warn!("AI usage worker not started; a usage record was dropped"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ThreadRecorder {
        threads: std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
        done: std::sync::mpsc::Sender<()>,
    }

    impl AiUsageObserver for ThreadRecorder {
        fn on_usage(&self, _record: &AiUsageRecord) {
            self.threads
                .lock()
                .unwrap()
                .insert(std::thread::current().id());
            let _ = self.done.send(());
        }
    }

    fn record() -> AiUsageRecord {
        AiUsageRecord {
            chat_session_id: None,
            runtime_session_id: String::new(),
            server_id: None,
            provider_config_id: String::new(),
            provider_type: ProviderType::OpenAi,
            configured_model: String::new(),
            purpose: AiUsagePurpose::Other,
            usage: AiUsage::default(),
            completed_at: Utc::now(),
        }
    }

    /// Issue #881: no runtime, 1,000 records, at most one worker thread.
    #[test]
    fn test_thousand_records_without_runtime_use_one_worker_thread() {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let recorder = Arc::new(ThreadRecorder {
            threads: Default::default(),
            done: done_tx,
        });
        let observers: Arc<Vec<Arc<dyn AiUsageObserver>>> = Arc::new(vec![recorder.clone()]);
        let (queue, rx) = UsageQueue::new(QUEUE_CAPACITY);
        let worker = std::thread::spawn(move || run_worker(rx, None));
        for _ in 0..1000 {
            queue.push((Arc::clone(&observers), record()));
        }
        for _ in 0..1000 {
            done_rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("record delivered");
        }
        assert_eq!(recorder.threads.lock().unwrap().len(), 1);
        drop(queue);
        worker.join().expect("worker ends when the queue is gone");
    }

    /// A full queue drops records instead of blocking the caller.
    #[test]
    fn test_full_queue_drops_instead_of_blocking() {
        let observers: Arc<Vec<Arc<dyn AiUsageObserver>>> = Arc::new(Vec::new());
        let (queue, rx) = UsageQueue::new(2);
        for _ in 0..10 {
            queue.push((Arc::clone(&observers), record()));
        }
        assert_eq!(rx.try_iter().count(), 2);
    }

    /// Compile-time guard: adding a field breaks this destructuring, so a
    /// content field cannot slip in unnoticed. Strings are ids/model only.
    #[test]
    fn test_record_has_no_content_fields() {
        let AiUsageRecord {
            chat_session_id: _chat_session_id,
            runtime_session_id: _runtime_session_id,
            server_id: _server_id,
            provider_config_id: _provider_config_id,
            provider_type: _provider_type,
            configured_model: _configured_model,
            purpose: _purpose,
            usage: _usage,
            completed_at: _completed_at,
        } = AiUsageRecord {
            chat_session_id: None,
            runtime_session_id: String::new(),
            server_id: None,
            provider_config_id: String::new(),
            provider_type: ProviderType::OpenAi,
            configured_model: String::new(),
            purpose: AiUsagePurpose::Other,
            usage: AiUsage::default(),
            completed_at: Utc::now(),
        };
    }
}
