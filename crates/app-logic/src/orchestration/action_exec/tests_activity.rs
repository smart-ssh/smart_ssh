//! Activity events for observers (issue #1505): every action produces the
//! events matching its ledger entries, built from the same redacted content,
//! and a slow or broken observer never affects the session.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::activity::{
    ActivityDecisionOutcome, ActivityEvent, ActivityEventKind, ActivityHub, ActivityObserver,
    ActivityReporting, ActivitySessionKind, ActivitySource, ServerSnapshot,
};
use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

pub(super) const WAIT: Duration = Duration::from_secs(30);

/// Forwards every event to a channel the test reads.
pub(super) struct Forward(Mutex<Sender<ActivityEvent>>);

impl ActivityObserver for Forward {
    fn on_event(&self, event: &ActivityEvent) {
        let _ = self.0.lock().unwrap().send(event.clone());
    }
}

struct Panicking;

impl ActivityObserver for Panicking {
    fn on_event(&self, _event: &ActivityEvent) {
        panic!("observer bug");
    }
}

/// Blocks until the test drops the sender.
struct Blocking(Mutex<Receiver<()>>);

impl ActivityObserver for Blocking {
    fn on_event(&self, _event: &ActivityEvent) {
        let _ = self.0.lock().unwrap().recv();
    }
}

pub(super) fn snapshot() -> ServerSnapshot {
    ServerSnapshot {
        id: "srv".into(),
        name: "Test".into(),
        host: "example.test".into(),
        port: 22,
        user: "deploy".into(),
        group_ids: vec![],
        group_names: vec![],
        tags: vec![],
    }
}

pub(super) fn with_hub(session: Session, hub: &Arc<ActivityHub>, incomplete: bool) -> Session {
    session
        .safety
        .redaction_incomplete
        .store(incomplete, std::sync::atomic::Ordering::SeqCst);
    session.with_activity_reporting(ActivityReporting::new(hub, "runtime-1".into(), snapshot()))
}

/// A session reporting to a recording observer.
fn observed(session: Session) -> (Session, Receiver<ActivityEvent>) {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    hub.start_detached_worker();
    (with_hub(session, &hub, false), rx)
}

fn take(rx: &Receiver<ActivityEvent>, n: usize) -> Vec<ActivityEvent> {
    (0..n)
        .map(|i| {
            rx.recv_timeout(WAIT)
                .unwrap_or_else(|_| panic!("event {i} of {n} did not arrive"))
        })
        .collect()
}

/// Nothing but a probe is left in the queue: the probe event is the next one.
async fn assert_drained(session: &Session, rx: &Receiver<ActivityEvent>) {
    session
        .emit_session_ended(crate::activity::SessionEndReason::Closed)
        .await;
    let next = take(rx, 1).remove(0);
    assert!(
        matches!(next.kind, ActivityEventKind::SessionEnded { .. }),
        "unexpected extra event: {next:?}"
    );
}

pub(super) fn suggest(command: &str) -> AiAction {
    AiAction::SuggestCommand {
        command: command.to_string(),
    }
}

pub(super) async fn propose(
    session: &Session,
    action: AiAction,
    origin: ActionOrigin,
    emitter: &TestEmitter,
    confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    action_id: ActionId,
) {
    handle_action_proposed(
        session,
        action_id,
        action,
        emitter,
        &InMemoryProfileStore::default(),
        confirmations,
        origin,
        test_fresh_rejection_flag(),
    )
    .await;
}

/// All events belong to one action: they carry the same, non-empty id.
fn assert_one_action_id(events: &[ActivityEvent]) {
    let first = action_id_of(&events[0].kind);
    assert!(!first.is_empty());
    assert!(events.iter().all(|e| action_id_of(&e.kind) == first));
}

fn action_id_of(kind: &ActivityEventKind) -> &str {
    match kind {
        ActivityEventKind::CommandProposed { action_id, .. }
        | ActivityEventKind::Decision { action_id, .. }
        | ActivityEventKind::CommandExecuted { action_id, .. }
        | ActivityEventKind::FileRead { action_id, .. }
        | ActivityEventKind::FileWritten { action_id, .. } => action_id,
        other => panic!("no action id on {other:?}"),
    }
}

