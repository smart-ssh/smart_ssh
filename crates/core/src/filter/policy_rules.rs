use super::types::{Rule, RuleAction};

/// The rules a [`PolicyStore`] returns for a scope, plus whether the set is
/// complete. When `unavailable` is set, the rules could not be (fully)
/// loaded: the engine then never auto-executes (fail closed), because a
/// missing `Allow` rule is harmless but a missing `Deny` rule is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolicyRules {
    pub rules: Vec<Rule>,
    pub unavailable: bool,
}

impl PolicyRules {
    /// A complete rule set (nothing failed to load).
    pub fn complete(rules: Vec<Rule>) -> Self {
        Self {
            rules,
            unavailable: false,
        }
    }

    /// The rule set could not be loaded completely; `rules` holds what is
    /// still known (callers pass no `Allow` rules here).
    pub fn unavailable(rules: Vec<Rule>) -> Self {
        Self {
            rules,
            unavailable: true,
        }
    }

    /// Rules to use for display or prompt building: no `Allow` rule when the
    /// set is unavailable.
    pub fn into_trusted_rules(self) -> Vec<Rule> {
        if self.unavailable {
            self.rules
                .into_iter()
                .filter(|r| r.action != RuleAction::Allow)
                .collect()
        } else {
            self.rules
        }
    }
}

impl From<Vec<Rule>> for PolicyRules {
    fn from(rules: Vec<Rule>) -> Self {
        Self::complete(rules)
    }
}
