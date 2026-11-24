//! Activity events of connection changes (issue #1507): connect, loss,
//! reconnect, user close and a failed connect reach the observers.

use app_logic::activity::{
    ActivityEvent, ActivityEventKind, ActivityHub, ActivityObserver, ActivitySessionKind,
    ConnectFailureStep, ConnectionChange,
};

use super::*;

/// Collects every event; the hub delivers inline, so nothing has to be awaited.
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
}

fn observed(fx: &mut Fixture) -> Arc<Recorder> {
    let recorder = Arc::new(Recorder::default());
    fx.state.activity = Arc::new(ActivityHub::new_inline_for_tests(vec![recorder.clone()]));
    recorder
}

fn connection(change: ConnectionChange) -> ActivityEventKind {
    ActivityEventKind::Connection { change }
}

#[tokio::test]
async fn test_remote_session_without_chat_emits_started_terminal_then_connected() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let connector = ScriptedConnector::new(connected(&Arc::new(AtomicBool::new(false))));
    let mut fx = fixture(connector).await;
    let recorder = observed(&mut fx);
    connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        true,
    )
    .await
    .unwrap();
    let kinds = recorder.kinds();
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    assert!(matches!(
        kinds[0],
        ActivityEventKind::SessionStarted {
            kind: ActivitySessionKind::Terminal,
            ..
        }
    ));
    assert_eq!(kinds[1], connection(ConnectionChange::Connected));
    // A remote session starts as a terminal session: no chat id is announced
    // before the first chat message, although the chat session row exists.
    assert!(recorder
        .0
        .lock()
        .unwrap()
        .iter()
        .all(|e| e.session.chat_session_id.is_none()));
    reset(app.handle());
}

#[tokio::test]
async fn test_failed_connect_emits_failed_with_the_step_and_no_session_events() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let connector = ScriptedConnector::new(Err(SshError::AuthenticationFailed));
    let mut fx = fixture(connector).await;
    let recorder = observed(&mut fx);
    let tab = uuid::Uuid::new_v4();
    connect_session(app.handle(), &fx.state, fx.server_id, tab, None, true)
        .await
        .expect_err("must fail");
    assert_eq!(
        recorder.kinds(),
        vec![connection(ConnectionChange::Failed(
            ConnectFailureStep::Transport
        ))]
    );
    let event = recorder.0.lock().unwrap()[0].clone();
    assert_eq!(event.session.runtime_session_id, tab.to_string());
    assert_eq!(event.session.server_id, Some(fx.server_id.0.to_string()));
    // No error text and no secret in the event.
    assert!(!format!("{event:?}").contains("uth"));
    reset(app.handle());
}

#[tokio::test]
async fn test_reconnect_emits_reconnected_in_the_same_session() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let old = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&old));
    let mut fx = fixture(connector.clone()).await;
    let recorder = observed(&mut fx);
    connector.push(connected(&old));
    let session_id = uuid::Uuid::new_v4();
    connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        session_id,
        None,
        true,
    )
    .await
    .unwrap();
    // The loss itself is reported by `report_connection_lost` (tested in
    // app-logic); here only the reconnect after it.
    let session = fx.state.sessions.get(session_id).unwrap();
    *session.conn.status.lock().unwrap() = ConnectionStatus::Disconnected;
    connector.push(connected(&Arc::new(AtomicBool::new(false))));
    let (elevated, grants) = registry_pair();
    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .unwrap();
    let kinds = recorder.kinds();
    assert_eq!(kinds[2..], [connection(ConnectionChange::Reconnected)]);
    reset(app.handle());
}

#[tokio::test]
async fn test_connect_without_observer_constructs_no_event() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let connector = ScriptedConnector::new(Err(SshError::AuthenticationFailed));
    let fx = fixture(connector).await;
    assert!(!fx.state.activity.is_active());
    connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        true,
    )
    .await
    .expect_err("must fail");
    // Nothing was queued or dropped without an observer.
    assert_eq!(fx.state.activity.dropped_events(), 0);
    reset(app.handle());
}
