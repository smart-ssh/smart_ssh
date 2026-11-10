//! Spec 0038, section 2a: the session extension's connect check also guards
//! the reconnect of an existing session (issue #1287).

use super::*;

/// Extension that refuses every connect.
struct Refuse;

#[async_trait]
impl app_logic::session_extension::SessionExtension for Refuse {
    async fn check_connect(
        &self,
        _scope: &app_logic::session_extension::ServerScope,
    ) -> Result<(), app_logic::session_extension::ConnectRefusal> {
        Err(app_logic::session_extension::ConnectRefusal::new(
            "server setting X required",
        ))
    }
}

async fn reconnect(
    app: &tauri::App<tauri::test::MockRuntime>,
    fx: &Fixture,
    session_id: SessionId,
) -> CommandResult<()> {
    let (elevated, grants) = registry_pair();
    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        false,
    )
    .await
}

#[tokio::test]
async fn test_refusing_extension_blocks_reconnect_before_the_connector() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let mut fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    fx.state.session_extension = Some(Arc::new(Refuse));
    let calls = connector.calls.load(Ordering::SeqCst);
    connector.push(connected(&Arc::new(AtomicBool::new(false))));

    let err = reconnect(&app, &fx, session_id)
        .await
        .expect_err("extension must refuse");

    assert_eq!(err.code, Some("CONNECT_REFUSED"));
    // A refusal is final for the automatic attempt (-> `Blocked`).
    assert!(!app_logic::auto_reconnect::is_retryable_code(err.code));
    assert_eq!(connector.calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        *session.conn.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );

    // A later reconnect can run again once the extension allows it.
    fx.state.session_extension = None;
    reconnect(&app, &fx, session_id)
        .await
        .expect("later reconnect runs");
    assert_eq!(connector.calls.load(Ordering::SeqCst), calls + 1);
    reset(app.handle());
}

#[tokio::test]
async fn test_default_extension_leaves_reconnect_unchanged() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let mut fx = fixture(connector.clone()).await;
    let (session_id, _session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    fx.state.session_extension = Some(Arc::new(
        app_logic::session_extension::CompositeSessionExtension::new(vec![]),
    ));
    connector.push(connected(&Arc::new(AtomicBool::new(false))));

    reconnect(&app, &fx, session_id)
        .await
        .expect("default extension allows reconnect");
    reset(app.handle());
}
