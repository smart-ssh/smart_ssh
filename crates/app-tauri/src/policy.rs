//! [`PolicyStore`]-Implementierung für Teil 2 dieser Spec.
//!
//! Es existiert noch keine Regel-Verwaltungs-UI (weder in Spec 0007 Teil 1
//! noch Teil 2 vorgesehen — das ist laut Spec 0007 Abschnitt 1 Teil der
//! separaten "Ausbaustufe 2"-Folge-Spec). `NoRulesPolicyStore` ist deshalb
//! keine Attrappe, sondern die schlicht korrekte Implementierung für "es
//! sind noch keine Nutzerregeln konfiguriert": [`FilterEngine::evaluate`]
//! fällt dann auf ihre eingebauten Defaults zurück (Hard-Blacklist bleibt
//! aktiv, alles andere landet auf `Confirm` statt `AutoExec`, s. Spec 0002
//! Abschnitt 3) — sicheres Verhalten ganz ohne Regel-Pflege.

use ssh_manager_core::filter::{EffectiveScope, PolicyStore, Rule};

pub struct NoRulesPolicyStore;

impl PolicyStore for NoRulesPolicyStore {
    fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        Vec::new()
    }
}