pub(super) fn session_allowing_all(transport: MockSshTransport) -> Session {
    session_with_ai_provider_allowing_everything(
        MockAiProvider::new(vec![AiEvent::Done]),
        transport,
    )
}

#[tokio::test]
async fn test_auto_approved_command_emits_proposed_decision_executed_with_one_action_id() {
    let session = session_allowing_all(
        MockSshTransport::default().with_response("ls -la", output("total 0")),
    );
    let (session, rx) = observed(session);
    propose(
        &session,
        suggest("ls -la"),
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    let events = take(&rx, 3);
    assert_drained(&session, &rx).await;
    assert!(matches!(
        &events[0].kind,
        ActivityEventKind::CommandProposed { command, .. } if command == "ls -la"
    ));
    assert!(matches!(
        &events[1].kind,
        ActivityEventKind::Decision {
            outcome: ActivityDecisionOutcome::AutoApproved,
            ..
        }
    ));
    let ActivityEventKind::CommandExecuted {
        stdout,
        exit_code,
        cancelled,
        ..
    } = &events[2].kind
    else {
        panic!("{events:?}")
    };
    assert_eq!(stdout, "total 0");
    assert_eq!(*exit_code, Some(0));
    assert!(!cancelled);
    assert_one_action_id(&events);
    for e in &events {
        assert_eq!(e.source, ActivitySource::Ai);
        assert_eq!(e.session.runtime_session_id, "runtime-1");
        assert!(!e.redaction_incomplete);
    }
}

#[tokio::test]
async fn test_confirmed_command_emits_decision_by_user_between_proposal_and_execution() {
    let session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response("ls", output("x")),
    );
    let (session, rx) = observed(session);
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    tokio::join!(
        propose(
            &session,
            suggest("ls"),
            ActionOrigin::Internal,
            &emitter,
            &confirmations,
            Uuid::new_v4(),
        ),
        approve_first_proposed_action(&emitter, &confirmations)
    );
    let events = take(&rx, 3);
    assert_drained(&session, &rx).await;
    assert!(matches!(
        &events[0].kind,
        ActivityEventKind::CommandProposed { .. }
    ));
    assert_eq!(events[0].source, ActivitySource::Ai);
    assert!(matches!(
        &events[1].kind,
        ActivityEventKind::Decision {
            outcome: ActivityDecisionOutcome::Confirmed,
            ..
        }
    ));
    assert_eq!(events[1].source, ActivitySource::User);
    assert!(matches!(
        &events[2].kind,
        ActivityEventKind::CommandExecuted { .. }
    ));
    assert_one_action_id(&events);
}

#[tokio::test]
async fn test_edited_then_confirmed_command_emits_both_proposals_under_one_action_id() {
    let session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response("echo ok", output("ok")),
    );
    let (session, rx) = observed(session);
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    tokio::join!(
        propose(
            &session,
            suggest("ls"),
            ActionOrigin::Internal,
            &emitter,
            &confirmations,
            Uuid::new_v4(),
        ),
        respond_to_first_proposed_action(
            &emitter,
            &confirmations,
            ActionUserDecision::EditThenApprove {
                command: "echo ok".to_string(),
            }
        )
    );
    let events = take(&rx, 4);
    assert_drained(&session, &rx).await;
    assert!(matches!(
        &events[0].kind,
        ActivityEventKind::CommandProposed { command, .. } if command == "ls"
    ));
    assert!(matches!(
        &events[1].kind,
        ActivityEventKind::CommandProposed { command, .. } if command == "echo ok"
    ));
    assert_eq!(events[1].source, ActivitySource::User);
    assert!(matches!(
        &events[2].kind,
        ActivityEventKind::Decision {
            outcome: ActivityDecisionOutcome::Confirmed,
            ..
        }
    ));
    assert!(matches!(
        &events[3].kind,
        ActivityEventKind::CommandExecuted { command, .. } if command == "echo ok"
    ));
    assert_one_action_id(&events);
}

