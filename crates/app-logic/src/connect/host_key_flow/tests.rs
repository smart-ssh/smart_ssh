//! Tests of the host key flow (issue #835): the wait, its end event and the
//! whole decision against a recording host key store, without Tauri.

use crate::events::TestEmitter;
use uuid::Uuid;

use super::*;

/// Spec 0068, Teil 5b: ein nie beantworteter Host-Key-Dialog (Frontend
/// verloren) endet nach der Frist als Ablehnung — nie als Vertrauen —
/// und räumt seinen eigenen Eintrag ab.
#[tokio::test]
async fn test_host_key_wait_times_out_as_rejection_and_cleans_up() {
    let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
        ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();
    let (generation, rx) = registry.register_tracked(session_id);

    let wait = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        wait_for_host_key_decision(
            &registry,
            session_id,
            generation,
            rx,
            std::time::Duration::from_millis(50),
        ),
    )
    .await
    .expect("darf nicht ewig warten");

    assert_eq!(wait, HostKeyWait::TimedOut);
    assert!(!registry.contains(&session_id));
    assert!(
        registry
            .resolve(&session_id, HostKeyUserDecision::Trust)
            .is_err(),
        "ein spätes Trust darf nach Ablauf niemanden mehr erreichen"
    );
}

#[tokio::test]
async fn test_host_key_wait_returns_the_user_decision() {
    let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
        ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();
    let (generation, rx) = registry.register_tracked(session_id);
    registry
        .resolve(&session_id, HostKeyUserDecision::Reject)
        .unwrap();

    let wait = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        wait_for_host_key_decision(
            &registry,
            session_id,
            generation,
            rx,
            std::time::Duration::from_secs(60),
        ),
    )
    .await
    .expect("darf nicht hängen");
    assert_eq!(wait, HostKeyWait::Decided(HostKeyUserDecision::Reject));
}

fn recorded_events(emitter: &TestEmitter) -> Vec<(String, serde_json::Value)> {
    emitter.events.lock().unwrap().clone()
}

/// Issue #37: läuft das Warten in den Timeout, meldet das Backend das
/// Ende genau dieser Abfrage (`sessionId` + `promptId`), und `connect()`
/// scheitert weiterhin mit `SSH_HOST_KEY_CONFIRM_TIMEOUT` — vertraut
/// wird nichts.
#[tokio::test]
async fn test_host_key_timeout_emits_ended_event_and_still_rejects() {
    let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
        ConfirmationRegistry::new();
    let emitter = TestEmitter::default();
    let session_id = Uuid::new_v4();
    let (generation, rx) = registry.register_tracked(session_id);

    let wait = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        wait_for_host_key_decision(
            &registry,
            session_id,
            generation,
            rx,
            std::time::Duration::from_millis(50),
        ),
    )
    .await
    .expect("darf nicht ewig warten");
    let result = finish_host_key_wait(
        &emitter,
        session_id,
        generation,
        wait,
        "prod-1.internal",
        2222,
    );

    let err = result.expect_err("ein Timeout darf nie zu einer Entscheidung werden");
    assert_eq!(err.code, Some("SSH_HOST_KEY_CONFIRM_TIMEOUT"));
    assert_eq!(
        recorded_events(&emitter),
        vec![(
            "host-key-verification-ended".to_string(),
            serde_json::json!({
                "sessionId": session_id,
                "promptId": generation.as_u64(),
                "reason": "timed_out",
            }),
        )]
    );
    assert!(
        registry
            .resolve(&session_id, HostKeyUserDecision::Trust)
            .is_err(),
        "ein spätes Trust darf nach Ablauf niemanden mehr erreichen"
    );
}

