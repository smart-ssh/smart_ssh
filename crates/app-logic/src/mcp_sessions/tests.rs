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
    registry.register(&key, mcp_session_id).unwrap();
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
    registry.register(&first, first_id).unwrap();
    sessions.insert(
        first_id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );

    assert_eq!(registry.reusable_session(&second, &sessions), None);

    let second_id = Uuid::new_v4();
    registry.register(&second, second_id).unwrap();
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
    registry.register(&key, id).unwrap();
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
    registry.register(&key, id).unwrap();

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
    registry
        .register(&McpSessionKey::new(ServerId::new(), None), mcp_id)
        .unwrap();

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
    registry.register(&key, id).unwrap();
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
        registry.register(&key, mcp_session_id).unwrap();
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
        registry.register(&key, session_id).unwrap();
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
        registry.register(&key, session_id).unwrap();
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

// ---- Issue #68: Grenzen für Client-Name, Sitzungen je Server, Anlege-Locks

fn tab_requested_events(emitter: &TestEmitter) -> usize {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "mcp-action-tab-requested")
        .count()
}

/// Trägt `n` verbundene MCP-Sitzungen verschiedener Clients auf `server` ein.
fn fill_server(
    registry: &McpSessionRegistry,
    sessions: &SessionManager,
    server: ServerId,
    n: usize,
) -> Vec<(McpSessionKey, SessionId)> {
    (0..n)
        .map(|i| {
            let key = McpSessionKey::new(server, Some(&format!("client-{i}")));
            let id = Uuid::new_v4();
            registry.register(&key, id).unwrap();
            sessions.insert(
                id,
                Arc::new(session_on(server, MockSshTransport::default())),
            );
            (key, id)
        })
        .collect()
}

/// Issue #68, AC 1: Ein zu langer Name wird gekürzt; zwei Namen, die sich
/// erst hinter der Grenze unterscheiden, ergeben denselben Schlüssel.
#[test]
fn test_client_name_is_capped_and_names_differing_after_the_cap_share_a_key() {
    let server = ServerId::new();
    let prefix = "x".repeat(MCP_CLIENT_NAME_MAX_CHARS);
    let first = McpSessionKey::new(server, Some(&format!("{prefix}-alpha")));
    let second = McpSessionKey::new(server, Some(&format!("{prefix}-beta")));

    assert_eq!(first, second);
    assert_eq!(first.client_name(), Some(prefix.clone()));
    // Genau an der Grenze bleibt der Name unverändert.
    assert_eq!(
        McpSessionKey::new(server, Some(&prefix)).client_name(),
        Some(prefix)
    );
    // Ein Unterschied vor der Grenze bleibt ein eigener Schlüssel.
    assert_ne!(
        McpSessionKey::new(server, Some("Claude Code")),
        McpSessionKey::new(server, Some("Claude Cod"))
    );
}

/// Issue #68, AC 1: Multi-Byte-Namen werden an einer Zeichengrenze
/// gekürzt, ohne Panic; die Grenze zählt Zeichen, nicht Bytes.
#[test]
fn test_multibyte_client_name_is_cut_on_a_char_boundary() {
    let server = ServerId::new();
    for ch in ['ä', '€', '🦀'] {
        let long: String = std::iter::repeat_n(ch, MCP_CLIENT_NAME_MAX_CHARS + 7).collect();
        let name = McpSessionKey::new(server, Some(&long))
            .client_name()
            .expect("Name fehlt");
        assert_eq!(name.chars().count(), MCP_CLIENT_NAME_MAX_CHARS);
        assert!(name.chars().all(|c| c == ch));
    }
    // Mischung aus 1- und 4-Byte-Zeichen, Schnitt direkt hinter einem
    // 4-Byte-Zeichen.
    let mixed = format!("{}🦀🦀🦀", "a".repeat(MCP_CLIENT_NAME_MAX_CHARS - 1));
    let name = normalize_client_name(Some(&mixed)).unwrap();
    assert_eq!(
        name,
        format!("{}🦀", "a".repeat(MCP_CLIENT_NAME_MAX_CHARS - 1))
    );
}

