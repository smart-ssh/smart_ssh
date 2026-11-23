//! Extension point: observe the activity of a session (issue #1505).
//!
//! An edition registers [`ActivityObserver`]s through `Wiring`. Whenever
//! something happens in a session (it starts or ends, a command is proposed,
//! decided on or executed, the AI writes a message, a file is read or
//! written) every observer is told with an [`ActivityEvent`]. The observer is
//! strictly passive: it returns nothing and has no influence on filter,
//! confirmation or execution.
//!
//! The core keeps no activity record of its own in this module. Events are
//! built from the content that was already redacted by the central
//! redaction in the activity funnel (`emit_activity_event` in `chat_turn/activity_funnel.rs`), so an
//! observer only ever sees redacted text. With no observer registered no
//! event is built and nothing is queued.
//!
//! Delivery is off the session path: events go onto a bounded queue served
//! by one worker thread. A full queue drops the event and counts it
//! ([`ActivityHub::dropped_events`]); a panicking observer is caught and
//! logged; a slow observer only delays later events, never the session.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerSource};
use ssh_manager_core::filter::{Pattern, RuleAction, RuleOrigin, Scope};
use ssh_manager_core::profiles::{ProfileStore, Server};
use ssh_manager_core::sync::lock_tolerating_poison;

use crate::tasks::TaskScope;

/// Who caused the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActivitySource {
    User,
    Ai,
    McpAgent,
}

impl From<LedgerSource> for ActivitySource {
    fn from(source: LedgerSource) -> Self {
        match source {
            LedgerSource::User => Self::User,
            LedgerSource::Ai => Self::Ai,
            LedgerSource::McpAgent => Self::McpAgent,
        }
    }
}

/// Which session an event belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    /// The id of the open tab / connection (changes with every connect).
    pub runtime_session_id: String,
    /// The saved chat session, if the session has one.
    pub chat_session_id: Option<String>,
    /// `None` for the local pseudo-server.
    pub server_id: Option<String>,
}

/// What kind of session started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActivitySessionKind {
    Chat,
    Mcp,
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionEndReason {
    /// The session was closed explicitly (by the user, or by the app for
    /// that session alone, e.g. a failed reconnect).
    Closed,
    /// The app is quitting: the session was still open at exit and is not
    /// closed individually.
    AppExit,
}

/// The server as it was when the session started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSnapshot {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Group ids, root group first.
    pub group_ids: Vec<String>,
    /// Group names, root group first.
    pub group_names: Vec<String>,
    pub tags: Vec<String>,
}

/// Outcome of a decision about a proposed action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActivityDecisionOutcome {
    /// The user confirmed (also after editing the command).
    Confirmed,
    /// The user rejected, or the filter blocked it, or the confirmation timed out.
    Rejected,
    /// A rule approved it automatically.
    AutoApproved,
}

impl From<LedgerDecisionOutcome> for ActivityDecisionOutcome {
    fn from(outcome: LedgerDecisionOutcome) -> Self {
        match outcome {
            LedgerDecisionOutcome::Confirmed => Self::Confirmed,
            LedgerDecisionOutcome::Rejected => Self::Rejected,
            LedgerDecisionOutcome::AutoApproved => Self::AutoApproved,
        }
    }
}

/// The rule that decided, as it is stored when the event is built.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSnapshot {
    pub id: String,
    pub pattern: Pattern,
    pub action: RuleAction,
    pub scope: Scope,
    pub origin: RuleOrigin,
}

