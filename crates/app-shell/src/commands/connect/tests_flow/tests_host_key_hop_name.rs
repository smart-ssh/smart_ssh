//! Issue #1389: a host key prompt names the saved entry of the hop it
//! belongs to.

use super::*;

/// Issue #1389: a host key prompt names the saved entry its key belongs to —
/// the jump host for the bastion's address, the target for its own.
#[tokio::test]
async fn test_host_key_prompt_names_the_hop_it_belongs_to() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let prompts = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let sink = prompts.clone();
    app.handle()
        .listen("host-key-verification-needed", move |e| {
            sink.lock()
                .unwrap()
                .push(serde_json::from_str(e.payload()).unwrap());
        });
    let connector = ScriptedConnector::new(Ok(pending_unknown()));
    let fx = fixture(connector.clone()).await;
    let mut jump = ServerBuilder::new()
        .name("web-1")
        .username("root")
        .post_ingest_policy(ssh_manager_core::profiles::PostIngestPolicy::Balanced)
        .build();
    jump.name = "bastion".to_string();
    jump.host = "bastion.invalid".to_string();
    jump.port = 2222;
    fx.state.profile_store.create_server(&jump).await.unwrap();
    let mut target = fx
        .state
        .profile_store
        .get_server(&fx.server_id)
        .await
        .unwrap();
    target.name = "web-01".to_string();
    target.host = "web.invalid".to_string();
    target.port = 22;
    target.jump_host = Some(jump.id);
    fx.state.profile_store.update_server(&target).await.unwrap();

    for (host, port, expected) in [
        ("bastion.invalid", 2222u16, "bastion"),
        ("web.invalid", 22, "web-01"),
    ] {
        connector.push(Ok(ConnectOutcome::PendingHostKeyConfirmation {
            host: host.to_string(),
            port,
            raw_key: vec![1, 2, 3],
            decision: HostKeyDecision::Unknown {
                fingerprint: "SHA256:x".to_string(),
            },
        }));
        let session_id = uuid::Uuid::new_v4();
        let reject = async {
            loop {
                if fx
                    .state
                    .pending_host_key_confirmations
                    .resolve(&session_id, HostKeyUserDecision::Reject)
                    .is_ok()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (result, ()) = tokio::join!(
            connect_session(
                app.handle(),
                &fx.state,
                fx.server_id,
                session_id,
                None,
                true
            ),
            reject
        );
        assert_eq!(result.unwrap_err().code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
        let seen = prompts.lock().unwrap();
        let last = seen.last().expect("a prompt was emitted");
        assert_eq!(last["host"], host);
        assert_eq!(last["serverName"], expected);
    }
    reset(app.handle());
}