/// Issue #37: ein `connect()`-Retry registriert unter derselben
/// `SessionId` neu und droppt damit den Sender der alten Abfrage. Das
/// Ende-Ereignis der alten Abfrage trägt nur deren `promptId` — die neue
/// Abfrage bleibt offen und beantwortbar.
#[tokio::test]
async fn test_host_key_abandoned_wait_emits_ended_event_for_its_registration_only() {
    let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
        ConfirmationRegistry::new();
    let emitter = TestEmitter::default();
    let session_id = Uuid::new_v4();
    let (old_generation, old_rx) = registry.register_tracked(session_id);
    let (new_generation, new_rx) = registry.register_tracked(session_id);
    assert_ne!(old_generation.as_u64(), new_generation.as_u64());

    let wait = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        wait_for_host_key_decision(
            &registry,
            session_id,
            old_generation,
            old_rx,
            std::time::Duration::from_secs(60),
        ),
    )
    .await
    .expect("darf nicht hängen");
    assert_eq!(wait, HostKeyWait::Abandoned);
    let result = finish_host_key_wait(
        &emitter,
        session_id,
        old_generation,
        wait,
        "prod-1.internal",
        2222,
    );

    let err = result.expect_err("ein Abbruch darf nie zu einer Entscheidung werden");
    assert_eq!(err.code, Some("SSH_CONNECTION_ABANDONED"));
    assert_eq!(
        recorded_events(&emitter),
        vec![(
            "host-key-verification-ended".to_string(),
            serde_json::json!({
                "sessionId": session_id,
                "promptId": old_generation.as_u64(),
                "reason": "abandoned",
            }),
        )]
    );
    // Die neue Abfrage ist unberührt und erreicht ihren Wartenden.
    assert!(registry.contains(&session_id));
    registry
        .resolve(&session_id, HostKeyUserDecision::Reject)
        .expect("die neue Abfrage muss noch beantwortbar sein");
    assert_eq!(new_rx.await, Ok(HostKeyUserDecision::Reject));
}

/// Issue #37: auch nach einer Nutzerentscheidung kommt das
/// Ende-Ereignis; die Entscheidung selbst wird unverändert durchgereicht.
#[test]
fn test_host_key_decided_wait_emits_ended_event_and_passes_decision_through() {
    let emitter = TestEmitter::default();
    let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
        ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();
    let (generation, _rx) = registry.register_tracked(session_id);

    let result = finish_host_key_wait(
        &emitter,
        session_id,
        generation,
        HostKeyWait::Decided(HostKeyUserDecision::Reject),
        "prod-1.internal",
        2222,
    );

    assert_eq!(result.ok(), Some(HostKeyUserDecision::Reject));
    let events = recorded_events(&emitter);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["reason"], "decided");
    assert_eq!(events[0].1["promptId"], generation.as_u64());
}

// ---- the whole flow (issue #835) ----

use std::sync::Mutex;

use ssh_manager_core::ssh::SshError;

const HOST: &str = "prod-1.internal";
const PORT: u16 = 2222;

/// Records every `trust` call; `check` is never consulted by the flow.
#[derive(Default)]
struct RecordingStore {
    trusted: Mutex<Vec<(String, u16, Vec<u8>)>>,
}

impl HostKeyStore for RecordingStore {
    fn check(&self, _host: &str, _port: u16, _key: &[u8]) -> HostKeyDecision {
        HostKeyDecision::Trusted
    }
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        self.trusted
            .lock()
            .unwrap()
            .push((host.to_string(), port, key.to_vec()));
        Ok(())
    }
}

struct Fixture {
    store: RecordingStore,
    confirmations: ConfirmationRegistry<SessionId, HostKeyUserDecision>,
    sessions: SessionManager,
    emitter: TestEmitter,
    log: ConnectLog,
    session_id: SessionId,
}

impl Fixture {
    fn new() -> Self {
        Self {
            store: RecordingStore::default(),
            confirmations: ConfirmationRegistry::new(),
            sessions: SessionManager::new(),
            emitter: TestEmitter::default(),
            log: ConnectLog::new(),
            session_id: Uuid::new_v4(),
        }
    }

    async fn run(
        &self,
        interactive: bool,
        timeout: std::time::Duration,
        decision: HostKeyDecision,
        cancel: impl Future<Output = ()>,
    ) -> CommandResult<()> {
        let progress = |_steps: Vec<ConnectStepSnapshot>| {};
        let ctx = HostKeyFlowContext {
            session_id: self.session_id,
            server_id: ServerId(Uuid::new_v4()),
            host_key_store: &self.store,
            confirmations: &self.confirmations,
            sessions: &self.sessions,
            emitter: &self.emitter,
            attempt_log: &self.log,
            progress: &progress,
            interactive,
            timeout,
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            resolve_pending_host_key(
                ctx,
                HOST.to_string(),
                PORT,
                b"raw-key".to_vec(),
                decision,
                cancel,
            ),
        )
        .await
        .expect("the flow must not hang")
    }

    /// Answers the open prompt as soon as it is registered.
    async fn answer(&self, decision: HostKeyUserDecision) {
        while !self.confirmations.contains(&self.session_id) {
            tokio::task::yield_now().await;
        }
        self.confirmations
            .resolve(&self.session_id, decision)
            .expect("the prompt is open");
    }

