//! Issue #1010: JSON shape of the chat events that share payload structs.

use super::*;
use ssh_manager_core::sync::lock_tolerating_poison;

/// Issue #1010: `chat-error` JSON — `code` stays present as `null`,
/// absent provider details are omitted, present ones are camelCase.
#[test]
fn test_chat_error_payload_shape_with_and_without_details() {
    let emitter = TestEmitter::default();
    let sid = Uuid::new_v4();
    emit_chat_error(&emitter, sid, "boom".into(), None);
    emit_chat_error_with_details(
        &emitter,
        sid,
        "limit".into(),
        Some("ai_rate_limited"),
        ChatErrorDetails {
            provider_error_type: Some("t".into()),
            http_status: Some(429),
            provider_message: Some("m".into()),
            provider_name: Some("p".into()),
            timeout_secs: Some(30),
            access_returns: Some("later".into()),
        },
    );
    let events = lock_tolerating_poison(&emitter.events);
    assert_eq!(events[0].0, "chat-error");
    assert_eq!(
        events[0].1,
        serde_json::json!({"sessionId": sid.to_string(), "message": "boom", "code": null})
    );
    assert_eq!(
        events[1].1,
        serde_json::json!({
            "sessionId": sid.to_string(), "message": "limit", "code": "ai_rate_limited",
            "providerErrorType": "t", "httpStatus": 429, "providerMessage": "m",
            "providerName": "p", "timeoutSecs": 30, "accessReturns": "later"
        })
    );
}

/// Issue #1010: all session-only events serialise to `{"sessionId": …}`.
#[test]
fn test_session_only_events_payload_shape() {
    let emitter = TestEmitter::default();
    let sid = Uuid::new_v4();
    emit_chat_response_truncated(&emitter, sid);
    emit_chat_web_research_unavailable(&emitter, sid);
    emit_chat_response_empty(&emitter, sid);
    emit_chat_response_refused(&emitter, sid);
    emit_chat_response_cancelled(&emitter, sid);
    emit_chat_queued_messages_sent(&emitter, sid);
    emit_chat_context_compacted(&emitter, sid);
    let events = lock_tolerating_poison(&emitter.events);
    let names: Vec<&str> = events.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "chat-response-truncated",
            "chat-web-research-unavailable",
            "chat-response-empty",
            "chat-response-refused",
            "chat-response-cancelled",
            "chat-queued-messages-sent",
            "chat-context-compacted"
        ]
    );
    for e in events.iter() {
        assert_eq!(e.1, serde_json::json!({"sessionId": sid.to_string()}));
    }
}
