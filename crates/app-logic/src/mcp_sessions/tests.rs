#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use ssh_manager_core::ai::{AiEvent, ChatMessage, MessageContent, Role};
use ssh_manager_core::filter::FilterEngine;
use ssh_manager_core::profiles::AiAction;
use ssh_manager_core::shared::ServerId;

use super::*;
use crate::events::TestEmitter;
use crate::orchestration::handle_mcp_action_proposed;
use crate::orchestration::test_support::{
    output, resolve_first_confirm, test_session, AllowEverythingPolicyStore, InMemoryProfileStore,
    MockSshTransport,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Eine verbundene Sitzung für `server_id`, deren Filter-Engine alles
/// erlauben würde — damit ein `Confirm` in den Tests unten eindeutig vom
/// MCP-Zwang kommt, nicht von fehlenden Regeln.
fn session_on(server_id: ServerId, transport: MockSshTransport) -> Session {
    let mut session = test_session(vec![AiEvent::Done], transport);
    session.parts_mut_for_tests().server_id = server_id;
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session
}

fn ls_action() -> AiAction {
    AiAction::SuggestCommand {
        command: "ls -la".to_string(),
    }
}

/// Issue #50, AC 1: Ein offener, verbundener Nutzer-Tab auf Server X wird
/// für MCP nie wiederverwendet — die Registry kennt ihn nicht.
#[test]
fn test_connected_user_session_is_never_reused_for_mcp() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let user_session_id = Uuid::new_v4();
    sessions.insert(
        user_session_id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));

    assert_eq!(registry.reusable_session(&key, &sessions), None);
    assert!(!registry.is_mcp_session(user_session_id));
}

#[test]
fn test_registered_connected_mcp_session_is_reused_for_the_same_client() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let mcp_session_id = Uuid::new_v4();
    registry.register(&key, mcp_session_id);
    sessions.insert(
        mcp_session_id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );

    assert_eq!(
        registry.reusable_session(&key, &sessions),
        Some(mcp_session_id)
    );
    assert_eq!(
        registry.info(mcp_session_id),
        Some(McpSessionInfo {
            server_id: server,
            client_name: Some("Claude Code".to_string()),
        })
    );
}

/// Issue #50, AC 6: zwei verschiedene MCP-Clients auf demselben Server
/// bekommen zwei getrennte Sitzungen.
#[test]
fn test_two_mcp_clients_on_the_same_server_get_separate_sessions() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let first = McpSessionKey::new(server, Some("Claude Code"));
    let second = McpSessionKey::new(server, Some("Cursor"));
    let first_id = Uuid::new_v4();
    registry.register(&first, first_id);
    sessions.insert(
        first_id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );

    assert_eq!(registry.reusable_session(&second, &sessions), None);

    let second_id = Uuid::new_v4();
    registry.register(&second, second_id);
    sessions.insert(
        second_id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );
    assert_eq!(registry.reusable_session(&first, &sessions), Some(first_id));
    assert_eq!(
        registry.reusable_session(&second, &sessions),
        Some(second_id)
    );
}

#[test]
fn test_session_key_trims_client_name_and_treats_empty_as_anonymous() {
    let server = ServerId::new();
    assert_eq!(
        McpSessionKey::new(server, Some("  Claude Code ")),
        McpSessionKey::new(server, Some("Claude Code"))
    );
    assert_eq!(
        McpSessionKey::new(server, Some("   ")),
        McpSessionKey::new(server, None)
    );
    assert_eq!(McpSessionKey::new(server, None).client_name(), None);
}

/// Eine abgerissene MCP-Verbindung wird nicht wiederverwendet; der alte
/// Tab bleibt aber eine MCP-Sitzung (keine Nutzer-Eingaben), bis er
/// geschlossen wird.
#[test]
fn test_disconnected_mcp_session_is_not_reused_but_stays_marked() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, None);
    let id = Uuid::new_v4();
    registry.register(&key, id);
    let session = session_on(server, MockSshTransport::default());
    *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
    sessions.insert(id, Arc::new(session));

    assert_eq!(registry.reusable_session(&key, &sessions), None);
    assert!(registry.is_mcp_session(id));
    assert!(registry.ensure_user_session(id).is_err());
}