#[tokio::test]
async fn test_rejected_command_emits_proposal_and_rejection_without_execution() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let (session, rx) = observed(session);
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    tokio::join!(
        propose(
            &session,
            suggest("ls"),
            ActionOrigin::Internal,
            &emitter,
            &confirmations,
            Uuid::new_v4(),
        ),
        deny_first_proposed_action(&emitter, &confirmations)
    );
    let events = take(&rx, 2);
    assert_drained(&session, &rx).await;
    assert!(matches!(
        &events[0].kind,
        ActivityEventKind::CommandProposed { .. }
    ));
    assert!(matches!(
        &events[1].kind,
        ActivityEventKind::Decision {
            outcome: ActivityDecisionOutcome::Rejected,
            ..
        }
    ));
    assert_eq!(events[1].source, ActivitySource::User);
    assert_one_action_id(&events);
}

#[tokio::test]
async fn test_timed_out_confirmation_emits_rejection_with_timeout_code() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let (session, rx) = observed(session);
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    tokio::time::pause();
    let action = propose(
        &session,
        suggest("ls"),
        ActionOrigin::Internal,
        &emitter,
        &confirmations,
        Uuid::new_v4(),
    );
    let advancer = async {
        loop {
            if session.conn.pending_action.lock().unwrap().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        tokio::time::advance(PENDING_ACTION_CONFIRM_TIMEOUT + Duration::from_secs(1)).await;
    };
    tokio::join!(action, advancer);
    tokio::time::resume();
    let events = take(&rx, 2);
    assert_drained(&session, &rx).await;
    let ActivityEventKind::Decision { outcome, code, .. } = &events[1].kind else {
        panic!("{events:?}")
    };
    assert_eq!(*outcome, ActivityDecisionOutcome::Rejected);
    assert_eq!(code.as_deref(), Some("TIMEOUT"));
    assert_one_action_id(&events);
}

#[tokio::test]
async fn test_mcp_command_events_have_source_mcp_agent_and_session_start_names_the_client() {
    let session =
        session_allowing_all(MockSshTransport::default().with_response("ls", output("x")));
    let (session, rx) = observed(session);
    session
        .emit_session_started(ActivitySessionKind::Mcp, Some("Claude Code"))
        .await;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    // MCP-originated actions always need the user's confirmation.
    tokio::join!(
        propose(
            &session,
            suggest("ls"),
            ActionOrigin::Mcp {
                client_name: Some("Claude Code".to_string()),
            },
            &emitter,
            &confirmations,
            Uuid::new_v4(),
        ),
        approve_first_proposed_action(&emitter, &confirmations)
    );
    let events = take(&rx, 4);
    assert_drained(&session, &rx).await;
    let ActivityEventKind::SessionStarted {
        kind,
        mcp_client,
        server,
        ..
    } = &events[0].kind
    else {
        panic!("{events:?}")
    };
    assert_eq!(*kind, ActivitySessionKind::Mcp);
    assert_eq!(mcp_client.as_deref(), Some("Claude Code"));
    assert_eq!(server.host, "example.test");
    assert_eq!(events[1].source, ActivitySource::McpAgent);
    assert_eq!(events[2].source, ActivitySource::User);
    // Same attribution as the ledger: the execution follows the user's approval.
    assert_eq!(events[3].source, ActivitySource::User);
    assert_one_action_id(&events[1..]);
}

