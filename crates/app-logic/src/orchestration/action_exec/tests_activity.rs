//! Activity events for observers (issue #1505): every action produces the
//! events matching its ledger entries, built from the same redacted content,
//! and a slow or broken observer never affects the session.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::activity::{
    ActivityDecisionOutcome, ActivityEvent, ActivityEventKind, ActivityHub, ActivityObserver,
    ActivityReporting, ActivitySessionKind, ActivitySource, ServerSnapshot,
};
use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

const WAIT: Duration = Duration::from_secs(30);

/// Forwards every event to a channel the test reads.
struct Forward(Mutex<Sender<ActivityEvent>>);

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

fn snapshot() -> ServerSnapshot {
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

fn with_hub(session: Session, hub: &Arc<ActivityHub>, incomplete: bool) -> Session {
    session.with_activity_reporting(ActivityReporting::new(
        hub,
        "runtime-1".into(),
        snapshot(),
        incomplete,
    ))
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

fn suggest(command: &str) -> AiAction {
    AiAction::SuggestCommand {
        command: command.to_string(),
    }
}

async fn propose(
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

fn session_allowing_all(transport: MockSshTransport) -> Session {
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
        .emit_session_started(ActivitySessionKind::Chat, None)
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
