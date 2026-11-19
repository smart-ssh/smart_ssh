//! Issue #1356, Spec 0011: batch save of staged quick rules.

use persistence_sqlite::SqlitePolicyStore;
use ssh_manager_core::filter::{RuleAction, RuleId};

use crate::dto::RuleInput;
use crate::filter_rules::RuleWriteError;

/// Issue #1356: saves several staged rules, each with its own patterns and
/// scope, in ONE transaction: all or nothing. Any invalid pattern (empty,
/// does not compile, Spec 0077) rejects the whole batch before anything is
/// written. Does not touch a pending confirmation.
pub async fn create_quick_rules_batch(
    policy_store: &SqlitePolicyStore,
    rules: Vec<crate::dto::StagedRuleDto>,
) -> Result<Vec<RuleId>, RuleWriteError> {
    let now = chrono::Utc::now();
    let mut stored = Vec::new();
    for staged in rules {
        // A staged rule without any pattern is rejected like an empty pattern.
        if staged.patterns.is_empty() {
            return Err(RuleWriteError::InvalidPattern(
                ssh_manager_core::filter::PatternError {
                    message: "staged rule without patterns".to_string(),
                },
            ));
        }
        for p in staged.patterns {
            // A staged rule never carries an empty pattern (stricter than
            // `Pattern::validate`, which accepts one).
            if p.pattern_value.trim().is_empty() {
                return Err(RuleWriteError::InvalidPattern(
                    ssh_manager_core::filter::PatternError {
                        message: "empty pattern".to_string(),
                    },
                ));
            }
            stored.push(
                RuleInput {
                    pattern_type: p.pattern_type,
                    pattern_value: p.pattern_value,
                    action: RuleAction::Allow,
                    scope: staged.scope.clone(),
                    priority: 0,
                }
                .into_stored_rule(
                    RuleId(uuid::Uuid::new_v4().to_string()),
                    now,
                    now,
                ),
            );
        }
    }
    for rule in &stored {
        rule.pattern.validate()?;
    }
    policy_store.create_many(&stored).await?;
    Ok(stored.into_iter().map(|r| r.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::PatternType;
    use ssh_manager_core::filter::Scope;

    async fn new_store() -> (tempfile::TempDir, SqlitePolicyStore) {
        let dir = tempfile::tempdir().unwrap();
        let store =
            persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
                .await
                .unwrap()
                .policy_store();
        (dir, store)
    }

    fn staged(value: &str, scope: Scope) -> crate::dto::StagedRuleDto {
        crate::dto::StagedRuleDto {
            patterns: vec![crate::dto::SuggestionPatternDto {
                pattern_type: PatternType::Glob,
                pattern_value: value.to_string(),
            }],
            scope,
        }
    }

    #[tokio::test]
    async fn batch_stores_rules_with_different_scopes() {
        let (_dir, store) = new_store().await;
        let server = Scope::Server(ssh_manager_core::shared::ServerId::new());
        let ids = create_quick_rules_batch(
            &store,
            vec![
                staged("ls *", Scope::Global),
                staged("git *", server.clone()),
                staged("cat *", Scope::Tag("prod".into())),
            ],
        )
        .await
        .unwrap();
        assert_eq!(ids.len(), 3);
        let all = store.list_all().await.unwrap();
        assert_eq!(all.len(), 3);
        assert!(all.iter().any(|r| r.scope == server));
        assert!(all.iter().any(|r| r.scope == Scope::Tag("prod".into())));
    }

    #[tokio::test]
    async fn batch_with_one_invalid_rule_stores_nothing() {
        for bad in ["ls [abc *", ""] {
            let (_dir, store) = new_store().await;
            let err = create_quick_rules_batch(
                &store,
                vec![staged("ls *", Scope::Global), staged(bad, Scope::Global)],
            )
            .await
            .expect_err("invalid pattern");
            assert!(matches!(err, RuleWriteError::InvalidPattern(_)), "{err:?}");
            assert!(store.list_all().await.unwrap().is_empty());
        }
        // A staged rule with an empty patterns list is rejected too.
        let (_dir, store) = new_store().await;
        let mut empty = staged("x", Scope::Global);
        empty.patterns.clear();
        let err = create_quick_rules_batch(&store, vec![staged("ls *", Scope::Global), empty])
            .await
            .expect_err("no patterns");
        assert!(matches!(err, RuleWriteError::InvalidPattern(_)), "{err:?}");
        assert!(store.list_all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn batch_leaves_pending_confirmation_open() {
        use crate::confirmation::ConfirmationRegistry;
        use crate::dto::ActionUserDecision;
        let (_dir, store) = new_store().await;
        let registry: ConfirmationRegistry<uuid::Uuid, ActionUserDecision> =
            ConfirmationRegistry::new();
        let id = uuid::Uuid::new_v4();
        let rx = registry.register(id);
        create_quick_rules_batch(&store, vec![staged("ls *", Scope::Global)])
            .await
            .unwrap();
        assert!(registry.contains(&id));
        registry.resolve(&id, ActionUserDecision::Approve).unwrap();
        assert!(matches!(rx.await.unwrap(), ActionUserDecision::Approve));
    }
}
