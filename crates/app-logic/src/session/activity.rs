//! Activity record of a session: connection events and the content-free
//! activity tick (issue #1447, Spec 0057).
//!
//! Nothing here sees what the user typed. [`ActivityTicker::claim`] only
//! learns "input happened"; the tick entry has no content.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use ssh_manager_core::audit::{LedgerConnectionEvent, LedgerEntryContent, LedgerSource};
use ssh_manager_core::sync::lock_tolerating_poison;

use super::Session;

/// Shared on/off switch of "Record active minutes" (default on). One flag is
/// shared by every session so a change applies to open sessions at once.
pub type ActiveMinutesFlag = Arc<AtomicBool>;

/// A new flag, on.
pub fn new_active_minutes_flag() -> ActiveMinutesFlag {
    Arc::new(AtomicBool::new(true))
}

/// Seconds since the Unix epoch; replaceable so tests run on virtual time.
pub type EpochClock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Decides whether an input or a running command starts a new tick.
pub struct ActivityTicker {
    enabled: ActiveMinutesFlag,
    last_minute: StdMutex<Option<i64>>,
    clock: EpochClock,
}

impl Default for ActivityTicker {
    fn default() -> Self {
        Self::new(new_active_minutes_flag())
    }
}

impl ActivityTicker {
    pub fn new(enabled: ActiveMinutesFlag) -> Self {
        Self::with_clock(enabled, Arc::new(|| chrono::Utc::now().timestamp()))
    }

    pub fn with_clock(enabled: ActiveMinutesFlag, clock: EpochClock) -> Self {
        Self {
            enabled,
            last_minute: StdMutex::new(None),
            clock,
        }
    }

    /// `true` at most once per calendar minute, and never while the setting
    /// is off. Cheap and synchronous: safe on the input path.
    pub fn claim(&self) -> bool {
        if !self.enabled.load(Ordering::Relaxed) {
            return false;
        }
        let minute = (self.clock)().div_euclid(60);
        let mut last = lock_tolerating_poison(&self.last_minute);
        if *last == Some(minute) {
            return false;
        }
        *last = Some(minute);
        true
    }
}

/// Source of entries the app writes on its own behalf: the user for a human
/// session, the MCP agent for an MCP session.
async fn event_source(session: &Session) -> LedgerSource {
    if session
        .persistence
        .mcp_activity_session_id
        .lock()
        .await
        .is_some()
    {
        LedgerSource::McpAgent
    } else {
        LedgerSource::User
    }
}

/// The recorded session this session writes to, if any.
async fn recorded_session_id(session: &Session) -> Option<uuid::Uuid> {
    match *session.persistence.chat_session_id.lock().await {
        Some(id) => Some(id),
        None => *session.persistence.mcp_activity_session_id.lock().await,
    }
}

/// Writes a `connection` entry (best effort, logged on failure). `detail` is
/// a short code (`user`, `network_loss`, `app_exit`, an error code) — never a
/// secret, never free text.
pub async fn record_connection_event(
    session: &Session,
    event: LedgerConnectionEvent,
    detail: Option<&str>,
) {
    let source = event_source(session).await;
    let (Some(store), Some(id)) = (
        session.persistence.ledger_store.as_ref(),
        recorded_session_id(session).await,
    ) else {
        return;
    };
    // The detail is a stable code, so no redaction pass is needed (and the
    // session module stays independent of `orchestration`).
    let content = LedgerEntryContent::Connection {
        event,
        detail: detail.map(str::to_owned),
    };
    if let Err(err) = store
        .append(id, source, &content, &Default::default())
        .await
    {
        tracing::warn!(error = %err, "connection entry persistence failed");
    }
}

/// Like [`record_connection_event`] from synchronous code: runs as a small
/// detached task (a no-op outside a Tokio runtime).
pub fn spawn_connection_event(
    session: Arc<Session>,
    event: LedgerConnectionEvent,
    detail: &'static str,
) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            record_connection_event(&session, event, Some(detail)).await;
        });
    }
}

async fn write_tick(session: &Session) {
    let (Some(store), Some(id)) = (
        &session.persistence.ledger_store,
        recorded_session_id(session).await,
    ) else {
        return;
    };
    let source = event_source(session).await;
    if let Err(err) = store.append_activity_tick(id, source).await {
        tracing::warn!(error = %err, "activity tick could not be stored");
    }
}

/// Awaitable form: claims the current minute and, if it was free, writes the
/// tick before returning.
pub async fn record_activity(session: &Session) {
    if session.persistence.activity.claim() {
        write_tick(session).await;
    }
}

/// Notes that the user did something (terminal input, chat message,
/// confirmation) or a command is running. Returns at once: the claim is
/// synchronous, the write runs detached, so terminal input never waits for
/// the database. At most one write per minute is ever started.
pub fn note_activity(session: &Arc<Session>) {
    if !session.persistence.activity.claim() {
        return;
    }
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let session = Arc::clone(session);
        handle.spawn(async move { write_tick(&session).await });
    }
}

/// Ends the recorded session of `session` (chat or MCP) at shutdown: an empty
/// chat session is discarded (its recorded session stays as `terminal`),
/// any other is marked ended. No-op without a recorded session.
pub async fn end_recorded_session(session: &Session) {
    if let (Some(store), Some(id)) = (
        &session.persistence.chat_session_store,
        *session.persistence.chat_session_id.lock().await,
    ) {
        if let Err(err) = store.end_or_discard(id).await {
            tracing::warn!(error = %err, "chat session could not be ended");
        }
    }
    crate::mcp_sessions::end_recorded_session(session).await;
}