    fn event_names(&self) -> Vec<String> {
        recorded_events(&self.emitter)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn trusted(&self) -> Vec<(String, u16, Vec<u8>)> {
        self.store.trusted.lock().unwrap().clone()
    }
}

fn unknown() -> HostKeyDecision {
    HostKeyDecision::Unknown {
        fingerprint: "SHA256:new".to_string(),
    }
}

fn mismatch() -> HostKeyDecision {
    HostKeyDecision::Mismatch {
        expected_fingerprint: "SHA256:old".to_string(),
        actual_fingerprint: "SHA256:new".to_string(),
    }
}

const LONG: std::time::Duration = std::time::Duration::from_secs(60);

#[tokio::test]
async fn test_flow_unknown_key_trusted_stores_key_and_retries() {
    let fx = Fixture::new();
    let (result, ()) = tokio::join!(
        fx.run(true, LONG, unknown(), std::future::pending()),
        fx.answer(HostKeyUserDecision::Trust),
    );
    result.expect("Trust means retry");
    assert_eq!(
        fx.trusted(),
        vec![(HOST.to_string(), PORT, b"raw-key".to_vec())]
    );
    assert_eq!(
        fx.event_names(),
        vec![
            "host-key-verification-needed",
            "host-key-verification-ended"
        ]
    );
    let events = recorded_events(&fx.emitter);
    assert_eq!(events[0].1["kind"], "unknown");
    assert_eq!(events[0].1["fingerprint"], "SHA256:new");
    assert_eq!(events[1].1["reason"], "decided");
    assert!(!fx.confirmations.contains(&fx.session_id));
}

#[tokio::test]
async fn test_flow_mismatch_rejected_does_not_store_key() {
    let fx = Fixture::new();
    let (result, ()) = tokio::join!(
        fx.run(true, LONG, mismatch(), std::future::pending()),
        fx.answer(HostKeyUserDecision::Reject),
    );
    let err = result.expect_err("Reject aborts the connect");
    assert_eq!(err.code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
    assert!(fx.trusted().is_empty());
    let events = recorded_events(&fx.emitter);
    assert_eq!(events[0].1["kind"], "mismatch");
    assert_eq!(events[0].1["expectedFingerprint"], "SHA256:old");
    assert_eq!(events[1].1["reason"], "decided");
}

#[tokio::test]
async fn test_flow_timeout_is_rejection_and_emits_ended_for_its_generation() {
    let fx = Fixture::new();
    let result = fx
        .run(
            true,
            std::time::Duration::from_millis(50),
            unknown(),
            std::future::pending(),
        )
        .await;
    let err = result.expect_err("a timeout never trusts");
    assert_eq!(err.code, Some("SSH_HOST_KEY_CONFIRM_TIMEOUT"));
    assert!(fx.trusted().is_empty());
    let events = recorded_events(&fx.emitter);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].0, "host-key-verification-ended");
    assert_eq!(events[1].1["reason"], "timed_out");
    assert_eq!(events[1].1["promptId"], events[0].1["promptId"]);
    assert!(!fx.confirmations.contains(&fx.session_id));
}

#[tokio::test]
async fn test_flow_cancel_while_waiting_releases_registration_and_never_trusts() {
    let fx = Fixture::new();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let cancel = async move {
        let _ = rx.await;
    };
    let canceller = async {
        while !fx.confirmations.contains(&fx.session_id) {
            tokio::task::yield_now().await;
        }
        tx.send(()).unwrap();
    };
    let (result, ()) = tokio::join!(fx.run(true, LONG, unknown(), cancel), canceller);
    let err = result.expect_err("cancel ends the connect");
    assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
    assert!(fx.trusted().is_empty());
    assert!(!fx.confirmations.contains(&fx.session_id));
    let events = recorded_events(&fx.emitter);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].0, "host-key-verification-ended");
    assert_eq!(events[1].1["reason"], "abandoned");
    assert_eq!(events[1].1["promptId"], events[0].1["promptId"]);
}

#[tokio::test]
async fn test_flow_automatic_reconnect_stops_without_prompting() {
    let fx = Fixture::new();
    let result = fx.run(false, LONG, unknown(), std::future::pending()).await;
    let err = result.expect_err("an automatic attempt never trusts");
    assert_eq!(err.code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
    assert!(fx.trusted().is_empty());
    assert!(fx.event_names().is_empty());
    assert!(!fx.confirmations.contains(&fx.session_id));
}