/// Issue #68: Leerraum, der nach dem Kürzen am Ende steht, fällt weg; ein
/// Name aus nur Leerraum bleibt anonym.
#[test]
fn test_normalize_client_name_trims_after_cutting() {
    let name = format!("{} tail", "a".repeat(MCP_CLIENT_NAME_MAX_CHARS - 1));
    assert_eq!(
        normalize_client_name(Some(&name)),
        Some("a".repeat(MCP_CLIENT_NAME_MAX_CHARS - 1))
    );
    assert_eq!(normalize_client_name(Some(&" ".repeat(200))), None);
    assert_eq!(normalize_client_name(None), None);
}

/// Issue #68, AC 2: Bei erreichter Höchstzahl bekommt ein weiterer Client
/// einen Fehler — keine Sitzung eingetragen, kein Tab-Event; der Aufrufer
/// baut ohne `McpSessionSlot::New` keine Verbindung auf.
#[tokio::test]
async fn test_session_cap_rejects_a_further_client_without_tab_or_connection() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    fill_server(&registry, &sessions, server, MCP_MAX_SESSIONS_PER_SERVER);

    let emitter = TestEmitter::default();
    let extra = McpSessionKey::new(server, Some("one too many"));
    let creation = registry.lock_creation(&extra).await;
    assert_eq!(
        registry.acquire_session(&creation, &extra, &sessions, &emitter),
        Err(McpSessionLimitReached)
    );
    assert_eq!(tab_requested_events(&emitter), 0);
    assert!(emitter.events.lock().unwrap().is_empty());
    assert_eq!(registry.reusable_session(&extra, &sessions), None);
    assert_eq!(
        registry.register(&extra, Uuid::new_v4()),
        Err(McpSessionLimitReached)
    );

    // Die Grenze gilt je Server: ein anderer Server ist nicht betroffen.
    let other = McpSessionKey::new(ServerId::new(), Some("one too many"));
    let creation = registry.lock_creation(&other).await;
    assert!(matches!(
        registry.acquire_session(&creation, &other, &sessions, &emitter),
        Ok(McpSessionSlot::New(_))
    ));
}

/// Issue #68: Auch eine abgerissene, aber noch offene MCP-Sitzung belegt
/// einen Platz — ihr Tab ist noch da.
#[test]
fn test_disconnected_open_mcp_session_still_counts_towards_the_cap() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let filled = fill_server(&registry, &sessions, server, MCP_MAX_SESSIONS_PER_SERVER);
    let (key, id) = &filled[0];
    *sessions.get(*id).unwrap().status.lock().unwrap() = ConnectionStatus::Disconnected;

    // Derselbe Client bekommt keine neue Sitzung, solange der alte Tab offen
    // ist und die Höchstzahl erreicht ist.
    assert_eq!(registry.reusable_session(key, &sessions), None);
    assert!(registry.is_mcp_session(*id));
    assert_eq!(
        registry.register(key, Uuid::new_v4()),
        Err(McpSessionLimitReached)
    );
}

/// Issue #68, AC 3: Schließen einer MCP-Sitzung gibt einen Platz frei; die
/// nächste Anfrage bekommt eine neue Sitzung samt Tab.
#[tokio::test]
async fn test_closing_an_mcp_session_frees_a_slot() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let confirmations = ConfirmationRegistry::new();
    let filled = fill_server(&registry, &sessions, server, MCP_MAX_SESSIONS_PER_SERVER);

    let extra = McpSessionKey::new(server, Some("next"));
    assert_eq!(
        registry.register(&extra, Uuid::new_v4()),
        Err(McpSessionLimitReached)
    );

    let (_, closed_id) = filled[2];
    let removed = sessions.remove(closed_id).unwrap();
    assert!(registry.end_session(closed_id, Some(&removed), &confirmations));

    let emitter = TestEmitter::default();
    let creation = registry.lock_creation(&extra).await;
    let slot = registry.acquire_session(&creation, &extra, &sessions, &emitter);
    let Ok(McpSessionSlot::New(new_id)) = slot else {
        panic!("erwartet neue Sitzung, war {slot:?}");
    };
    assert!(registry.is_mcp_session(new_id));
    assert_eq!(tab_requested_events(&emitter), 1);
}

