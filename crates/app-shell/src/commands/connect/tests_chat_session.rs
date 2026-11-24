//! Tests of `should_create_chat_session`.
//! Spec 0040, Abschnitt 4, erster Fix-Punkt: "MCP-ausgelöste Aktionen
//! erzeugen keine `chat_sessions`-Zeile". `should_create_chat_session` ist
//! die vollständige, isolierte Entscheidung dahinter — dieser Test deckt
//! damit den Fix ab, ohne den (hier nicht sinnvoll mockbaren) echten
//! SSH-Verbindungsaufbau in `connect_session` selbst zu brauchen (s.
//! dortiger Doc-Kommentar).

use super::*;

#[test]
fn test_mcp_triggered_new_connection_creates_no_chat_session() {
    assert!(!should_create_chat_session(
        /* is_local */ false, /* persist_chat_session */ false
    ));
}

#[test]
fn test_regular_human_connection_creates_a_chat_session() {
    assert!(should_create_chat_session(
        /* is_local */ false, /* persist_chat_session */ true
    ));
}

#[test]
fn test_local_pseudo_server_never_creates_a_chat_session_even_if_persist_requested() {
    assert!(!should_create_chat_session(
        /* is_local */ true, /* persist_chat_session */ true
    ));
}