#[test]
fn test_vanished_mcp_session_is_forgotten() {
    let server = ServerId::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, None);
    let id = Uuid::new_v4();
    registry.register(&key, id);

    assert_eq!(
        registry.reusable_session(&key, &SessionManager::new()),
        None
    );
    assert!(!registry.is_mcp_session(id));
}

/// Issue #50: Der Nutzer-Chat und das Terminal kommen nie in eine
/// MCP-Sitzung (`send_chat_message`/`open_terminal` prüfen das).
#[test]
fn test_user_input_is_rejected_for_mcp_sessions_only() {
    let registry = McpSessionRegistry::new();
    let mcp_id = Uuid::new_v4();
    registry.register(&McpSessionKey::new(ServerId::new(), None), mcp_id);

    let err = registry.ensure_user_session(mcp_id).unwrap_err();
    assert_eq!(err.message, MCP_SESSION_NOT_INTERACTIVE_MESSAGE);
    assert!(registry.ensure_user_session(Uuid::new_v4()).is_ok());
}

#[test]
fn test_unregister_frees_the_key_for_a_new_session() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let id = Uuid::new_v4();
    registry.register(&key, id);
    sessions.insert(
        id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );

    assert!(registry.unregister(id).is_some());
    assert_eq!(registry.reusable_session(&key, &sessions), None);
    assert!(registry.unregister(id).is_none());
}

/// Ein Nicht-MCP-Tab beim Schließen: `end_session` selbst fasst seine
/// wartende Bestätigung nicht an — das macht für jede Sitzung
/// `session::reject_pending_confirmation_on_close` (Issue #66, Tests in
/// `orchestration::action_exec::tests_pending_confirmation`).
#[test]
fn test_end_session_ignores_non_mcp_sessions() {
    let registry = McpSessionRegistry::new();
    let confirmations = ConfirmationRegistry::new();
    let session = session_on(ServerId::new(), MockSshTransport::default());
    let action_id: ActionId = Uuid::new_v4();
    let _rx = confirmations.register(action_id);
    *session.pending_action.lock().unwrap() = Some(action_id);

    assert!(!registry.end_session(Uuid::new_v4(), Some(&session), &confirmations));
    assert!(confirmations.contains(&action_id));
}

