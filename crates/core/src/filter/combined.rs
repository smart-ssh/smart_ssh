use super::engine::{scope_applies, PolicySource, PolicyStore};
use super::policy_rules::PolicyRules;
use super::types::{EffectiveScope, RuleAction};
use async_trait::async_trait;

/// Kombiniert mehrere [`PolicySource`]s zu einem [`PolicyStore`] (Spec
/// 0037, Abschnitt 5) — die Grundlage dafür, dass `FilterEngine` (die
/// weiterhin nur einen einzelnen `PolicyStore` kennt) mehrere
/// unterschiedliche Regel-Ebenen gleichzeitig auswerten kann, sobald mehr
/// als eine `PolicySource` existiert. In der Community Edition ist die
/// Liste immer nur die eine SQLite-`User`-Quelle (s. Spec-Text) — dieselbe
/// Filter-Engine-Logik, keine Sonderbehandlung für den Ein-Quellen-Fall.
///
/// Fällt eine Quelle aus, wird das geloggt und das Ergebnis als
/// `unavailable` markiert: es enthält dann die `Deny`-/`Confirm`-Regeln der
/// funktionierenden Quellen, aber **keine** `Allow`-Regel einer Quelle
/// (fail closed, die Engine führt dann nichts automatisch aus).
pub struct CombinedPolicySource {
    sources: Vec<std::sync::Arc<dyn PolicySource>>,
}

impl CombinedPolicySource {
    pub fn new(sources: Vec<std::sync::Arc<dyn PolicySource>>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl PolicyStore for CombinedPolicySource {
    async fn rules_for(&self, scope: &EffectiveScope) -> PolicyRules {
        let mut all = Vec::new();
        let mut unavailable = false;
        for source in &self.sources {
            match source.rules().await {
                Ok(rules) => {
                    all.extend(rules.into_iter().filter(|r| scope_applies(&r.scope, scope)))
                }
                Err(err) => {
                    unavailable = true;
                    tracing::error!(origin = ?source.origin(), error = %err, "policy source unavailable, rule set incomplete");
                }
            }
        }
        if unavailable {
            // Fail closed: keep Deny/Confirm rules of the working sources,
            // but no Allow rule from any source.
            all.retain(|r| r.action != RuleAction::Allow);
            PolicyRules::unavailable(all)
        } else {
            PolicyRules::complete(all)
        }
    }
}
