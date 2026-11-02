//! Issue #836: an unavailable rule set fails closed.

use super::*;

struct FailingPolicySource;

#[async_trait]
impl PolicySource for FailingPolicySource {
    fn origin(&self) -> RuleOrigin {
        RuleOrigin::Organization
    }

    async fn rules(&self) -> PolicySourceResult<Vec<Rule>> {
        Err(PolicySourceError("boom".to_string()))
    }

    async fn watch(&self) -> PolicySourceResult<tokio::sync::watch::Receiver<Vec<Rule>>> {
        Err(PolicySourceError("boom".to_string()))
    }
}

/// A store that reports its rule set as unavailable.
struct UnavailableStore(Vec<Rule>);

#[async_trait]
impl PolicyStore for UnavailableStore {
    async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
        PolicyRules::unavailable(self.0.clone())
    }
}

fn combined_with_failing(healthy_rules: Vec<Rule>) -> FilterEngine<CombinedPolicySource> {
    FilterEngine::new(CombinedPolicySource::new(vec![
        Arc::new(FailingPolicySource),
        Arc::new(InMemoryPolicySource {
            origin: RuleOrigin::User,
            rules: healthy_rules,
        }),
    ]))
}

fn allow_ls() -> Rule {
    glob_rule("allow-ls", "ls*", RuleAction::Allow, Scope::Global, 0)
}

#[tokio::test]
async fn test_unavailable_store_never_auto_executes() {
    let engine = FilterEngine::new(UnavailableStore(vec![allow_ls()]));
    let trace = engine.evaluate_explained("ls -la", &ctx("s", &[])).await;
    match &trace.decision {
        Decision::Confirm { code, .. } => assert_eq!(code, "FILTER_RULES_UNAVAILABLE"),
        other => panic!("expected Confirm, got {other:?}"),
    }
    assert!(trace.checks.iter().any(|c| c == "FILTER_RULES_UNAVAILABLE"));
}

#[tokio::test]
async fn test_unavailable_store_ignores_allow_candidates() {
    let engine = FilterEngine::new(UnavailableStore(vec![]));
    let trace = engine
        .evaluate_explained_with_candidates("ls -la", &ctx("s", &[]), &[allow_ls()])
        .await;
    assert!(
        matches!(trace.decision, Decision::Confirm { ref code, .. } if code == "FILTER_RULES_UNAVAILABLE")
    );
}

#[tokio::test]
async fn test_combined_failing_source_drops_allow_of_working_source() {
    let engine = combined_with_failing(vec![allow_ls()]);
    let decision = engine.evaluate("ls -la", &ctx("s", &[])).await;
    assert!(
        matches!(decision, Decision::Confirm { ref code, .. } if code == "FILTER_RULES_UNAVAILABLE"),
        "got {decision:?}"
    );
}

#[tokio::test]
async fn test_combined_failing_source_keeps_deny_of_working_source() {
    let deny = glob_rule("deny-ls", "ls*", RuleAction::Deny, Scope::Global, 0);
    let engine = combined_with_failing(vec![allow_ls(), deny]);
    let decision = engine.evaluate("ls -la", &ctx("s", &[])).await;
    assert!(
        matches!(decision, Decision::Deny { .. }),
        "got {decision:?}"
    );
}

#[tokio::test]
async fn test_unavailable_rules_keep_hard_blacklist_decision() {
    let loaded = engine(vec![]).evaluate("rm -rf /", &ctx("s", &[])).await;
    let unavailable = FilterEngine::new(UnavailableStore(vec![]))
        .evaluate("rm -rf /", &ctx("s", &[]))
        .await;
    assert!(
        matches!(loaded, Decision::Confirm { ref code, .. } if code == FILTER_HARD_BLACKLIST),
        "got {loaded:?}"
    );
    // Same kind and code; only the reason gains the "rules unavailable" note.
    assert!(
        matches!(unavailable, Decision::Confirm { ref code, .. } if code == FILTER_HARD_BLACKLIST),
        "got {unavailable:?}"
    );
}

#[tokio::test]
async fn test_combined_healthy_sources_auto_exec_counter_check() {
    let engine = FilterEngine::new(CombinedPolicySource::new(vec![
        Arc::new(InMemoryPolicySource {
            origin: RuleOrigin::Organization,
            rules: vec![],
        }),
        Arc::new(InMemoryPolicySource {
            origin: RuleOrigin::User,
            rules: vec![allow_ls()],
        }),
    ]));
    assert_eq!(
        engine.evaluate("ls -la", &ctx("s", &[])).await,
        Decision::AutoExec
    );
}
