//! [`PolicyStore`]-Implementierung für "es sind (in diesem Kontext) keine
//! Nutzerregeln zu berücksichtigen".
//!
//! Ursprünglich (Spec 0007) die einzige verfügbare `PolicyStore`-
//! Implementierung überhaupt, mangels einer Regel-Verwaltungs-UI — seit
//! Spec 0009 (`persistence_sqlite::SqlitePolicyStore`, s.
//! `crate::state::AppState::policy_store`) übernimmt die echte
//! Verbindungen (`crate::commands::connect`). Deshalb jetzt `#[cfg(test)]`
//! (s. `crate::lib`): `NoRulesPolicyStore` lebt nur noch als expliziter,
//! klar benannter Testdouble überall dort (v. a. `crate::orchestration`-
//! Tests), wo ein `PolicyStore` gebraucht wird, dessen konkretes Verhalten
//! für den jeweiligen Test irrelevant ist — [`FilterEngine::evaluate`]
//! fällt dann auf ihre eingebauten Defaults zurück (Hard-Blacklist bleibt
//! aktiv, alles andere landet auf `Confirm` statt `AutoExec`, s. Spec 0002
//! Abschnitt 3).

use async_trait::async_trait;

use ssh_manager_core::filter::{EffectiveScope, PolicyStore, Rule};

pub struct NoRulesPolicyStore;

#[async_trait]
impl PolicyStore for NoRulesPolicyStore {
    async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        Vec::new()
    }
}