#[tokio::test]
async fn test_file_read_event_has_size_and_hash_but_no_content() {
    let session = session_allowing_all(MockSshTransport::default());
    let (session, rx) = observed(session);
    session
        .set_sftp_for_tests(Box::new(
            MockSftpSession::new()
                .with_file("/home/deploy/app.conf", b"CONTENT-MARKER-4711".to_vec()),
        ))
        .await;
    propose(
        &session,
        AiAction::ReadRemoteFile {
            path: "/home/deploy/app.conf".to_string(),
        },
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    let events = take(&rx, 1);
    assert_drained(&session, &rx).await;
    let ActivityEventKind::FileRead {
        path, size, sha256, ..
    } = &events[0].kind
    else {
        panic!("{events:?}")
    };
    assert_eq!(path, "/home/deploy/app.conf");
    assert_eq!(*size, Some(19));
    assert_eq!(sha256.as_deref().map(str::len), Some(64));
    assert!(!format!("{events:?}").contains("CONTENT-MARKER-4711"));
}

#[tokio::test]
async fn test_file_write_event_has_backup_path_and_hashes_but_no_content() {
    let path = "/home/deploy/app.conf";
    let session = session_allowing_all(MockSshTransport::default());
    let (session, rx) = observed(session);
    session
        .set_sftp_for_tests(Box::new(
            MockSftpSession::new().with_file(path, b"OLD-MARKER-1 body".to_vec()),
        ))
        .await;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    tokio::join!(
        propose(
            &session,
            AiAction::WriteRemoteFile {
                path: path.to_string(),
                content: "NEW-MARKER-2 body".to_string(),
            },
            ActionOrigin::Internal,
            &emitter,
            &confirmations,
            Uuid::new_v4(),
        ),
        approve_first_proposed_action(&emitter, &confirmations)
    );
    // Decision (confirmed) and the write itself.
    let events = take(&rx, 2);
    assert_drained(&session, &rx).await;
    let ActivityEventKind::FileWritten {
        backup_path,
        size_before,
        size_after,
        sha256_before,
        sha256_after,
        sudo,
        ..
    } = &events[1].kind
    else {
        panic!("{events:?}")
    };
    assert!(backup_path
        .as_deref()
        .unwrap()
        .starts_with("/home/deploy/app.conf.smartssh-backup-"));
    assert_eq!(*size_before, Some(17));
    assert_eq!(*size_after, Some(17));
    assert_eq!(sha256_before.as_deref().map(str::len), Some(64));
    assert_ne!(sha256_before, sha256_after);
    assert!(!sudo);
    let dump = format!("{events:?}");
    assert!(!dump.contains("OLD-MARKER-1") && !dump.contains("NEW-MARKER-2"));
}

#[tokio::test]
async fn test_secrets_in_output_pattern_and_extension_known_are_masked_in_the_event() {
    let session = session_allowing_all(MockSshTransport::default().with_response(
        "cat db.conf",
        output("password=hunter2geheim token=EXTENSION-ONLY-SECRET-9"),
    ));
    session
        .redactor
        .add_secret_values(&[secrecy::SecretString::from(
            "EXTENSION-ONLY-SECRET-9".to_string(),
        )]);
    let (session, rx) = observed(session);
    propose(
        &session,
        suggest("cat db.conf"),
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    let events = take(&rx, 3);
    assert_drained(&session, &rx).await;
    let dump = format!("{events:?}");
    assert!(!dump.contains("hunter2geheim"), "{dump}");
    assert!(!dump.contains("EXTENSION-ONLY-SECRET-9"), "{dump}");
    assert!(matches!(
        &events[2].kind,
        ActivityEventKind::CommandExecuted { .. }
    ));
}

#[tokio::test]
async fn test_secret_in_command_text_is_masked_in_the_proposal_event() {
    let session = session_allowing_all(MockSshTransport::default());
    session
        .redactor
        .add_secret_values(&[secrecy::SecretString::from("CMD-SECRET-77".to_string())]);
    let (session, rx) = observed(session);
    propose(
        &session,
        suggest("login --pass CMD-SECRET-77"),
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    let events = take(&rx, 3);
    assert_drained(&session, &rx).await;
    assert!(!format!("{events:?}").contains("CMD-SECRET-77"));
}

#[tokio::test]
async fn test_incomplete_redaction_is_flagged_on_the_event() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    hub.start_detached_worker();
    let session = with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        true,
    );
    session
        .emit_session_ended(crate::activity::SessionEndReason::Closed)
        .await;
    assert!(take(&rx, 1)[0].redaction_incomplete);
}

/// Zero cost: without an observer no event is constructed, even though the
/// same command flow runs (and with one, the counter does move).
#[tokio::test]
async fn test_without_observer_no_event_is_constructed() {
    crate::activity::CONSTRUCTED.with(|c| c.set(0));
    let session =
        session_allowing_all(MockSshTransport::default().with_response("ls", output("x")));
    let inactive = Arc::new(ActivityHub::default());
    let session = with_hub(session, &inactive, false);
    assert!(!session.activity_active());
    propose(
        &session,
        suggest("ls"),
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    session
        .emit_session_started(ActivitySessionKind::Terminal, None)
        .await;
    assert_eq!(crate::activity::CONSTRUCTED.with(|c| c.get()), 0);

    let session =
        session_allowing_all(MockSshTransport::default().with_response("ls", output("x")));
    let (session, rx) = observed(session);
    propose(
        &session,
        suggest("ls"),
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    take(&rx, 3);
    assert_eq!(crate::activity::CONSTRUCTED.with(|c| c.get()), 3);
}

#[tokio::test]
async fn test_panicking_observer_neither_breaks_the_command_nor_starves_other_observers() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![
        Arc::new(Panicking),
        Arc::new(Forward(Mutex::new(tx))),
    ]));
    hub.start_detached_worker();
    let transport = MockSshTransport::default().with_response("ls", output("x"));
    let executed = transport.executed_handle();
    let session = with_hub(session_allowing_all(transport), &hub, false);
    propose(
        &session,
        suggest("ls"),
        ActionOrigin::Internal,
        &TestEmitter::default(),
        &ConfirmationRegistry::new(),
        Uuid::new_v4(),
    )
    .await;
    assert_eq!(executed.lock().unwrap().as_slice(), ["ls"]);
    assert_eq!(take(&rx, 3).len(), 3);
}

