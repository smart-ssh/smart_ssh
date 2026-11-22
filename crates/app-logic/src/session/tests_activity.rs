//! Activity record of a session (issue #1447): connection events, the
//! content-free activity tick and the "Record active minutes" switch.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use uuid::Uuid;

use ssh_manager_core::audit::{LedgerConnectionEvent, LedgerEntryContent};
use ssh_manager_core::ssh::{InteractiveShell, SshError};

use super::activity::{self, ActivityTicker};
use super::*;
use crate::orchestration::test_support::{
    session_with_ai_provider, MockAiProvider, MockSshTransport,
};
use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::testing::ServerBuilder;

/// A ticker on a clock the test moves by hand (seconds since the epoch).
fn ticker_on(clock: &Arc<AtomicI64>, on: bool) -> ActivityTicker {
    let flag = activity::new_active_minutes_flag();
    flag.store(on, Ordering::Relaxed);
    let clock = Arc::clone(clock);
    ActivityTicker::with_clock(flag, Arc::new(move || clock.load(Ordering::Relaxed)))
}

struct Fixture {
    session: Arc<Session>,
    ledger: persistence_sqlite::SqliteLedgerStore,
    activity_store: persistence_sqlite::SqliteActivitySessionStore,
    recorded_id: Uuid,
    chat: persistence_sqlite::SqliteChatSessionStore,
    clock: Arc<AtomicI64>,
    _tmp: tempfile::TempDir,
}

async fn fixture(record_minutes: bool) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let profiles =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&tmp.path().join("t.sqlite3"))
            .await
            .unwrap();
    let server = ServerBuilder::new().name("Test-Server").build();
    profiles.create_server(&server).await.unwrap();
    let chat = profiles.chat_session_store();
    let recorded_id = chat.create_session(&server.id, None).await.unwrap();
    let ledger = profiles.ledger_store();
    let activity_store = profiles.activity_session_store();

    let mut session =
        session_with_ai_provider(MockAiProvider::new(Vec::new()), MockSshTransport::default());
    session.set_server_id_for_tests(server.id);
    session.set_chat_session_store_for_tests(Some(chat.clone()));
    session.set_ledger_store_for_tests(Some(ledger.clone()));
    session.set_activity_session_store_for_tests(Some(activity_store.clone()));
    session.set_chat_session_id_for_tests(AsyncMutex::new(Some(recorded_id)));
    let clock = Arc::new(AtomicI64::new(1_700_000_000));
    session.set_activity_ticker_for_tests(ticker_on(&clock, record_minutes));
    Fixture {
        session: Arc::new(session),
        ledger,
        activity_store,
        recorded_id,
        chat,
        clock,
        _tmp: tmp,
    }
}

async fn entries(fx: &Fixture) -> Vec<LedgerEntryContent> {
    fx.ledger
        .load_entries(fx.recorded_id)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.content)
        .collect()
}

fn tick_count(all: &[LedgerEntryContent]) -> usize {
    all.iter()
        .filter(|c| matches!(c, LedgerEntryContent::ActivityTick))
        .count()
}