/// Issue #50, AC 4 (neuer Sitzungspfad) + AC 1/3: Mit einem offenen
/// Nutzer-Tab auf demselben Server läuft eine MCP-Aktion in der eigenen
/// MCP-Sitzung — weiterhin als `Confirm` mit dem MCP-Code, obwohl eine
/// Allow-Regel greift, und nur über deren Transport. Weder der Transport
/// noch der KI-Verlauf der Nutzer-Sitzung sehen etwas davon, und der
/// Nutzer-Verlauf kommt nicht in die MCP-Sitzung.
#[tokio::test]
async fn test_mcp_action_runs_only_in_the_mcp_session_and_stays_confirm() {
    tokio::time::timeout(TEST_TIMEOUT, async {
        let server = ServerId::new();
        let sessions = SessionManager::new();
        let registry = McpSessionRegistry::new();

        let user_transport = MockSshTransport::default();
        let user_executed = user_transport.executed_handle();
        let user_session = Arc::new(session_on(server, user_transport));
        user_session.context.lock().await.history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text("geheimer Nutzer-Kontext".to_string()),
        });
        let user_session_id = Uuid::new_v4();
        sessions.insert(user_session_id, Arc::clone(&user_session));

        let key = McpSessionKey::new(server, Some("Claude Code"));
        assert_eq!(registry.reusable_session(&key, &sessions), None);
        let mcp_transport = MockSshTransport::default().with_response("ls -la", output("total 0"));
        let mcp_executed = mcp_transport.executed_handle();
        let mcp_session = Arc::new(session_on(server, mcp_transport));
        let mcp_session_id = Uuid::new_v4();
        registry.register(&key, mcp_session_id);
        sessions.insert(mcp_session_id, Arc::clone(&mcp_session));
        assert_ne!(mcp_session_id, user_session_id);

        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        tokio::join!(
            handle_mcp_action_proposed(
                &mcp_session,
                mcp_session_id,
                ls_action(),
                &emitter,
                &profile_store,
                &confirmations,
                Some("Claude Code".to_string()),
            ),
            resolve_first_confirm(&emitter, &confirmations, ActionUserDecision::Approve),
        );

        let events = emitter.events.lock().unwrap().clone();
        let (_, proposed) = events
            .iter()
            .find(|(name, _)| name == "chat-action-proposed")
            .expect("chat-action-proposed fehlt");
        assert_eq!(
            proposed["decision"]["Confirm"]["code"],
            serde_json::json!("FILTER_MCP_ORIGIN_REQUIRES_CONFIRM")
        );
        // Jedes Event gehört zur MCP-Sitzung, keines zum Nutzer-Tab.
        for (name, payload) in &events {
            if let Some(id) = payload.get("sessionId") {
                assert_eq!(
                    id,
                    &serde_json::json!(mcp_session_id),
                    "Event {name} trifft nicht die MCP-Sitzung"
                );
            }
        }

        assert_eq!(*mcp_executed.lock().unwrap(), vec!["ls -la".to_string()]);
        assert!(user_executed.lock().unwrap().is_empty());

        let user_history = user_session.context.lock().await.history.clone();
        assert_eq!(
            user_history.len(),
            1,
            "MCP-Ausgabe im Nutzer-Verlauf: {user_history:?}"
        );

        let mcp_history = mcp_session.context.lock().await.history.clone();
        assert!(
            mcp_history
                .iter()
                .any(|m| matches!(m.role, Role::ActionResult)),
            "MCP-Ergebnis fehlt im MCP-Verlauf: {mcp_history:?}"
        );
        assert!(
            !format!("{mcp_history:?}").contains("geheimer Nutzer-Kontext"),
            "Nutzer-Verlauf in der MCP-Sitzung: {mcp_history:?}"
        );
    })
    .await
    .expect("Test hing");
}

/// Issue #50, AC 5: Schließen des MCP-Tabs lehnt die wartende Bestätigung
/// ab (fail closed) — die Aktion wird nicht ausgeführt, endet nicht erst am
/// Timeout, und die nächste Anfrage bekommt eine neue Sitzung.
#[tokio::test]
async fn test_closing_the_mcp_session_rejects_its_pending_confirmation() {
    tokio::time::timeout(TEST_TIMEOUT, async {
        let server = ServerId::new();
        let sessions = SessionManager::new();
        let registry = McpSessionRegistry::new();
        let key = McpSessionKey::new(server, Some("Claude Code"));

        let transport = MockSshTransport::default().with_response("ls -la", output("total 0"));
        let executed = transport.executed_handle();
        let session = Arc::new(session_on(server, transport));
        let session_id = Uuid::new_v4();
        registry.register(&key, session_id);
        sessions.insert(session_id, Arc::clone(&session));

        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        let close = async {
            while session.pending_action.lock().unwrap().is_none() {
                tokio::task::yield_now().await;
            }
            // Wie `app_shell::commands::disconnect`: erst aus dem
            // `SessionManager`, dann austragen und ablehnen.
            let removed = sessions.remove(session_id).expect("Sitzung fehlt");
            assert!(registry.end_session(session_id, Some(&removed), &confirmations));
        };
        tokio::join!(
            handle_mcp_action_proposed(
                &session,
                session_id,
                ls_action(),
                &emitter,
                &profile_store,
                &confirmations,
                Some("Claude Code".to_string()),
            ),
            close,
        );

        assert!(
            executed.lock().unwrap().is_empty(),
            "Aktion lief trotz Schließen"
        );
        assert!(session.pending_action.lock().unwrap().is_none());
        let history = session.context.lock().await.history.clone();
        assert!(
            history
                .iter()
                .any(|m| matches!(m.content, MessageContent::ActionRejected { .. })),
            "Ablehnung fehlt im MCP-Verlauf: {history:?}"
        );
        assert!(!registry.is_mcp_session(session_id));
        assert_eq!(registry.reusable_session(&key, &sessions), None);
    })
    .await
    .expect("Test hing");
}

