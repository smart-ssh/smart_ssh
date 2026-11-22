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
        .unwrap();
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
        .unwrap();
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

/// The confirmation wait itself needs a 5-minute clock, which cannot be
/// paused while a real database is open (pool timeouts use the clock). The
/// timeout's ledger fields are therefore checked on the record that
/// `handle_user_decision` writes for `RejectionReason::Timeout`.
#[test]
fn test_mcp_timeout_decision_is_attributed_to_the_mcp_agent_with_timeout_code() {
    let record = super::decision::DecisionRecord::new(
        &RejectionReason::Timeout,
        &ActionOrigin::Mcp {
            client_name: Some("Claude Code".to_string()),
        },
        "confirm".to_string(),
        "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM".to_string(),
        None,
        None,
    );
    assert_eq!(record.source, LedgerSource::McpAgent);
    assert_eq!(record.code.as_deref(), Some("TIMEOUT"));
    // A human's denial keeps the user as the source.
    let user = super::decision::DecisionRecord::new(
        &RejectionReason::User,
        &ActionOrigin::Mcp { client_name: None },
        "confirm".to_string(),
        "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM".to_string(),
        None,
        None,
    );
    assert_eq!(user.source, LedgerSource::User);
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
        .unwrap();
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
