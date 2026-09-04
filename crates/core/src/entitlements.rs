//! Vokabular für Bezahlfunktionen (Spec 0037, Abschnitt 2) — bewusst ohne
//! die eigentlichen Bezahlmodul-Implementierungen (folgen später in einem
//! privaten Repo, Spec 0038). `ssh-manager-core` kennt hier nur die Typen,
//! keine Entitlement-**Logik** im Sinne von "was darf ein bestimmter
//! Nutzer" (D5) — insbesondere entscheidet dieses Modul nie, WELCHE
//! `Entitlements` gelten, das ist Sache der jeweiligen
//! `EntitlementProvider`-Implementierung (z. B. [`FixedEntitlements`] für
//! die Community Edition).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Ein einzelnes, gatebares Feature (Spec 0037, Abschnitt 2 — Architektur-
/// Brief Abschnitt 2, Feature-Matrix). **Kein `CertificateAuth`-Feature**:
/// Zertifikats-Auth ist bereits veröffentlicht und bleibt endgültig Free
/// (D4), kein Gating dafür.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    SharedInventory,
    SharedNotes,
    CuratedRulePacks,
    OrgPolicy,
    MultiServerActions,
    ManagedAi,
    OrgAiPolicy,
    TeamAgents,
    SessionHistory,
    ActivityReport,
    SessionHandover,
    CloudSync,
    DocumentExport,
    AuditExport,
    Sso,
    SelfHosted,
}

/// Lizenzstufe (Spec 0037, Abschnitt 2 — Architektur-Brief Abschnitt 1,
/// D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Free,
    Personal,
    Pro,
    Team,
    Business,
    Enterprise,
}

/// Der für einen Nutzer/eine Installation aktuell geltende Entitlement-
/// Stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entitlements {
    pub tier: Tier,
    pub features: HashSet<Feature>,
    pub seats: Option<u32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub non_commercial: bool,
    pub licensee: Option<String>,
}

impl Entitlements {
    /// Der Zustand der (aktuell einzigen existierenden) Community Edition
    /// — `Tier::Free`, keine Features freigeschaltet.
    pub fn free() -> Self {
        Self {
            tier: Tier::Free,
            features: HashSet::new(),
            seats: None,
            expires_at: None,
            non_commercial: false,
            licensee: None,
        }
    }

    pub fn has(&self, f: Feature) -> bool {
        self.features.contains(&f)
    }

    /// Spec 0037, Abschnitt 3 (D5): Gating-Konvention — jeder Tauri-Command,
    /// der ein gegatetes Feature auslöst, ruft dies als erste Anweisung auf.
    /// Gesperrte Features schlagen **geschlossen** fehl (`Err`), nie eine
    /// stille Degradierung auf ein "Free-Verhalten" als Fallback.
    pub fn require(&self, f: Feature) -> Result<(), FeatureLocked> {
        if self.has(f) {
            Ok(())
        } else {
            Err(FeatureLocked {
                feature: f,
                tier: self.tier,
            })
        }
    }
}

/// Fehler aus [`Entitlements::require`] — wird in den jeweiligen App-Fehler-
/// typ eingebettet (nicht als String transportiert), damit das Frontend ihn
/// eindeutig von fachlichen Fehlern unterscheiden kann (Spec 0037,
/// Abschnitt 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("feature {feature:?} is not available in tier {tier:?}")]
pub struct FeatureLocked {
    pub feature: Feature,
    pub tier: Tier,
}

/// Liefert den aktuell geltenden Entitlement-Stand — als Trait modelliert,
/// damit `app-tauri` (bzw. künftig ein Lizenzschlüssel-/Managed-Backend im
/// privaten Repo) eine eigene Implementierung einsetzen kann, ohne dass
/// `core` von einem konkreten Lizenzmechanismus abhängen müsste (analog zum
/// `ProfileStore`/`PolicyStore`-Muster).
pub trait EntitlementProvider: Send + Sync {
    fn current(&self) -> Entitlements;
    fn watch(&self) -> tokio::sync::watch::Receiver<Entitlements>;
}

/// Für die (aktuell einzige existierende) Community Edition und für Tests:
/// ein fest verdrahteter, sich nie ändernder Entitlement-Stand — kein
/// Lizenzschlüssel-Mechanismus in diesem Schritt (Spec 0037, Abschnitt 2).
pub struct FixedEntitlements(pub Entitlements);

impl EntitlementProvider for FixedEntitlements {
    fn current(&self) -> Entitlements {
        self.0.clone()
    }

    fn watch(&self) -> tokio::sync::watch::Receiver<Entitlements> {
        // Ein `FixedEntitlements`-Stand ändert sich per Definition nie —
        // der Receiver liefert also nie ein Update, nur den initialen
        // Wert. Der zugehörige `Sender` wird bewusst nicht gehalten
        // (`let (tx, rx) = ...; std::mem::forget(tx)` wäre ein Leak-Risiko
        // ohne Zweck) — ein `watch::channel`, dessen `Sender` sofort
        // gedroppt wird, liefert seinen Empfängern weiterhin klaglos den
        // zuletzt gesendeten (hier: initialen) Wert; nur ein erneutes
        // `.changed().await` würde `RecvError` liefern, was für einen
        // Stand, der sich nie ändert, korrekt ist.
        let (_tx, rx) = tokio::sync::watch::channel(self.0.clone());
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_entitlements_has_free_tier_and_empty_feature_set() {
        let entitlements = Entitlements::free();

        assert_eq!(entitlements.tier, Tier::Free);
        assert!(entitlements.features.is_empty());
        assert_eq!(entitlements.seats, None);
        assert_eq!(entitlements.expires_at, None);
        assert!(!entitlements.non_commercial);
        assert_eq!(entitlements.licensee, None);
    }

    #[test]
    fn test_require_ok_when_feature_is_present() {
        let mut entitlements = Entitlements::free();
        entitlements.features.insert(Feature::DocumentExport);

        assert!(entitlements.require(Feature::DocumentExport).is_ok());
    }

    #[test]
    fn test_require_returns_feature_locked_with_feature_and_tier_when_absent() {
        let entitlements = Entitlements::free();

        let err = entitlements
            .require(Feature::DocumentExport)
            .expect_err("DocumentExport ist in Entitlements::free() nicht enthalten");

        assert_eq!(err.feature, Feature::DocumentExport);
        assert_eq!(err.tier, Tier::Free);
    }

    #[test]
    fn test_fixed_entitlements_current_returns_configured_state() {
        let mut free = Entitlements::free();
        free.features.insert(Feature::SessionHistory);
        let provider = FixedEntitlements(free.clone());

        assert_eq!(provider.current(), free);
    }

    #[test]
    fn test_fixed_entitlements_watch_receiver_yields_the_initial_value() {
        let entitlements = Entitlements::free();
        let provider = FixedEntitlements(entitlements.clone());

        let rx = provider.watch();
        assert_eq!(*rx.borrow(), entitlements);
    }
}
