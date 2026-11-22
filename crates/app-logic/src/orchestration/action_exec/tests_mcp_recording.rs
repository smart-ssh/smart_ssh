//! Activity record for MCP-driven actions (issue #1444, Spec 0057): an MCP
//! connection records under its own `mcp` recorded session, source
//! `mcp-agent`, with the same redaction as chat entries.

use async_trait::async_trait;

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent, LedgerSource};
use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyRules, PolicyStore, Rule};
use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::testing::ServerBuilder;

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

struct Fixture {
    session: Session,
    profiles: persistence_sqlite::SqliteProfileStore,
    server_id: ServerId,
    mcp_id: Uuid,
    _tmp: tempfile::TempDir,
}

async fn mcp_fixture(transport: MockSshTransport, client: Option<&str>) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let profiles =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&tmp.path().join("t.sqlite3"))
            .await
            .unwrap();
    let server = ServerBuilder::new().name("Test-Server").build();
    profiles.create_server(&server).await.unwrap();
    let mut session = session_with_ai_provider(MockAiProvider::new(vec![AiEvent::Done]), transport);
    session.set_server_id_for_tests(server.id);
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
    session.set_ledger_store_for_tests(Some(profiles.ledger_store()));
    session.set_activity_session_store_for_tests(Some(profiles.activity_session_store()));
    crate::mcp_sessions::attach_recorded_session(&session, &server.id, client).await;
    let mcp_id = session
        .persistence
        .mcp_activity_session_id
        .lock()
        .await
        .expect("an MCP recorded session must be attached");
    Fixture {
        session,
        profiles,
        server_id: server.id,
        mcp_id,
        _tmp: tmp,
    }
}

fn suggest(command: &str) -> AiAction {
    AiAction::SuggestCommand {
        command: command.to_string(),
    }
}

async fn propose_mcp(session: &Session, command: &str, emitter: &TestEmitter) {
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();
    handle_action_proposed(
        session,
        Uuid::new_v4(),
        suggest(command),
        emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    )
    .await;
}

