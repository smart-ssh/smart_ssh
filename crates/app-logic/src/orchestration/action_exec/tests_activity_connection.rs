//! Activity events for connection changes, user activity and failed file
//! writes (issue #1507).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::activity::{
    ActivityEvent, ActivityEventKind, ActivityHub, ActivityObserver, ActivitySessionKind,
    ConnectionChange, DisconnectReason, SessionEndReason,
};
use crate::events::TestEmitter;
use crate::session::{spawn_user_activity_tick, USER_ACTIVITY_INTERVAL};

use super::super::test_support::*;
use super::tests_activity::{session_allowing_all, with_hub};
use super::*;

/// Collects every event; the inline hub delivers during `push`.
#[derive(Default)]
struct Recorder(Mutex<Vec<ActivityEvent>>);

impl ActivityObserver for Recorder {
    fn on_event(&self, event: &ActivityEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

impl Recorder {
    fn kinds(&self) -> Vec<ActivityEventKind> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.kind.clone())
            .collect()
    }
    fn count_activity(&self) -> usize {
        self.kinds()
            .iter()
            .filter(|k| matches!(k, ActivityEventKind::UserActivity))
            .count()
    }
}

fn observed() -> (Arc<Session>, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let hub = Arc::new(ActivityHub::new_inline_for_tests(vec![recorder.clone()]));
    let session = with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        false,
    );
    (Arc::new(session), recorder)
}

fn conn(change: ConnectionChange) -> ActivityEventKind {
    ActivityEventKind::Connection { change }
}

#[tokio::test]
async fn test_terminal_only_session_emits_started_terminal_and_ended() {
    let (session, recorder) = observed();
    session
        .emit_session_started(ActivitySessionKind::Terminal, None)
        .await;
    session.emit_session_ended(SessionEndReason::Closed).await;
    let kinds = recorder.kinds();
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    let ActivityEventKind::SessionStarted { kind, server, .. } = &kinds[0] else {
        panic!("{kinds:?}")
    };
    assert_eq!(*kind, ActivitySessionKind::Terminal);
    assert_eq!(server.name, "Test");
    assert!(matches!(kinds[1], ActivityEventKind::SessionEnded { .. }));
}

#[tokio::test]
async fn test_connection_loss_emits_network_disconnect_once_without_the_reason_text() {
    let (session, recorder) = observed();
    let emitter = TestEmitter::default();
    let id = Uuid::new_v4();
    assert!(
        crate::session::report_connection_lost(
            id,
            &session,
            &emitter,
            Some("password=hunter2 refused".into())
        )
        .await
    );
    assert!(!crate::session::report_connection_lost(id, &session, &emitter, None).await);
    assert_eq!(
        recorder.kinds(),
        vec![conn(ConnectionChange::Disconnected(
            DisconnectReason::Network
        ))]
    );
    assert!(!format!("{:?}", recorder.kinds()).contains("hunter2"));
}

#[tokio::test]
async fn test_app_exit_emits_disconnect_app_exit_then_session_ended() {
    let recorder = Arc::new(Recorder::default());
    let hub = Arc::new(ActivityHub::new_inline_for_tests(vec![recorder.clone()]));
    let manager = crate::session::SessionManager::default();
    let session = with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        false,
    );
    session
        .emit_session_started(ActivitySessionKind::Chat, None)
        .await;
    manager.insert(Uuid::new_v4(), Arc::new(session));
    manager.emit_app_exit_activity().await;
    let kinds = recorder.kinds()[1..].to_vec();
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    assert_eq!(
        kinds[0],
        conn(ConnectionChange::Disconnected(DisconnectReason::AppExit))
    );
    assert!(matches!(
        kinds[1],
        ActivityEventKind::SessionEnded {
            reason: SessionEndReason::AppExit
        }
    ));
}