#[tokio::test]
async fn test_blocking_observer_does_not_delay_commands_and_dropped_events_are_counted() {
    let (release, blocked) = channel::<()>();
    let hub = Arc::new(ActivityHub::with_capacity(
        vec![Arc::new(Blocking(Mutex::new(blocked)))],
        1,
    ));
    hub.start_detached_worker();
    let transport = MockSshTransport::default().with_response("ls", output("x"));
    let executed = transport.executed_handle();
    let session = with_hub(session_allowing_all(transport), &hub, false);
    let run = async {
        for _ in 0..3 {
            propose(
                &session,
                suggest("ls"),
                ActionOrigin::Internal,
                &TestEmitter::default(),
                &ConfirmationRegistry::new(),
                Uuid::new_v4(),
            )
            .await;
        }
    };
    tokio::time::timeout(WAIT, run)
        .await
        .expect("a blocked observer must not block the session");
    assert_eq!(executed.lock().unwrap().len(), 3);
    // 9 events, at most one in the observer and one queued.
    assert!(hub.dropped_events() >= 7, "{}", hub.dropped_events());
    drop(release);
}

/// The default hub (Community) has no observer and no queue consumer.
#[test]
fn test_default_hub_is_inactive() {
    let hub = ActivityHub::default();
    assert!(!hub.is_active());
    assert_eq!(hub.dropped_events(), 0);
}

/// Issue #1533: rebuilds the session's redactor against `store` and returns
/// the flag the next event carries.
async fn flag_after_rebuild(
    started_incomplete: bool,
    store: &crate::test_support::InMemoryProfileStore,
) -> bool {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    hub.start_detached_worker();
    let session = with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        started_incomplete,
    );
    let server_id = session.server_id;
    let manager = crate::session::SessionManager::new();
    let id = Uuid::new_v4();
    manager.insert(id, Arc::new(session));
    crate::session_extension::rebuild_redactors_for_sessions(
        &manager,
        store,
        Arc::new(crate::test_support::InMemoryCredentialStore::new()),
        None,
        server_id,
    )
    .await;
    manager
        .get(id)
        .unwrap()
        .emit_session_ended(crate::activity::SessionEndReason::Closed)
        .await;
    take(&rx, 1)[0].redaction_incomplete
}

#[tokio::test]
async fn test_failing_rebuild_marks_later_events_incomplete() {
    let store = crate::test_support::InMemoryProfileStore::new();
    store
        .fail_get_server
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(flag_after_rebuild(false, &store).await);
}

#[tokio::test]
async fn test_complete_rebuild_clears_the_incomplete_flag_on_events() {
    // Server not stored: no groups, a complete lookup.
    let store = crate::test_support::InMemoryProfileStore::new();
    assert!(!flag_after_rebuild(true, &store).await);
}

