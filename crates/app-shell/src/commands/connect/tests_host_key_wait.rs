//! Tests of the host-key confirmation wait of `connect.rs`.

use app_logic::events::TestEmitter;
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
