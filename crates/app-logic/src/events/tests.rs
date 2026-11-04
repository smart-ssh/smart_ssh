//! Regressionstest, s. ausführliche Begründung in
//! `crate::dto::tests` — `rename_all` auf einem `#[serde(tag = ...)]`-
//! Enum färbt nur die Tag-Werte camelCase, nicht die Feldnamen
//! innerhalb der Varianten; `exit_code` blieb dadurch snake_case und
//! kam im Frontend als `undefined` an (`result.exitCode`).

use super::*;

/// Issue #672: the payload is exactly `{"kinds": [...]}` — kind names only.
#[test]
fn test_data_changed_payload_carries_only_kinds() {
    use ssh_manager_core::data_change::DataKind;
    let emitter = TestEmitter::default();
    emit_data_changed(
        &emitter,
        &std::collections::BTreeSet::from([DataKind::Servers, DataKind::HostKeys]),
    );
    let events = emitter.events.lock().unwrap();
    assert_eq!(events[0].0, "data-changed");
    assert_eq!(
        events[0].1,
        serde_json::json!({"kinds": ["servers", "host_keys"]})
    );
}

/// Issue #611: `chat-session-changed` payload shape (camelCase, `null`
/// for a session without a persisted chat).
#[test]
fn test_chat_session_changed_payload_shape() {
    let emitter = TestEmitter::default();
    let session_id = Uuid::new_v4();
    let chat = Uuid::new_v4();
    emit_chat_session_changed(&emitter, session_id, Some(chat));
    emit_chat_session_changed(&emitter, session_id, None);
    let events = emitter.events.lock().unwrap();
    assert_eq!(events[0].0, "chat-session-changed");
    assert_eq!(events[0].1["sessionId"], session_id.to_string());
    assert_eq!(events[0].1["chatSessionId"], chat.to_string());
    assert!(events[1].1["chatSessionId"].is_null());
}

/// Issue #37: `host-key-verification-needed` trägt additiv die
/// `promptId`, an der das Frontend das passende Ende-Ereignis erkennt.
#[test]
fn test_host_key_verification_needed_carries_prompt_id() {
    let emitter = TestEmitter::default();
    let session_id = Uuid::new_v4();
    emit_host_key_verification_needed(
        &emitter,
        session_id,
        7,
        "prod-1.internal".to_string(),
        2222,
        HostKeyKind::Unknown,
        "SHA256:abc".to_string(),
        None,
        Some("ssh-ed25519".to_string()),
    );
    let events = emitter.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "host-key-verification-needed");
    assert_eq!(events[0].1["sessionId"], session_id.to_string());
    assert_eq!(events[0].1["promptId"], 7);
    assert_eq!(events[0].1["fingerprint"], "SHA256:abc");
    assert_eq!(events[0].1["keyType"], "ssh-ed25519");
}

#[test]
fn test_host_key_verification_needed_key_type_is_null_when_unknown() {
    let emitter = TestEmitter::default();
    emit_host_key_verification_needed(
        &emitter,
        Uuid::new_v4(),
        1,
        "h".to_string(),
        22,
        HostKeyKind::Unknown,
        "SHA256:abc".to_string(),
        None,
        None,
    );
    let events = emitter.events.lock().unwrap();
    assert!(events[0].1["keyType"].is_null());
}

#[test]
fn test_action_result_payload_command_uses_camel_case_exit_code() {
    let value = ActionResultPayload::Command {
        command: "ls".to_string(),
        stdout: "out".to_string(),
        stderr: String::new(),
        exit_code: Some(0),
        cancelled: false,
        truncated: false,
    };
    let json = serde_json::to_value(&value).unwrap();

    assert_eq!(json["exitCode"], 0);
    assert!(json.get("exit_code").is_none());
}