/// Issue #68, AC 4: Bei erreichter Höchstzahl nimmt ein Client seine
/// bestehende, verbundene Sitzung weiter — ohne neues Tab-Event.
#[tokio::test]
async fn test_existing_session_is_reused_when_the_cap_is_reached() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let filled = fill_server(&registry, &sessions, server, MCP_MAX_SESSIONS_PER_SERVER);
    let (key, id) = filled[1].clone();

    let emitter = TestEmitter::default();
    let creation = registry.lock_creation(&key).await;
    assert_eq!(
        registry.acquire_session(&creation, &key, &sessions, &emitter),
        Ok(McpSessionSlot::Existing(id))
    );
    assert!(emitter.events.lock().unwrap().is_empty());
}

#[test]
fn test_limit_message_names_the_cap() {
    assert!(MCP_SESSION_LIMIT_MESSAGE.contains(&MCP_MAX_SESSIONS_PER_SERVER.to_string()));
    assert_eq!(
        McpSessionLimitReached.to_string(),
        MCP_SESSION_LIMIT_MESSAGE
    );
}

/// Issue #68, AC 5: Nach Öffnen und Austragen von Sitzungen hält
/// `creation_locks` keine Einträge mehr für sie.
#[tokio::test]
async fn test_creation_locks_are_dropped_after_sessions_open_and_close() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let emitter = TestEmitter::default();

    let mut opened = Vec::new();
    for i in 0..MCP_MAX_SESSIONS_PER_SERVER {
        let key = McpSessionKey::new(server, Some(&format!("client-{i}")));
        let creation = registry.lock_creation(&key).await;
        assert_eq!(registry.creation_lock_entries(), 1);
        let Ok(McpSessionSlot::New(id)) =
            registry.acquire_session(&creation, &key, &sessions, &emitter)
        else {
            panic!("neue Sitzung erwartet");
        };
        drop(creation);
        opened.push(id);
    }
    assert_eq!(registry.creation_lock_entries(), 0);

    for id in opened {
        assert!(registry.unregister(id).is_some());
    }
    assert_eq!(registry.creation_lock_entries(), 0);
}

/// Issue #68, AC 5: Der Eintrag bleibt, solange jemand wartet — sonst
/// bekäme der Wartende ein anderes Lock als ein später Ankommender —, und
/// verschwindet erst, wenn auch der Letzte fertig ist.
#[tokio::test]
async fn test_creation_lock_entry_survives_while_a_task_waits() {
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(ServerId::new(), Some("Claude Code"));

    let guard = registry.lock_creation(&key).await;
    let mut waiter = Box::pin(registry.lock_creation(&key));
    assert!(futures::poll!(waiter.as_mut()).is_pending());
    drop(guard);
    assert_eq!(registry.creation_lock_entries(), 1);

    // Ein dritter Aufrufer muss auf den Wartenden warten (dasselbe Lock).
    let second = tokio::time::timeout(TEST_TIMEOUT, waiter)
        .await
        .expect("nach Freigabe verfügbar");
    let mut third = Box::pin(registry.lock_creation(&key));
    assert!(futures::poll!(third.as_mut()).is_pending());
    drop(second);
    drop(
        tokio::time::timeout(TEST_TIMEOUT, third)
            .await
            .expect("nach Freigabe verfügbar"),
    );
    assert_eq!(registry.creation_lock_entries(), 0);
}