/// Two started sessions in a manager, both reporting to `hub`. Their
/// `SessionStarted` events are queued already.
async fn manager_with_two(hub: &Arc<ActivityHub>) -> crate::session::SessionManager {
    let manager = crate::session::SessionManager::default();
    for _ in 0..2 {
        let session = with_hub(
            session_allowing_all(MockSshTransport::default()),
            hub,
            false,
        );
        session
            .emit_session_started(ActivitySessionKind::Terminal, None)
            .await;
        manager.insert(Uuid::new_v4(), Arc::new(session));
    }
    manager
}

/// Issue #1532: the exit sequence emits one `AppExit` per open session and
/// the worker delivers queued events before the shutdown completes.
#[tokio::test]
async fn test_app_exit_emits_one_session_ended_per_session_and_drains_the_queue() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    let sup = crate::tasks::TaskSupervisor::new();
    hub.start_worker(&sup.app_scope());
    let manager = manager_with_two(&hub).await;
    manager.emit_app_exit_activity().await;
    let reports = sup.shutdown(Duration::from_secs(3)).await;
    assert!(reports.iter().all(|r| !r.timed_out));
    // Everything arrived by the time the shutdown returned.
    // Per session: the connection ends (issue #1507), then the session.
    let events: Vec<_> = rx.try_iter().collect();
    assert_eq!(events.len(), 6, "{events:?}");
    // Each start comes before any end.
    assert!(events[..2]
        .iter()
        .all(|e| matches!(e.kind, ActivityEventKind::SessionStarted { .. })));
    let ended: Vec<_> = events[2..]
        .iter()
        .filter(|e| !matches!(e.kind, ActivityEventKind::Connection { .. }))
        .collect();
    assert_eq!(ended.len(), 2);
    for event in ended {
        assert!(matches!(
            event.kind,
            ActivityEventKind::SessionEnded {
                reason: crate::activity::SessionEndReason::AppExit
            }
        ));
    }
}

/// Events queued before the cancellation are delivered, not dropped: the
/// observer holds the first event while the rest queues up and the scope is
/// cancelled, so only the drain can deliver the others.
#[tokio::test]
async fn test_events_queued_before_cancellation_are_delivered_by_the_shutdown() {
    struct Gate {
        first: Mutex<Option<Receiver<()>>>,
        seen: Mutex<Sender<()>>,
        out: Mutex<Sender<ActivityEvent>>,
    }
    impl ActivityObserver for Gate {
        fn on_event(&self, event: &ActivityEvent) {
            if let Some(release) = self.first.lock().unwrap().take() {
                let _ = self.seen.lock().unwrap().send(());
                let _ = release.recv();
            }
            let _ = self.out.lock().unwrap().send(event.clone());
        }
    }
    let (release_tx, release_rx) = channel();
    let (seen_tx, seen_rx) = channel();
    let (out_tx, out_rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Gate {
        first: Mutex::new(Some(release_rx)),
        seen: Mutex::new(seen_tx),
        out: Mutex::new(out_tx),
    })]));
    let sup = crate::tasks::TaskSupervisor::new();
    let scope = sup.app_scope();
    hub.start_worker(&scope);
    let session = with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        false,
    );
    session
        .emit_session_started(ActivitySessionKind::Terminal, None)
        .await;
    seen_rx.recv_timeout(WAIT).expect("worker holds event 1");
    for _ in 0..4 {
        session
            .emit_session_started(ActivitySessionKind::Terminal, None)
            .await;
    }
    scope.cancel();
    release_tx.send(()).unwrap();
    let reports = sup.shutdown(Duration::from_secs(30)).await;
    assert!(reports.iter().all(|r| !r.timed_out));
    assert_eq!(out_rx.try_iter().count(), 5);
}

/// A blocking observer does not hold up the exit beyond the timeout.
#[tokio::test]
async fn test_blocking_observer_does_not_delay_exit_beyond_the_shutdown_timeout() {
    let (release, blocked) = channel::<()>();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Blocking(Mutex::new(
        blocked,
    )))]));
    let sup = crate::tasks::TaskSupervisor::new();
    hub.start_worker(&sup.app_scope());
    let manager = manager_with_two(&hub).await;
    manager.emit_app_exit_activity().await;
    let started = std::time::Instant::now();
    let reports = sup.shutdown(Duration::from_millis(300)).await;
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(reports.iter().any(|r| r.timed_out));
    drop(release);
}