/// Issue #66, AC 2: Über den sitzungsunabhängigen Schließ-Schritt, den
/// `disconnect` jetzt aufruft, verhält sich eine MCP-Sitzung wie bisher —
/// abgelehnt, nichts ausgeführt, ausgetragen, und zwar ausgetragen, bevor
/// die wartende Aktion ihr Ergebnis meldet (dafür prüft
/// `app_shell::mcp_backend`, ob die Sitzung noch eingetragen ist).
#[tokio::test]
async fn test_generic_close_keeps_the_mcp_session_behaviour() {
    tokio::time::timeout(TEST_TIMEOUT, async {
        let server = ServerId::new();
        let sessions = SessionManager::new();
        let registry = McpSessionRegistry::new();
        let key = McpSessionKey::new(server, Some("Claude Code"));

        let transport = MockSshTransport::default().with_response("ls -la", output("total 0"));
        let executed = transport.executed_handle();
        let session = Arc::new(session_on(server, transport));
        let session_id = Uuid::new_v4();
        registry.register(&key, session_id);
        sessions.insert(session_id, Arc::clone(&session));

        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        let close = async {
            while session.pending_action.lock().unwrap().is_none() {
                tokio::task::yield_now().await;
            }
            let removed = sessions.remove(session_id).expect("Sitzung fehlt");
            crate::session::reject_pending_confirmation_on_close(
                session_id,
                Some(&removed),
                &registry,
                &confirmations,
            );
        };
        let registered_when_action_returned = async {
            handle_mcp_action_proposed(
                &session,
                session_id,
                ls_action(),
                &emitter,
                &profile_store,
                &confirmations,
                Some("Claude Code".to_string()),
            )
            .await;
            registry.is_mcp_session(session_id)
        };
        let (still_registered, ()) = tokio::join!(registered_when_action_returned, close);

        assert!(
            !still_registered,
            "die MCP-Sitzung muss ausgetragen sein, wenn ihre Aktion zurückkehrt"
        );
        assert!(
            executed.lock().unwrap().is_empty(),
            "Aktion lief trotz Schließen"
        );
        let history = session.context.lock().await.history.clone();
        assert!(
            history
                .iter()
                .any(|m| matches!(m.content, MessageContent::ActionRejected { .. })),
            "Ablehnung fehlt im MCP-Verlauf: {history:?}"
        );
        assert_eq!(registry.reusable_session(&key, &sessions), None);
    })
    .await
    .expect("Test hing");
}

/// Zwei gleichzeitige Anfragen desselben Clients an denselben Server
/// warten aufeinander, statt zwei Verbindungen aufzubauen.
#[tokio::test]
async fn test_creation_lock_serialises_the_same_key_only() {
    let registry = McpSessionRegistry::new();
    let server = ServerId::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let other = McpSessionKey::new(server, Some("Cursor"));

    let guard = registry.lock_creation(&key).await;
    // Anderer Schlüssel: sofort verfügbar.
    tokio::time::timeout(TEST_TIMEOUT, registry.lock_creation(&other))
        .await
        .expect("anderer Schlüssel darf nicht blockieren");
    // Derselbe Schlüssel: blockiert, solange der Guard lebt.
    let mut same = Box::pin(registry.lock_creation(&key));
    assert!(futures::poll!(same.as_mut()).is_pending());
    drop(guard);
    tokio::time::timeout(TEST_TIMEOUT, same)
        .await
        .expect("nach Freigabe verfügbar");
}