#[tokio::test]
async fn test_mcp_confirmed_action_records_proposed_decision_executed_as_mcp_agent() {
    let fx = mcp_fixture(
        MockSshTransport::default().with_response("ls -la", output("total 0")),
        Some("Claude Code"),
    )
    .await;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();
    let action = handle_action_proposed(
        &fx.session,
        Uuid::new_v4(),
        suggest("ls -la"),
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    tokio::join!(
        action,
        approve_first_proposed_action(&emitter, &confirmations)
    );

    let entries = fx
        .profiles
        .ledger_store()
        .load_entries(fx.mcp_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            !matches!(
                e.content,
                ssh_manager_core::audit::LedgerEntryContent::ActivityTick
                    | ssh_manager_core::audit::LedgerEntryContent::Connection { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 3, "{entries:?}");
    assert_eq!(entries[0].source, LedgerSource::McpAgent);
    assert!(matches!(
        entries[0].content,
        LedgerEntryContent::CommandProposed { .. }
    ));
    assert!(matches!(
        entries[1].content,
        LedgerEntryContent::Decision {
            outcome: LedgerDecisionOutcome::Confirmed,
            ..
        }
    ));
    assert!(matches!(
        entries[2].content,
        LedgerEntryContent::CommandExecuted { .. }
    ));
    // The recorded session is an MCP one named after the client, no chat.
    let recorded = fx
        .profiles
        .activity_session_store()
        .load(fx.mcp_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recorded.kind, "mcp");
    assert_eq!(recorded.mcp_client.as_deref(), Some("Claude Code"));
    assert!(fx
        .profiles
        .chat_session_store()
        .list_sessions_for_server(&fx.server_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_mcp_action_rejected_by_rule_records_proposed_and_rejected_only() {
    struct DenyLs;
    #[async_trait]
    impl PolicyStore for DenyLs {
        async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
            PolicyRules::complete(vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-ls".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("ls *".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }])
        }
    }
    let mut fx = mcp_fixture(MockSshTransport::default(), Some("Claude Code")).await;
    fx.session
        .set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyLs)));
    propose_mcp(&fx.session, "ls -la", &TestEmitter::default()).await;

    let entries = fx
        .profiles
        .ledger_store()
        .load_entries(fx.mcp_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            !matches!(
                e.content,
                ssh_manager_core::audit::LedgerEntryContent::ActivityTick
                    | ssh_manager_core::audit::LedgerEntryContent::Connection { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0].source, LedgerSource::McpAgent);
    assert!(matches!(
        entries[1].content,
        LedgerEntryContent::Decision {
            outcome: LedgerDecisionOutcome::Rejected,
            ..
        }
    ));
}

/// A real MCP confirmation timeout on a real database (paused clock set up
/// after the database, as in the chat counterpart
/// `test_ledger_records_origin_derived_source_and_timeout_code_for_timed_out_confirmation`).
#[tokio::test]
async fn test_mcp_timeout_decision_is_attributed_to_the_mcp_agent_with_timeout_code() {
    let fx = mcp_fixture(MockSshTransport::default(), Some("Claude Code")).await;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();
    tokio::time::pause();
    let action_future = handle_action_proposed(
        &fx.session,
        Uuid::new_v4(),
        suggest("ls -la"),
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    let advancer = async {
        loop {
            if fx.session.conn.pending_action.lock().unwrap().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        tokio::time::advance(PENDING_ACTION_CONFIRM_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
        // Resume before the timeout branch writes its ledger entry (real
        // SQLite I/O must not run on the paused clock).
        tokio::time::resume();
    };
    tokio::join!(action_future, advancer);

    let entries = fx
        .profiles
        .ledger_store()
        .load_entries(fx.mcp_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            !matches!(
                e.content,
                ssh_manager_core::audit::LedgerEntryContent::ActivityTick
                    | ssh_manager_core::audit::LedgerEntryContent::Connection { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0].source, LedgerSource::McpAgent);
    assert!(matches!(
        entries[0].content,
        LedgerEntryContent::CommandProposed { .. }
    ));
    assert_eq!(entries[1].source, LedgerSource::McpAgent);
    assert!(matches!(
        &entries[1].content,
        LedgerEntryContent::Decision {
            outcome: LedgerDecisionOutcome::Rejected,
            code: Some(code),
            ..
        } if code == "TIMEOUT"
    ));
}

/// A chat session and an MCP session on one server write to two separate
/// recorded sessions; each ledger holds only its own entries.
#[tokio::test]
async fn test_chat_and_mcp_session_on_one_server_are_two_recorded_sessions() {
    let mcp = mcp_fixture(
        MockSshTransport::default().with_response("ls -la", output("total 0")),
        Some("Claude Code"),
    )
    .await;
    let chat_id = mcp
        .profiles
        .chat_session_store()
        .create_session(&mcp.server_id, None)
        .await
        .unwrap();
    let mut chat = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response("pwd", output("/")),
    );
    chat.set_server_id_for_tests(mcp.server_id);
    chat.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
    chat.set_ledger_store_for_tests(Some(mcp.profiles.ledger_store()));
    chat.set_chat_session_store_for_tests(Some(mcp.profiles.chat_session_store()));
    chat.set_chat_session_id_for_tests(tokio::sync::Mutex::new(Some(chat_id)));
    assert_ne!(chat_id, mcp.mcp_id);

    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();
    let emitter = TestEmitter::default();
    let mcp_action = handle_action_proposed(
        &mcp.session,
        Uuid::new_v4(),
        suggest("ls -la"),
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        test_fresh_rejection_flag(),
    );
    tokio::join!(
        mcp_action,
        approve_first_proposed_action(&emitter, &confirmations)
    );
    // The chat side runs without a confirmation (allow-everything policy).
    let chat_emitter = TestEmitter::default();
    let chat_confirmations = ConfirmationRegistry::new();
    let chat_action = handle_action_proposed(
        &chat,
        Uuid::new_v4(),
        suggest("pwd"),
        &chat_emitter,
        &profile_store,
        &chat_confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    chat_action.await;

    let ledger = mcp.profiles.ledger_store();
    let mcp_entries = ledger
        .load_entries(mcp.mcp_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            !matches!(
                e.content,
                ssh_manager_core::audit::LedgerEntryContent::ActivityTick
                    | ssh_manager_core::audit::LedgerEntryContent::Connection { .. }
            )
        })
        .collect::<Vec<_>>();
    let chat_entries = ledger
        .load_entries(chat_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            !matches!(
                e.content,
                ssh_manager_core::audit::LedgerEntryContent::ActivityTick
                    | ssh_manager_core::audit::LedgerEntryContent::Connection { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(!mcp_entries.is_empty() && !chat_entries.is_empty());
    assert!(mcp_entries
        .iter()
        .all(|e| e.source == LedgerSource::McpAgent || e.source == LedgerSource::User));
    assert!(mcp_entries.iter().any(|e| matches!(
        &e.content,
        LedgerEntryContent::CommandProposed { command, .. } if command == "ls -la"
    )));
    assert!(chat_entries
        .iter()
        .all(|e| e.source != LedgerSource::McpAgent));
    assert!(chat_entries.iter().any(|e| matches!(
        &e.content,
        LedgerEntryContent::CommandProposed { command, .. } if command == "pwd"
    )));
    assert!(!mcp_entries.iter().any(|e| matches!(
        &e.content,
        LedgerEntryContent::CommandProposed { command, .. } if command == "pwd"
    )));
    assert!(!chat_entries.iter().any(|e| matches!(
        &e.content,
        LedgerEntryContent::CommandProposed { command, .. } if command == "ls -la"
    )));
}

/// Closing the MCP tab while it is still connecting ends the recorded
/// session attached by `connect`.
#[tokio::test]
async fn test_mcp_tab_closed_during_connect_ends_the_recorded_session() {
    let tmp = tempfile::tempdir().unwrap();
    let profiles =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&tmp.path().join("t.sqlite3"))
            .await
            .unwrap();
    let server = ServerBuilder::new().name("Test-Server").build();
    profiles.create_server(&server).await.unwrap();
    let sessions = crate::session::SessionManager::new();
    let registry = crate::mcp_sessions::McpSessionRegistry::new();
    let key = crate::mcp_sessions::McpSessionKey::new(server.id, Some("Claude Code"));
    let emitter = TestEmitter::default();
    let recorded = std::sync::Mutex::new(None);

    let result = crate::mcp_sessions::ensure_mcp_session(
        &registry,
        &sessions,
        &key,
        &emitter,
        |id| {
            let (registry, sessions, profiles, recorded) =
                (&registry, &sessions, &profiles, &recorded);
            async move {
                let mut session = session_with_ai_provider(
                    MockAiProvider::new(vec![AiEvent::Done]),
                    MockSshTransport::default(),
                );
                session.set_server_id_for_tests(server.id);
                session
                    .set_activity_session_store_for_tests(Some(profiles.activity_session_store()));
                let session = std::sync::Arc::new(session);
                sessions.insert(id, session.clone());
                crate::mcp_sessions::attach_recorded_session(
                    &session,
                    &server.id,
                    Some("Claude Code"),
                )
                .await;
                *recorded.lock().unwrap() =
                    *session.persistence.mcp_activity_session_id.lock().await;
                // The user closes the tab while connecting.
                registry.unregister(id);
                Ok::<(), ()>(())
            }
        },
        |_| async {},
    )
    .await;
    assert!(result.is_err());
    let mcp_id = recorded.lock().unwrap().expect("recorded session attached");
    let row = profiles
        .activity_session_store()
        .load(mcp_id)
        .await
        .unwrap()
        .unwrap();
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn test_mcp_run_output_secret_is_redacted_in_the_stored_entry() {
    let fx = mcp_fixture(
        MockSshTransport::default()
            .with_response("cat db.conf", output("ok, password=hunter2geheim")),
        None,
    )
    .await;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();
    let action = handle_action_proposed(
        &fx.session,
        Uuid::new_v4(),
        suggest("cat db.conf"),
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp { client_name: None },
        test_fresh_rejection_flag(),
    );
    tokio::join!(
        action,
        approve_first_proposed_action(&emitter, &confirmations)
    );

    let entries = fx
        .profiles
        .ledger_store()
        .load_entries(fx.mcp_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            !matches!(
                e.content,
                ssh_manager_core::audit::LedgerEntryContent::ActivityTick
                    | ssh_manager_core::audit::LedgerEntryContent::Connection { .. }
            )
        })
        .collect::<Vec<_>>();
    for e in &entries {
        let raw = serde_json::to_string(&e.content).unwrap();
        assert!(!raw.contains("hunter2geheim"), "{raw}");
    }
    assert!(entries.iter().any(|e| matches!(
        &e.content,
        LedgerEntryContent::CommandExecuted { output, .. }
            if String::from_utf8_lossy(&output.stdout).contains("REDACTED")
    )));
    // No handshake name: stored as `unknown`.
    let recorded = fx
        .profiles
        .activity_session_store()
        .load(fx.mcp_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recorded.mcp_client.as_deref(), Some("unknown"));
}

#[tokio::test]
async fn test_ending_the_mcp_session_sets_ended_at() {
    let fx = mcp_fixture(MockSshTransport::default(), Some("c")).await;
    let store = fx.profiles.activity_session_store();
    assert!(store
        .load(fx.mcp_id)
        .await
        .unwrap()
        .unwrap()
        .ended_at
        .is_none());
    crate::mcp_sessions::end_recorded_session(&fx.session).await;
    assert!(store
        .load(fx.mcp_id)
        .await
        .unwrap()
        .unwrap()
        .ended_at
        .is_some());
}

#[tokio::test]
async fn test_without_recorded_session_an_mcp_action_writes_no_ledger_entry() {
    // A session that never got a recorded session (e.g. creation failed)
    // still runs the action; nothing is written and nothing panics.
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default(),
    );
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyEverything)));
    propose_mcp(&session, "ls", &TestEmitter::default()).await;
}

struct DenyEverything;
#[async_trait]
impl PolicyStore for DenyEverything {
    async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
        PolicyRules::complete(vec![Rule {
            id: ssh_manager_core::filter::RuleId("deny-all".to_string()),
            pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
            action: ssh_manager_core::filter::RuleAction::Deny,
            scope: ssh_manager_core::filter::Scope::Global,
            priority: 0,
            origin: ssh_manager_core::filter::RuleOrigin::User,
        }])
    }
}