/// Without an observer nothing is built at exit.
#[tokio::test]
async fn test_app_exit_without_observer_constructs_no_event() {
    crate::activity::CONSTRUCTED.with(|c| c.set(0));
    let hub = Arc::new(ActivityHub::default());
    manager_with_two(&hub).await.emit_app_exit_activity().await;
    assert_eq!(crate::activity::CONSTRUCTED.with(|c| c.get()), 0);
}

/// Issue #1539: a session still connecting (no `SessionStarted`) gets no end.
#[tokio::test]
async fn test_app_exit_skips_sessions_that_have_not_started() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    let sup = crate::tasks::TaskSupervisor::new();
    hub.start_worker(&sup.app_scope());
    let manager = manager_with_two(&hub).await;
    manager.insert(
        Uuid::new_v4(),
        Arc::new(with_hub(
            session_allowing_all(MockSshTransport::default()),
            &hub,
            false,
        )),
    );
    manager.emit_app_exit_activity().await;
    sup.shutdown_app(Duration::from_secs(3)).await;
    let ends = rx
        .try_iter()
        .filter(|e| matches!(e.kind, ActivityEventKind::SessionEnded { .. }))
        .count();
    assert_eq!(ends, 2);
}

/// Issue #1539: a session scope that never ends does not use up the app
/// scope's drain budget; a task pushing while it is cancelled delivers
/// before the session's end event.
#[tokio::test]
async fn test_hanging_session_scope_does_not_starve_the_drain_and_events_precede_the_end() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    let sup = crate::tasks::TaskSupervisor::new();
    hub.start_worker(&sup.app_scope());
    let session = Arc::new(with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        false,
    ));
    session
        .emit_session_started(ActivitySessionKind::Terminal, None)
        .await;
    let id = Uuid::new_v4();
    let manager = crate::session::SessionManager::default();
    manager.insert(id, session.clone());
    let scope = sup.session_scope(id);
    // Pushes an event once cancelled (a decision written during the cancel).
    let pusher = session.clone();
    let (armed_tx, armed_rx) = tokio::sync::oneshot::channel::<()>();
    scope.spawn("push-on-cancel", async move {
        struct OnDrop(Arc<Session>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                // Synchronous, so the push is queued before the scope's
                // cancellation completes.
                futures::executor::block_on(self.0.emit_activity_for_content(
                    LedgerSource::Ai,
                    &LedgerEntryContent::AiMessage {
                        text: "late".into(),
                    },
                    None,
                ));
            }
        }
        let _g = OnDrop(pusher);
        let _ = armed_tx.send(());
        std::future::pending::<()>().await;
    });
    // The guard exists before the cancel (a never-polled task has none).
    tokio::time::timeout(WAIT, armed_rx)
        .await
        .expect("task started")
        .unwrap();
    // A second scope that never ends (blocking task ignores cancellation).
    let (release, blocked) = channel::<()>();
    let hang = sup.session_scope(Uuid::new_v4());
    hang.spawn_blocking("hang", move || {
        let _ = blocked.recv();
    });
    let t = std::time::Instant::now();
    manager
        .shutdown_for_exit(
            &sup,
            crate::tasks::ExitBudget {
                sessions: Duration::from_millis(300),
                end_events: Duration::from_secs(1),
                drain: Duration::from_secs(3),
            },
        )
        .await;
    assert!(t.elapsed() < Duration::from_secs(5));
    let kinds: Vec<_> = rx.try_iter().map(|e| e.kind).collect();
    let end = kinds
        .iter()
        .position(|k| matches!(k, ActivityEventKind::SessionEnded { .. }))
        .expect("end delivered");
    let mcp_start = kinds
        .iter()
        .position(|k| matches!(k, ActivityEventKind::AiMessage { .. }))
        .unwrap_or_else(|| panic!("pushed event delivered: {kinds:?}"));
    assert!(mcp_start < end, "{kinds:?}");
    drop(release);
}