/// Issue #68: Ein abgebrochenes Warten (MCP-Anfrage verworfen) hinterlässt
/// keinen Eintrag.
#[tokio::test]
async fn test_cancelled_wait_leaves_no_creation_lock_entry() {
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(ServerId::new(), None);

    let guard = registry.lock_creation(&key).await;
    let mut waiter = Box::pin(registry.lock_creation(&key));
    assert!(futures::poll!(waiter.as_mut()).is_pending());
    drop(waiter);
    assert_eq!(registry.creation_lock_entries(), 1);
    drop(guard);
    assert_eq!(registry.creation_lock_entries(), 0);
}

// ---- Issue #67: Zusammensetzung in `ensure_mcp_session`

/// Zählt `connect`-Aufrufe und trägt bei Erfolg — wie `connect_session` in
/// `app-shell` — eine verbundene Sitzung für die übergebene Id ein.
#[derive(Default)]
struct FakeConnector {
    calls: std::sync::Mutex<Vec<SessionId>>,
}

impl FakeConnector {
    fn calls(&self) -> Vec<SessionId> {
        self.calls.lock().unwrap().clone()
    }

    async fn connect_ok(
        &self,
        sessions: &SessionManager,
        server: ServerId,
        session_id: SessionId,
    ) -> Result<(), ()> {
        self.calls.lock().unwrap().push(session_id);
        // Gibt anderen Aufgaben Gelegenheit, dazwischenzukommen — wie ein
        // echter Verbindungsaufbau.
        tokio::task::yield_now().await;
        sessions.insert(
            session_id,
            Arc::new(session_on(server, MockSshTransport::default())),
        );
        Ok(())
    }

    async fn connect_err(&self, session_id: SessionId) -> Result<(), ()> {
        self.calls.lock().unwrap().push(session_id);
        Err(())
    }
}

/// Ein `disconnect_orphan`, der nicht aufgerufen werden darf.
async fn no_orphan(session_id: SessionId) {
    panic!("unerwartetes Trennen von {session_id}");
}

/// Issue #67, AC 1: Mit einem verbundenen **Nutzer**-Tab auf Server X und
/// ohne MCP-Sitzung wird verbunden und eine neue Sitzung geliefert — nie
/// die Nutzer-Sitzung.
#[tokio::test]
async fn test_ensure_mcp_session_connects_a_new_session_next_to_a_user_tab() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let user_session_id = Uuid::new_v4();
    sessions.insert(
        user_session_id,
        Arc::new(session_on(server, MockSshTransport::default())),
    );
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let emitter = TestEmitter::default();
    let connector = FakeConnector::default();

    let (session_id, session) = tokio::time::timeout(
        TEST_TIMEOUT,
        ensure_mcp_session(
            &registry,
            &sessions,
            &key,
            &emitter,
            |id| connector.connect_ok(&sessions, server, id),
            no_orphan,
        ),
    )
    .await
    .expect("Test hing")
    .expect("Sitzung erwartet");

    assert_ne!(session_id, user_session_id);
    assert_eq!(connector.calls(), vec![session_id]);
    assert!(registry.is_mcp_session(session_id));
    assert!(!registry.is_mcp_session(user_session_id));
    assert!(Arc::ptr_eq(&session, &sessions.get(session_id).unwrap()));
    assert_eq!(tab_requested_events(&emitter), 1);
}

/// Issue #67, AC 2: Eine verbundene MCP-Sitzung desselben Schlüssels wird
/// ohne `connect` wiederverwendet.
#[tokio::test]
async fn test_ensure_mcp_session_reuses_the_connected_mcp_session() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let existing = Uuid::new_v4();
    registry.register(&key, existing).unwrap();
    sessions.insert(
        existing,
        Arc::new(session_on(server, MockSshTransport::default())),
    );
    let emitter = TestEmitter::default();
    let connector = FakeConnector::default();

    let (session_id, _) = ensure_mcp_session(
        &registry,
        &sessions,
        &key,
        &emitter,
        |id| connector.connect_ok(&sessions, server, id),
        no_orphan,
    )
    .await
    .expect("Sitzung erwartet");

    assert_eq!(session_id, existing);
    assert!(connector.calls().is_empty());
    assert_eq!(tab_requested_events(&emitter), 0);
}