/// Input in 3 distinct minutes gives exactly 3 events; continuous input in
/// one minute gives 1.
#[tokio::test(start_paused = true)]
async fn test_user_activity_is_at_most_once_per_minute() {
    let (session, recorder) = observed();
    let sup = crate::tasks::TaskSupervisor::new();
    spawn_user_activity_tick(&session, &sup.session_scope(Uuid::new_v4()));
    // Let the tick task register its timer.
    tokio::task::yield_now().await;

    // Minute 1: continuous input (many keystrokes).
    for _ in 0..50 {
        session.note_user_input();
        tokio::time::advance(Duration::from_millis(500)).await;
    }
    assert_eq!(
        recorder.count_activity(),
        0,
        "not before the minute is over"
    );
    tokio::time::advance(USER_ACTIVITY_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(recorder.count_activity(), 1);

    // A quiet minute produces nothing.
    tokio::time::advance(USER_ACTIVITY_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(recorder.count_activity(), 1);

    // Two more distinct minutes with a single input each.
    for expected in [2, 3] {
        session.note_user_input();
        tokio::time::advance(USER_ACTIVITY_INTERVAL).await;
        tokio::task::yield_now().await;
        assert_eq!(recorder.count_activity(), expected);
    }
    // The event carries nothing but the fact.
    assert!(recorder
        .kinds()
        .iter()
        .all(|k| matches!(k, ActivityEventKind::UserActivity)));
}

#[tokio::test(start_paused = true)]
async fn test_running_command_counts_as_activity_without_input() {
    let recorder = Arc::new(Recorder::default());
    let hub = Arc::new(ActivityHub::new_inline_for_tests(vec![recorder.clone()]));
    let transport = MockSshTransport::default();
    let session = Arc::new(with_hub(session_allowing_all(transport), &hub, false));
    let _slot = session
        .transport
        .begin_command(Uuid::new_v4())
        .await
        .unwrap();
    let sup = crate::tasks::TaskSupervisor::new();
    spawn_user_activity_tick(&session, &sup.session_scope(Uuid::new_v4()));
    tokio::task::yield_now().await;
    tokio::time::advance(USER_ACTIVITY_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(recorder.count_activity(), 1);
}

#[tokio::test(start_paused = true)]
async fn test_no_input_marker_reaches_any_event_and_nothing_is_built_without_observer() {
    // Without an observer no tick task runs, no flag is kept, nothing is built.
    crate::activity::CONSTRUCTED.with(|c| c.set(0));
    let inactive = Arc::new(ActivityHub::default());
    let session = Arc::new(with_hub(
        session_allowing_all(MockSshTransport::default()),
        &inactive,
        false,
    ));
    let sup = crate::tasks::TaskSupervisor::new();
    let scope_id = Uuid::new_v4();
    spawn_user_activity_tick(&session, &sup.session_scope(scope_id));
    session.note_user_input();
    session
        .emit_connection(
            ConnectionChange::Connected,
            crate::activity::ActivitySource::User,
        )
        .await;
    tokio::time::advance(USER_ACTIVITY_INTERVAL * 3).await;
    tokio::task::yield_now().await;
    assert_eq!(crate::activity::CONSTRUCTED.with(|c| c.get()), 0);
    assert_eq!(sup.running_in(scope_id), 0, "no tick task without observer");

    // With an observer: input bytes are never seen, only a flag is set
    // (`note_user_input` takes no argument at all), so the marker typed into
    // the terminal cannot be in any event.
    let (session, recorder) = observed();
    let marker = "TYPED-MARKER-7f3a";
    let _typed = marker.as_bytes().to_vec();
    session.note_user_input();
    session
        .emit_connection(
            ConnectionChange::Connected,
            crate::activity::ActivitySource::User,
        )
        .await;
    assert!(!format!("{:?}", recorder.kinds()).contains(marker));
}

#[tokio::test]
async fn test_failed_file_write_emits_file_write_failed_with_code_and_no_content() {
    let path = "/home/deploy/app.conf";
    let recorder = Arc::new(Recorder::default());
    let hub = Arc::new(ActivityHub::new_inline_for_tests(vec![recorder.clone()]));
    let session = with_hub(
        session_allowing_all(MockSshTransport::default()),
        &hub,
        false,
    );
    // A directory at the target: the file exists for `stat`, the write fails.
    session
        .set_sftp_for_tests(Box::new(MockSftpSession::new().with_dir(path)))
        .await;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    tokio::join!(
        super::tests_activity::propose(
            &session,
            AiAction::WriteRemoteFile {
                path: path.to_string(),
                content: "NEW-MARKER-9 body".to_string(),
            },
            ActionOrigin::Internal,
            &emitter,
            &confirmations,
            Uuid::new_v4(),
        ),
        approve_first_proposed_action(&emitter, &confirmations)
    );
    let kinds = recorder.kinds();
    // The decision of the same action carries the action id.
    let action_id = kinds
        .iter()
        .find_map(|k| match k {
            ActivityEventKind::Decision { action_id, .. } => Some(action_id.clone()),
            _ => None,
        })
        .expect("decision event");
    let failed: Vec<_> = kinds
        .iter()
        .filter_map(|k| match k {
            ActivityEventKind::FileWriteFailed {
                action_id: id,
                path: p,
                error_code,
            } => Some((id.clone(), p.clone(), error_code.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 1, "{kinds:?}");
    assert_eq!(failed[0].0, action_id);
    assert_eq!(failed[0].1, path);
    assert!(!failed[0].2.is_empty());
    assert!(!kinds
        .iter()
        .any(|k| matches!(k, ActivityEventKind::FileWritten { .. })));
    assert!(!format!("{kinds:?}").contains("NEW-MARKER-9"));
}