/// Issue #1539: a task of the same session that outlives its scope's
/// deadline and pushes while the worker still drains is dropped once the
/// end was queued.
#[tokio::test]
async fn test_no_event_follows_the_session_end_when_its_own_scope_times_out() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    let sup = crate::tasks::TaskSupervisor::new();
    hub.start_worker(&sup.app_scope());
    let session = Arc::new(with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        false,
    ));
    session
        .emit_session_started(ActivitySessionKind::Terminal, None)
        .await;
    let id = Uuid::new_v4();
    let manager = crate::session::SessionManager::default();
    manager.insert(id, session.clone());
    // The same session's task ignores the cancellation (scope times out).
    let (release, blocked) = channel::<()>();
    sup.session_scope(id).spawn_blocking("hang", move || {
        let _ = blocked.recv();
    });
    sup.shutdown_sessions(Duration::from_millis(200)).await;
    manager.emit_app_exit_activity().await;
    // The worker still drains: this late push must not follow the end.
    session
        .emit_session_started(ActivitySessionKind::Mcp, None)
        .await;
    sup.shutdown_app(Duration::from_secs(3)).await;
    let kinds: Vec<_> = rx.try_iter().map(|e| e.kind).collect();
    let end = kinds
        .iter()
        .position(|k| matches!(k, ActivityEventKind::SessionEnded { .. }))
        .expect("end delivered");
    assert_eq!(end, kinds.len() - 1, "{kinds:?}");
    drop(release);
}

/// Issue #1545: a start emitted after the exit began (between phase 1 and
/// the end step, or after the end step) queues nothing, so no start is left
/// without an end; a session started before still gets one start and one end.
#[tokio::test]
async fn test_start_after_exit_began_is_refused_and_started_sessions_end_once() {
    let (tx, rx) = channel();
    let hub = Arc::new(ActivityHub::new(vec![Arc::new(Forward(Mutex::new(tx)))]));
    let sup = crate::tasks::TaskSupervisor::new();
    hub.start_worker(&sup.app_scope());
    let manager = manager_with_two(&hub).await;
    let mk = || {
        Arc::new(with_hub(
            session_allowing_all(MockSshTransport::default()),
            &hub,
            false,
        ))
    };
    // Connect in flight: inserted before the exit, starts after it began.
    let before_end = mk();
    manager.insert(Uuid::new_v4(), before_end.clone());
    let after_end = mk();
    manager.insert(Uuid::new_v4(), after_end.clone());
    let budget = crate::tasks::ExitBudget::default();
    // A task of a session scope that emits its start while `shutdown_sessions`
    // is still waiting for it, i.e. between phase 1 and the end step.
    let (go, go_rx) = channel::<()>();
    let handle = tokio::runtime::Handle::current();
    let in_flight = before_end.clone();
    let spawned = sup
        .session_scope(Uuid::new_v4())
        .spawn_blocking("late-start", move || {
            let _ = go_rx.recv();
            handle.block_on(in_flight.emit_session_started(ActivitySessionKind::Chat, None));
        });
    assert!(spawned);
    let exit = manager.shutdown_for_exit(&sup, budget);
    tokio::pin!(exit);
    // Poll the exit once: it sets the flag synchronously, then waits for the
    // blocking task. No manual `begin_exit` here.
    tokio::select! {
        biased;
        () = &mut exit => panic!("exit must wait for the blocking task"),
        () = std::future::ready(()) => {}
    }
    go.send(()).expect("task alive");
    exit.await;
    after_end
        .emit_session_started(ActivitySessionKind::Chat, None)
        .await;
    // Inserted after the exit began.
    let late = mk();
    manager.insert(Uuid::new_v4(), late.clone());
    late.emit_session_started(ActivitySessionKind::Chat, None)
        .await;
    sup.shutdown_app(Duration::from_secs(3)).await;
    let events: Vec<_> = rx.try_iter().collect();
    let starts = events
        .iter()
        .filter(|e| matches!(e.kind, ActivityEventKind::SessionStarted { .. }))
        .count();
    let ends = events
        .iter()
        .filter(|e| matches!(e.kind, ActivityEventKind::SessionEnded { .. }))
        .count();
    assert_eq!((starts, ends), (2, 2), "{events:?}");
}
