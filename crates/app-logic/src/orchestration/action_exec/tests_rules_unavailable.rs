//! Issue #836: ledger entry for a decision made while the filter rules are unavailable.

use async_trait::async_trait;
use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::PolicyRules;
use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyStore, Rule};

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

/// Issue #836: a decision made while the filter rules are unavailable
/// carries `FILTER_RULES_UNAVAILABLE` in the ledger, even though an allow
/// rule for the command exists.
#[tokio::test]
async fn test_ledger_records_rules_unavailable_code() {
    struct UnavailableWithAllow;
    #[async_trait]
    impl PolicyStore for UnavailableWithAllow {
        async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
            PolicyRules::unavailable(vec![Rule {
                id: ssh_manager_core::filter::RuleId("allow-ls".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("ls*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Allow,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }])
        }
    }

    let (mut session, _chat_store, chat_session_id, _tmp_dir, ledger_store) =
        session_with_real_chat_and_ledger_persistence(
            vec![AiEvent::Done],
            MockSshTransport::default(),
        )
        .await;
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(UnavailableWithAllow)));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let action_future = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    let responder = deny_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    let entries = ledger_store.load_entries(chat_session_id).await.unwrap();
    assert!(
        matches!(
            &entries[1].content,
            LedgerEntryContent::Decision {
                code: Some(code),
                ..
            } if code == "FILTER_RULES_UNAVAILABLE"
        ),
        "{entries:?}"
    );
}
