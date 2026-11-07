//! Issue #633: live progress and cancel of a tracked connection test.

use super::*;

// --- Issue #633: live progress and cancel ------------------------------

struct CollectingEmitter(std::sync::Mutex<Vec<serde_json::Value>>);

impl EventEmitter for CollectingEmitter {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        if event == "connect-progress" {
            self.0.lock().unwrap().push(payload);
        }
    }
}

/// Starts a TCP step, then hangs (`hang`) or fails authentication.
struct ProgressConnector {
    /// Cancels the attempt once the steps are written, then hangs.
    cancel_with: Option<Arc<crate::connect_attempts::ConnectAttempts>>,
}

#[async_trait]
impl Connector for ProgressConnector {
    async fn connect(
        &self,
        _target: &ConnectionTarget,
        _credentials: &(dyn CredentialStore + Send + Sync),
        _key_files: &(dyn KeyFileReader + Send + Sync),
        _host_keys: Arc<dyn HostKeyStore>,
        _prompter: &dyn ssh_manager_core::ssh::AuthPrompter,
        log: &ConnectLog,
        _limits: ConnectLimits,
    ) -> Result<ConnectOutcome, SshError> {
        let tcp = log.start(
            0,
            "deploy@example.invalid:22",
            ConnectStep::TcpConnect {
                address: Some("192.0.2.7".into()),
                port: 22,
            },
        );
        log.succeed(tcp);
        log.start(
            0,
            "deploy@example.invalid:22",
            ConnectStep::Handshake {
                server_version: None,
                kex: None,
                host_key_algorithm: None,
                cipher: None,
                mac: None,
            },
        );
        if let Some(attempts) = &self.cancel_with {
            assert!(attempts.cancel("att-1"));
            std::future::pending::<()>().await;
        }
        log.fail_running(SshError::AuthenticationFailed.code());
        Err(SshError::AuthenticationFailed)
    }
}

async fn run_tracked(
    connector: &dyn Connector,
    attempts: &crate::connect_attempts::ConnectAttempts,
    emitter: Arc<CollectingEmitter>,
) -> CommandResult<TestConnectionReport> {
    let progress = TestProgress {
        attempt_id: "att-1".into(),
        emitter,
        guard: attempts.register("att-1"),
    };
    tokio::time::timeout(
        Duration::from_secs(10),
        test_connection_tracked(
            &InMemoryProfileStore::new(),
            &InMemoryCredentialStore::new(),
            &MockKeyFileReader::new(),
            Arc::new(NoOpHostKeyStore),
            connector,
            password_input(),
            None,
            &progress,
        ),
    )
    .await
    .expect("test must not hang")
}

#[tokio::test]
async fn tracked_test_emits_progress_events_with_the_attempt_id() {
    let attempts = crate::connect_attempts::ConnectAttempts::new();
    let emitter = Arc::new(CollectingEmitter(Default::default()));
    let report = run_tracked(
        &ProgressConnector { cancel_with: None },
        &attempts,
        emitter.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        report.result,
        TestConnectionResult::AuthenticationFailed
    ));
    let events = emitter.0.lock().unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().all(|e| e["attemptId"] == "att-1"));
    let last = events.last().unwrap();
    let steps = last["steps"].as_array().unwrap();
    assert_eq!(steps.last().unwrap()["status"], "failed");
    assert_eq!(steps[0]["status"], "ok");
}

#[tokio::test]
async fn tracked_test_can_be_cancelled() {
    let attempts = Arc::new(crate::connect_attempts::ConnectAttempts::new());
    let emitter = Arc::new(CollectingEmitter(Default::default()));
    let err = run_tracked(
        &ProgressConnector {
            cancel_with: Some(attempts.clone()),
        },
        &attempts,
        emitter.clone(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
    let events = emitter.0.lock().unwrap();
    let steps = events.last().unwrap()["steps"].as_array().unwrap().clone();
    assert_eq!(steps.last().unwrap()["status"], "failed");
}