/// Issue #67, AC 3: Scheitert `connect`, ist die neue Sitzung wieder
/// ausgetragen und der Fehler kommt zurück; die nächste Anfrage verbindet
/// erneut.
#[tokio::test]
async fn test_ensure_mcp_session_unregisters_after_a_failed_connect() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, None);
    let emitter = TestEmitter::default();
    let connector = FakeConnector::default();

    let result = ensure_mcp_session(
        &registry,
        &sessions,
        &key,
        &emitter,
        |id| connector.connect_err(id),
        no_orphan,
    )
    .await;
    assert!(matches!(result, Err(EnsureMcpSessionError::Unavailable)));
    let failed = connector.calls();
    assert_eq!(failed.len(), 1);
    assert!(!registry.is_mcp_session(failed[0]));
    assert_eq!(registry.reusable_session(&key, &sessions), None);

    let (session_id, _) = ensure_mcp_session(
        &registry,
        &sessions,
        &key,
        &emitter,
        |id| connector.connect_ok(&sessions, server, id),
        no_orphan,
    )
    .await
    .expect("zweiter Versuch verbindet");
    assert_ne!(session_id, failed[0]);
    assert_eq!(connector.calls(), vec![failed[0], session_id]);
}

/// Issue #67, AC 4 / Spec 0104, §5: Wird die Sitzung ausgetragen (Tab
/// geschlossen), während `connect` noch läuft, kommt "geschlossen" zurück
/// und die verwaiste Verbindung wird getrennt.
#[tokio::test]
async fn test_ensure_mcp_session_disconnects_an_orphan_closed_during_connect() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let emitter = TestEmitter::default();
    let connector = FakeConnector::default();
    let disconnected = std::sync::Mutex::new(Vec::new());
    let (registry_ref, sessions_ref, connector_ref) = (&registry, &sessions, &connector);

    let result = ensure_mcp_session(
        &registry,
        &sessions,
        &key,
        &emitter,
        |id| async move {
            // Der Nutzer schließt den Tab während des Verbindungsaufbaus
            // (z. B. bei offenem Host-Key-Dialog).
            registry_ref.unregister(id);
            connector_ref.connect_ok(sessions_ref, server, id).await
        },
        |id| {
            let removed = sessions.remove(id).is_some();
            disconnected.lock().unwrap().push((id, removed));
            async {}
        },
    )
    .await;

    assert!(matches!(result, Err(EnsureMcpSessionError::Closed)));
    let connected = connector.calls();
    assert_eq!(connected.len(), 1);
    assert_eq!(*disconnected.lock().unwrap(), vec![(connected[0], true)]);
    assert!(sessions.get(connected[0]).is_none());
    assert!(!registry.is_mcp_session(connected[0]));
}

/// Issue #67, AC 5: Zwei gleichzeitige Anfragen desselben Schlüssels
/// bauen genau eine Verbindung auf und bekommen dieselbe Sitzung.
#[tokio::test]
async fn test_ensure_mcp_session_concurrent_calls_connect_once() {
    let server = ServerId::new();
    let sessions = SessionManager::new();
    let registry = McpSessionRegistry::new();
    let key = McpSessionKey::new(server, Some("Claude Code"));
    let emitter = TestEmitter::default();
    let connector = FakeConnector::default();

    let call = || {
        ensure_mcp_session(
            &registry,
            &sessions,
            &key,
            &emitter,
            |id| connector.connect_ok(&sessions, server, id),
            no_orphan,
        )
    };
    let (first, second) =
        tokio::time::timeout(TEST_TIMEOUT, async { tokio::join!(call(), call()) })
            .await
            .expect("Test hing");

    let (first_id, _) = first.expect("erste Sitzung");
    let (second_id, _) = second.expect("zweite Sitzung");
    assert_eq!(first_id, second_id);
    assert_eq!(connector.calls(), vec![first_id]);
    assert_eq!(tab_requested_events(&emitter), 1);
}
