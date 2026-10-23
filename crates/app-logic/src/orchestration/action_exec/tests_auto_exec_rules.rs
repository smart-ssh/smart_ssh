//! Issue #696: the `chat-action-proposed` event of an auto-executed command
//! names the rules behind it (display only); other decisions carry none.

use async_trait::async_trait;
use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::{
    EffectiveScope, FilterEngine, Pattern, PolicyStore, Rule, RuleAction, Scope,
};

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

struct Rules(Vec<Rule>);
#[async_trait]
impl PolicyStore for Rules {
    async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        self.0.clone()
    }
}

fn allow(id: &str, glob: &str, scope: Scope) -> Rule {
    Rule {
        id: RuleId(id.to_string()),
        pattern: Pattern::Glob(glob.to_string()),
        action: RuleAction::Allow,
        scope,
        priority: 0,
        origin: RuleOrigin::User,
    }
}

async fn proposed_payload(session: &Session, command: &str) -> serde_json::Value {
    let emitter = TestEmitter::default();
    let (_, payload) = proposed_decision_code_with_emitter(
        session,
        AiAction::SuggestCommand {
            command: command.to_string(),
        },
        ActionOrigin::Internal,
        &emitter,
    )
    .await;
    payload
}

fn session_with(rules: impl FnOnce(ssh_manager_core::shared::ServerId) -> Vec<Rule>) -> Session {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let rules = rules(session.server_id);
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(Rules(rules))));
    session
}

#[tokio::test]
async fn auto_exec_event_carries_the_rules_of_a_chain_and_a_server_rule() {
    let session = session_with(|id| {
        vec![
            allow("cd-rule", "cd *", Scope::Global),
            allow("ls-rule", "ls *", Scope::Global),
            allow("up-rule", "uptime", Scope::Server(id)),
        ]
    });
    let payload = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_payload(&session, "cd /tmp && ls /x"),
    )
    .await
    .unwrap();
    assert_eq!(payload["decision"], "AutoExec", "{payload}");
    let ids: Vec<&str> = payload["autoExecRules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["ruleId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["cd-rule", "ls-rule"]);
    assert_eq!(payload["autoExecRules"][0]["patternValue"], "cd *");
    assert_eq!(payload["autoExecRules"][0]["patternType"], "glob");
    assert_eq!(payload["autoExecRules"][1]["segmentIndices"][0], 1);

    let payload = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_payload(&session, "uptime"),
    )
    .await
    .unwrap();
    assert_eq!(payload["decision"], "AutoExec", "{payload}");
    assert_eq!(payload["autoExecRules"][0]["ruleId"], "up-rule");
    assert!(payload["autoExecRules"][0]["scope"]["Server"].is_string());
}

#[tokio::test]
async fn confirmed_command_carries_no_rules() {
    let session = session_with(|_| vec![allow("cd-rule", "cd *", Scope::Global)]);
    let payload = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_payload(&session, "ls /x"),
    )
    .await
    .unwrap();
    assert!(payload["decision"]["Confirm"].is_object(), "{payload}");
    assert_eq!(payload["autoExecRules"].as_array().unwrap().len(), 0);
}