/// What happened.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ActivityEventKind {
    SessionStarted {
        kind: ActivitySessionKind,
        /// Name of the MCP client (`mcp` sessions only).
        mcp_client: Option<String>,
        server: ServerSnapshot,
        /// IANA name of the device time zone, if known.
        time_zone: Option<String>,
    },
    SessionEnded {
        reason: SessionEndReason,
    },
    CommandProposed {
        action_id: String,
        command: String,
    },
    Decision {
        action_id: String,
        outcome: ActivityDecisionOutcome,
        code: Option<String>,
        reason: Option<String>,
        /// `None` if no rule decided or the rule no longer exists.
        rule: Option<RuleSnapshot>,
    },
    CommandExecuted {
        action_id: String,
        command: String,
        /// Redacted output as UTF-8 text (lossy).
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
        truncated: bool,
        cancelled: bool,
    },
    AiMessage {
        text: String,
    },
    /// Never carries file content.
    FileRead {
        action_id: String,
        path: String,
        size: Option<u64>,
        sha256: Option<String>,
    },
    /// Never carries file content or a diff.
    FileWritten {
        action_id: String,
        path: String,
        backup_path: Option<String>,
        size_before: Option<u64>,
        size_after: Option<u64>,
        sha256_before: Option<String>,
        sha256_after: Option<String>,
        sudo: bool,
    },
}

/// One thing that happened in a session. Text fields are redacted.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEvent {
    pub at: DateTime<Utc>,
    pub session: SessionRef,
    pub source: ActivitySource,
    /// `true` if the session's redactor could not be built completely (a
    /// session extension's secrets were unavailable), so a secret may remain.
    pub redaction_incomplete: bool,
    pub kind: ActivityEventKind,
}

/// Registered through `Wiring`; called off the session flow. A panic is
/// caught and logged; the session is not affected.
pub trait ActivityObserver: Send + Sync {
    fn on_event(&self, event: &ActivityEvent);
}

/// Events waiting for the worker. Beyond this, new events are dropped.
const QUEUE_CAPACITY: usize = 1024;
/// How often an idle worker looks at its scope's cancellation.
const WORKER_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// The registered observers and the queue that delivers events to them.
/// One per app (`AppState.activity`); inactive without observers.
pub struct ActivityHub {
    observers: Arc<Vec<Arc<dyn ActivityObserver>>>,
    tx: SyncSender<ActivityEvent>,
    rx: Mutex<Option<Receiver<ActivityEvent>>>,
    dropped: AtomicU64,
    /// Tests only: deliver in `push` instead of through the worker.
    inline: bool,
}

impl Default for ActivityHub {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl ActivityHub {
    pub fn new(observers: Vec<Arc<dyn ActivityObserver>>) -> Self {
        Self::with_capacity(observers, QUEUE_CAPACITY)
    }

    pub(crate) fn with_capacity(
        observers: Vec<Arc<dyn ActivityObserver>>,
        capacity: usize,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity);
        Self {
            observers: Arc::new(observers),
            tx,
            rx: Mutex::new(Some(rx)),
            dropped: AtomicU64::new(0),
            inline: false,
        }
    }

    /// Delivers events synchronously inside `push`, so a test can read what
    /// an observer received without waiting for a worker.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_inline_for_tests(observers: Vec<Arc<dyn ActivityObserver>>) -> Self {
        let mut hub = Self::new(observers);
        hub.inline = true;
        hub
    }

    /// `true` if at least one observer is registered.
    pub fn is_active(&self) -> bool {
        !self.observers.is_empty()
    }

    /// Events dropped because the queue was full (or the worker had ended).
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Starts the delivery worker as a tracked blocking task of `scope` (the
    /// app scope). Called once at startup; later calls do nothing.
    pub fn start_worker(&self, scope: &TaskScope) {
        if !self.is_active() {
            return;
        }
        let Some(rx) = lock_tolerating_poison(&self.rx).take() else {
            return;
        };
        let observers = Arc::clone(&self.observers);
        let worker_scope = scope.clone();
        if !scope.spawn_blocking("activity-worker", move || {
            run_worker(rx, &observers, Some(&worker_scope))
        }) {
            tracing::warn!("activity worker not started: scope already closed");
        }
    }

    /// Starts a detached worker thread that ends when the hub is dropped
    /// (tests, and embedding code without an app scope).
    #[cfg(any(test, feature = "test-support"))]
    pub fn start_detached_worker(&self) {
        let Some(rx) = lock_tolerating_poison(&self.rx).take() else {
            return;
        };
        let observers = Arc::clone(&self.observers);
        std::thread::spawn(move || run_worker(rx, &observers, None));
    }

    /// Never blocks. A full queue drops the event (counted, logged without
    /// content).
    pub(crate) fn push(&self, event: ActivityEvent) {
        if self.inline {
            deliver(&self.observers, &event);
            return;
        }
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("activity queue full; an event was dropped");
            }
            Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("activity worker has ended; an event was dropped");
            }
        }
    }
}