/// Waits (bounded) until the ledger holds `want` ticks; the tick writes of
/// `note_activity` run detached.
async fn wait_for_ticks(fx: &Fixture, want: usize) {
    for _ in 0..1000 {
        if tick_count(&entries(fx).await) >= want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!(
        "expected {want} ticks, found {}",
        tick_count(&entries(fx).await)
    );
}

#[tokio::test]
async fn test_input_in_three_distinct_minutes_writes_three_ticks_and_one_minute_writes_one() {
    let fx = fixture(true).await;
    // Continuous input inside one minute: one tick.
    for second in 0..60 {
        fx.clock
            .store(1_700_000_040 + second % 20, Ordering::Relaxed);
        activity::record_activity(&fx.session).await;
    }
    assert_eq!(tick_count(&entries(&fx).await), 1);
    // Two more distinct minutes: three in total, however often input comes.
    for minute in 1..=2 {
        for second in 0..10 {
            fx.clock
                .store(1_700_000_040 + minute * 120 + second, Ordering::Relaxed);
            activity::record_activity(&fx.session).await;
        }
    }
    assert_eq!(tick_count(&entries(&fx).await), 3);
}

#[tokio::test]
async fn test_tick_moves_last_activity_of_the_recorded_session() {
    let fx = fixture(true).await;
    let before = fx
        .activity_store
        .load(fx.recorded_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.last_activity_at, None);
    activity::record_activity(&fx.session).await;
    let after = fx
        .activity_store
        .load(fx.recorded_id)
        .await
        .unwrap()
        .unwrap();
    assert!(after.last_activity_at.is_some());
}

#[tokio::test]
async fn test_no_tick_is_written_when_recording_active_minutes_is_off() {
    let fx = fixture(false).await;
    for minute in 0..5 {
        fx.clock
            .store(1_700_000_000 + minute * 120, Ordering::Relaxed);
        activity::record_activity(&fx.session).await;
        activity::note_activity(&fx.session);
    }
    // The detached writes had every chance to run.
    tokio::task::yield_now().await;
    assert_eq!(tick_count(&entries(&fx).await), 0);
}

#[tokio::test]
async fn test_switching_the_flag_off_applies_to_an_open_session() {
    let flag = activity::new_active_minutes_flag();
    let clock = Arc::new(AtomicI64::new(0));
    let c = Arc::clone(&clock);
    let ticker =
        ActivityTicker::with_clock(flag.clone(), Arc::new(move || c.load(Ordering::Relaxed)));
    assert!(ticker.claim());
    flag.store(false, Ordering::Relaxed);
    clock.store(600, Ordering::Relaxed);
    assert!(!ticker.claim());
}

/// Records what the terminal wrote to the shell; never returns output.
struct RecordingShell {
    written: Arc<std::sync::Mutex<Vec<u8>>>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

#[async_trait::async_trait]
impl InteractiveShell for RecordingShell {
    async fn write(&mut self, data: &[u8]) -> Result<(), SshError> {
        self.written.lock().unwrap().extend_from_slice(data);
        Ok(())
    }
    async fn read(&mut self) -> Result<Vec<u8>, SshError> {
        Ok(self.rx.recv().await.unwrap_or_default())
    }
    async fn resize(&mut self, _: ssh_manager_core::ssh::PtySize) -> Result<(), SshError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_terminal_input_ticks_and_no_entry_contains_the_typed_text() {
    const MARKER: &str = "KNOWN-INPUT-MARKER-9f3a";
    let fx = fixture(true).await;
    activity::record_connection_event(&fx.session, LedgerConnectionEvent::Connected, None).await;
    let written = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (_out_tx, out_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::channel(8);
    let scope = crate::tasks::TaskSupervisor::new().session_scope(Uuid::new_v4());
    spawn_terminal_actor(
        Uuid::new_v4(),
        Arc::clone(&fx.session),
        Box::new(RecordingShell {
            written: Arc::clone(&written),
            rx: out_rx,
        }),
        cmd_rx,
        Arc::new(crate::events::TestEmitter::default()),
        &scope,
    );
    cmd_tx
        .send(TerminalCommand::Write(
            format!("echo {MARKER}\n").into_bytes(),
        ))
        .await
        .unwrap();
    wait_for_ticks(&fx, 1).await;

    // The input reached the shell unchanged ...
    assert!(String::from_utf8_lossy(&written.lock().unwrap()).contains(MARKER));
    // ... and appears in no stored entry, nor in the recorded session row.
    let stored = serde_json::to_string(&entries(&fx).await).unwrap();
    assert!(!stored.contains(MARKER), "entries: {stored}");
    let session_row = fx.activity_store.load(fx.recorded_id).await.unwrap();
    assert!(!format!("{session_row:?}").contains(MARKER));
}

#[tokio::test]
async fn test_network_loss_and_reconnect_are_recorded_in_the_same_session() {
    let fx = fixture(true).await;
    activity::record_connection_event(&fx.session, LedgerConnectionEvent::Connected, None).await;
    let emitter = crate::events::TestEmitter::default();
    assert!(report_connection_lost(
        Uuid::new_v4(),
        &fx.session,
        &emitter,
        None
    ));
    // The loss entry is written by a detached task: wait for it.
    for _ in 0..1000 {
        if entries(&fx).await.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    activity::record_connection_event(&fx.session, LedgerConnectionEvent::Reconnected, None).await;

    let events: Vec<_> = entries(&fx)
        .await
        .into_iter()
        .filter_map(|c| match c {
            LedgerEntryContent::Connection { event, detail } => Some((event, detail)),
            _ => None,
        })
        .collect();
    assert_eq!(
        events,
        vec![
            (LedgerConnectionEvent::Connected, None),
            (
                LedgerConnectionEvent::Disconnected,
                Some("network_loss".to_string())
            ),
            (LedgerConnectionEvent::Reconnected, None),
        ]
    );
}

#[tokio::test]
async fn test_terminal_only_session_stays_recorded_after_the_empty_chat_is_discarded() {
    let fx = fixture(true).await;
    activity::record_connection_event(&fx.session, LedgerConnectionEvent::Connected, None).await;
    activity::record_connection_event(
        &fx.session,
        LedgerConnectionEvent::Disconnected,
        Some("user"),
    )
    .await;
    fx.chat.end_or_discard(fx.recorded_id).await.unwrap();

    let recorded = fx
        .activity_store
        .load(fx.recorded_id)
        .await
        .unwrap()
        .expect("the recorded session is kept");
    assert_eq!(recorded.kind, "terminal");
    assert!(recorded.ended_at.is_some());
    assert_eq!(entries(&fx).await.len(), 2);
    let listed = fx
        .chat
        .list_sessions_for_server(
            &recorded
                .server_id
                .parse()
                .map(ssh_manager_core::shared::ServerId)
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        listed.is_empty(),
        "a terminal-only session is never offered for resume"
    );
}
