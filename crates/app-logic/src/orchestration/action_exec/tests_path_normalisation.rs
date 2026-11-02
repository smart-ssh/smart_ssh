//! Issue #840: filter and SFTP execution see the same normalised path.

use async_trait::async_trait;
use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyStore, Rule};
use ssh_manager_core::ssh::mock::MockSftpSession;
use uuid::Uuid;

use crate::events::TestEmitter;
use crate::orchestration::run_chat_turn;

use super::super::test_support::*;
use super::*;

/// Issue #840: the path the filter engine matches and the path handed to
/// SFTP are the same normalised string. An exact allow rule on the normalised
/// path only matches (AutoExec) when the filter saw it, and the SFTP double
/// only records that same path.
#[tokio::test]
async fn test_read_remote_file_filter_and_sftp_see_same_normalised_path() {
    struct AllowExactPath;
    #[async_trait]
    impl PolicyStore for AllowExactPath {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("allow-exact".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob(
                    "sftp-read /etc/nginx/secret.conf".to_string(),
                ),
                action: ssh_manager_core::filter::RuleAction::Allow,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::ReadRemoteFile {
                path: "/etc/nginx/../nginx//./secret.conf".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowExactPath)));
    let mock_sftp = MockSftpSession::new().with_file("/etc/nginx/secret.conf", b"x=1".to_vec());
    session
        .set_sftp_for_tests(Box::new(mock_sftp.clone()))
        .await;

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    assert!(
        events[0].1["decision"] == "AutoExec",
        "filter must have matched the normalised path: {}",
        events[0].1["decision"]
    );
    assert_eq!(
        mock_sftp.calls(),
        vec![
            "stat /etc/nginx/secret.conf",
            "read_file /etc/nginx/secret.conf"
        ]
    );
}