/// Delivers events until the sender is gone or `scope` is cancelled (then the
/// queue is drained once). A
/// panicking observer is logged (no content) and does not stop the others.
fn run_worker(
    rx: Receiver<ActivityEvent>,
    observers: &[Arc<dyn ActivityObserver>],
    scope: Option<&TaskScope>,
) {
    loop {
        if scope.is_some_and(TaskScope::is_cancelled) {
            // App exit: deliver what is already queued (the exit sequence
            // queues the `SessionEnded` events before it cancels). The
            // supervisor's shutdown timeout bounds a blocking observer.
            while let Ok(event) = rx.try_recv() {
                deliver(observers, &event);
            }
            return;
        }
        match rx.recv_timeout(WORKER_POLL) {
            Ok(event) => deliver(observers, &event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn deliver(observers: &[Arc<dyn ActivityObserver>], event: &ActivityEvent) {
    for observer in observers {
        if catch_unwind(AssertUnwindSafe(|| observer.on_event(event))).is_err() {
            tracing::warn!("an activity observer panicked");
        }
    }
}

/// Per-session setup (set once when the session is built, only if an
/// observer is registered).
#[derive(Clone)]
pub struct ActivityReporting {
    pub(crate) hub: Arc<ActivityHub>,
    pub(crate) runtime_session_id: String,
    pub(crate) server: ServerSnapshot,
    pub(crate) time_zone: Option<String>,
    pub(crate) redaction_incomplete: bool,
}

impl ActivityReporting {
    /// `None` without an observer: nothing is built for such a session.
    pub(crate) fn new(
        hub: &Arc<ActivityHub>,
        runtime_session_id: String,
        server: ServerSnapshot,
        redaction_incomplete: bool,
    ) -> Option<Self> {
        hub.is_active().then(|| Self {
            hub: Arc::clone(hub),
            runtime_session_id,
            server,
            time_zone: persistence_sqlite::device_time_zone_source()(),
            redaction_incomplete,
        })
    }
}

#[cfg(test)]
thread_local! {
    /// Events built on this thread (zero-cost check: stays 0 without observer).
    pub(crate) static CONSTRUCTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Longest group path that is followed (guards against a cycle).
const MAX_GROUP_DEPTH: usize = 64;

/// The snapshot of `server` with its group path (root first).
pub(crate) async fn server_snapshot(
    profile_store: &dyn ProfileStore,
    server: &Server,
) -> ServerSnapshot {
    let mut chain = Vec::new();
    let mut next = server.group_id;
    while let Some(group_id) = next {
        if chain.len() >= MAX_GROUP_DEPTH {
            break;
        }
        match profile_store.get_group(&group_id).await {
            Ok(group) => {
                next = group.parent_id.filter(|p| *p != group_id);
                chain.push(group);
            }
            Err(err) => {
                tracing::warn!(error = %err, "group lookup for the activity snapshot failed");
                break;
            }
        }
    }
    chain.reverse();
    ServerSnapshot {
        id: server.id.0.to_string(),
        name: server.name.clone(),
        host: server.host.clone(),
        port: server.port,
        user: server.username.clone(),
        group_ids: chain.iter().map(|g| g.id.0.to_string()).collect(),
        group_names: chain.iter().map(|g| g.name.clone()).collect(),
        tags: server.tags.clone(),
    }
}
