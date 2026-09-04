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

impl Feature {
    /// Trennt lokale von Dienst-gebundenen Features (Spec 0042, Abschnitt
    /// 3.5) — Metadaten fürs Lizenzsystem des privaten Repos, das im
    /// versions-basierten Perpetual-Fallback (abgelaufenes Abo, App-Version
    /// noch in der zuletzt bezahlten Minor-Reihe) nur die **lokalen**
    /// Pro-Features aktiv lässt. `core` selbst trifft keine
    /// Lizenzentscheidung (s. Moduldoc) — diese Methode liefert nur die
    /// Einordnung, auf der das private `licensing`-Modul aufbaut.
    ///
    /// **Kriterium**: "Dienst-gebunden" heißt, ein von uns selbst
    /// betriebener laufender Dienst steht dahinter, der ohne aktives Abo
    /// nicht mehr funktionieren darf/kann — nicht dasselbe wie
    /// Team-/Enterprise-Tier-Zugehörigkeit. `OrgPolicy` (Policy-as-Code aus
    /// dem Git des Kunden) und `AuditExport` (lokaler signierter Export)
    /// sind z. B. Team-Tier, aber lokal: kein Dienst von uns, der bei
    /// Ablauf abgeschaltet werden müsste.
    ///
    /// `ManagedAi` (unser KI-Proxy) und `CloudSync` (unser gehosteter
    /// Sync-Dienst) sind die einzigen aktuell dienst-gebundenen Features.
    /// Alle anderen bleiben im Fallback aktiv.
    pub fn is_service_bound(&self) -> bool {
        match self {
            Feature::ManagedAi | Feature::CloudSync => true,
            Feature::SharedInventory
            | Feature::SharedNotes
            | Feature::CuratedRulePacks
            | Feature::OrgPolicy
            | Feature::MultiServerActions
            | Feature::OrgAiPolicy
            | Feature::TeamAgents
            | Feature::SessionHistory
            | Feature::ActivityReport
            | Feature::SessionHandover
            | Feature::DocumentExport
            | Feature::AuditExport
            | Feature::Sso
            | Feature::SelfHosted => false,
        }
    }
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
/// damit `app-shell` (bzw. künftig ein Lizenzschlüssel-/Managed-Backend im
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

    /// Spec 0042, Abschnitt 3.5: ein Assert pro Variante statt einer
    /// Schleife über ein Array — das exhaustive `match` in
    /// `is_service_bound` selbst sorgt bereits dafür, dass eine künftig
    /// hinzugefügte `Feature`-Variante ohne Klassifizierung nicht
    /// kompiliert; dieser Test sichert zusätzlich ab, dass jede *bestehende*
    /// Variante tatsächlich den beabsichtigten Wert liefert (ein
    /// Kopier-/Vertauschungsfehler im `match` selbst würde sonst nicht
    /// auffallen).
    #[test]
    fn test_is_service_bound_classifies_every_feature() {
        assert!(Feature::ManagedAi.is_service_bound());
        assert!(Feature::CloudSync.is_service_bound());

        assert!(!Feature::SharedInventory.is_service_bound());
        assert!(!Feature::SharedNotes.is_service_bound());
        assert!(!Feature::CuratedRulePacks.is_service_bound());
        assert!(!Feature::OrgPolicy.is_service_bound());
        assert!(!Feature::MultiServerActions.is_service_bound());
        assert!(!Feature::OrgAiPolicy.is_service_bound());
        assert!(!Feature::TeamAgents.is_service_bound());
        assert!(!Feature::SessionHistory.is_service_bound());
        assert!(!Feature::ActivityReport.is_service_bound());
        assert!(!Feature::SessionHandover.is_service_bound());
        assert!(!Feature::DocumentExport.is_service_bound());
        assert!(!Feature::AuditExport.is_service_bound());
        assert!(!Feature::Sso.is_service_bound());
        assert!(!Feature::SelfHosted.is_service_bound());
    }

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
