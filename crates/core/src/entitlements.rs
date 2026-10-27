//! Vokabular für Bezahlfunktionen (Spec 0037, Abschnitt 2) — bewusst ohne
//! die eigentlichen Bezahlmodul-Implementierungen (folgen später in einem
//! privaten Repo, Spec 0038). `ssh-manager-core` kennt hier nur die Typen,
//! keine Entitlement-**Logik** im Sinne von "was darf ein bestimmter
//! Nutzer" (D5) — insbesondere entscheidet dieses Modul nie, WELCHE
//! `Entitlements` gelten, das ist Sache der jeweiligen
//! `EntitlementProvider`-Implementierung (z. B. [`FixedEntitlements`] für
//! die Community Edition).

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

/// Maximum length of a feature id.
const MAX_FEATURE_ID_LEN: usize = 64;

/// Whether `id` matches `^[a-z][a-z0-9_]{0,63}$`.
fn is_valid_feature_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_FEATURE_ID_LEN
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// An opaque, gatable feature identifier (Spec 0037, section 2). The core
/// declares no product features; editions and integrations declare their own
/// constants with [`Feature::new`] and register them through
/// `Wiring::entitlement_features`. Serialises as the plain id string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Feature(Cow<'static, str>);

impl Feature {
    /// Declares a feature id in `const` context. The id should match
    /// `^[a-z][a-z0-9_]{0,63}$`; an id outside that pattern can be neither
    /// deserialised nor sent to the frontend.
    pub const fn new(id: &'static str) -> Feature {
        Feature(Cow::Borrowed(id))
    }

    pub fn id(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Feature {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        if is_valid_feature_id(&id) {
            Ok(Feature(Cow::Owned(id)))
        } else {
            Err(serde::de::Error::custom("invalid feature id"))
        }
    }
}

/// Transition constants for the ids extensions use today. Editions declare
/// their own constants; these go away in a later release.
#[allow(non_upper_case_globals)]
impl Feature {
    #[deprecated(note = "editions declare their own feature constants; this goes away")]
    pub const DocumentExport: Feature = Feature::new("document_export");
    #[deprecated(note = "editions declare their own feature constants; this goes away")]
    pub const SessionHistory: Feature = Feature::new("session_history");
    #[deprecated(note = "editions declare their own feature constants; this goes away")]
    pub const ServerEnvironment: Feature = Feature::new("server_environment");
    #[deprecated(note = "editions declare their own feature constants; this goes away")]
    pub const ChatAttachments: Feature = Feature::new("chat_attachments");
    #[deprecated(note = "editions declare their own feature constants; this goes away")]
    pub const SelfHostedSync: Feature = Feature::new("self_hosted_sync");
    #[deprecated(note = "editions declare their own feature constants; this goes away")]
    pub const ManagedAi: Feature = Feature::new("managed_ai");

    /// Transition only: true for the ids `managed_ai` and `cloud_sync`.
    /// Editions classify their own features; this goes away.
    #[deprecated(note = "editions classify their own features; this goes away")]
    pub fn is_service_bound(&self) -> bool {
        matches!(self.id(), "managed_ai" | "cloud_sync")
    }
}

/// Deserialises a feature set, skipping unknown or malformed entries.
fn lenient_feature_set<'de, D: Deserializer<'de>>(d: D) -> Result<HashSet<Feature>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry {
        Valid(Feature),
        Skipped(serde::de::IgnoredAny),
    }
    let raw = Vec::<Entry>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|e| match e {
            Entry::Valid(f) => Some(f),
            Entry::Skipped(_) => None,
        })
        .collect())
}

/// An opaque tier identifier (Spec 0037, section 2). The core knows only
/// [`TierId::FREE`]; editions name and label their own tiers in their
/// frontend registration. Any string is accepted (an unknown tier is shown
/// with a generic label), and it serialises as the plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TierId(Cow<'static, str>);

impl TierId {
    /// The free state: the only tier the core itself uses.
    pub const FREE: TierId = TierId(Cow::Borrowed("free"));

    /// Declares a tier id in `const` context.
    pub const fn new(id: &'static str) -> TierId {
        TierId(Cow::Borrowed(id))
    }

    pub fn id(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TierId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Der für einen Nutzer/eine Installation aktuell geltende Entitlement-
/// Stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entitlements {
    pub tier: TierId,
    #[serde(deserialize_with = "lenient_feature_set")]
    pub features: HashSet<Feature>,
    pub seats: Option<u32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub non_commercial: bool,
    pub licensee: Option<String>,
}

impl Entitlements {
    /// Der Zustand der (aktuell einzigen existierenden) Community Edition
    /// — `TierId::FREE`, keine Features freigeschaltet.
    pub fn free() -> Self {
        Self {
            tier: TierId::FREE,
            features: HashSet::new(),
            seats: None,
            expires_at: None,
            non_commercial: false,
            licensee: None,
        }
    }

    pub fn has(&self, f: &Feature) -> bool {
        self.features.contains(f)
    }

    /// Spec 0037, Abschnitt 3 (D5): Gating-Konvention — jeder Tauri-Command,
    /// der ein gegatetes Feature auslöst, ruft dies als erste Anweisung auf.
    /// Gesperrte Features schlagen **geschlossen** fehl (`Err`), nie eine
    /// stille Degradierung auf ein "Free-Verhalten" als Fallback.
    pub fn require(&self, f: &Feature) -> Result<(), FeatureLocked> {
        if self.has(f) {
            Ok(())
        } else {
            Err(FeatureLocked {
                feature: f.clone(),
                tier: self.tier.clone(),
            })
        }
    }
}

/// Fehler aus [`Entitlements::require`] — wird in den jeweiligen App-Fehler-
/// typ eingebettet (nicht als String transportiert), damit das Frontend ihn
/// eindeutig von fachlichen Fehlern unterscheiden kann (Spec 0037,
/// Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("feature {feature:?} is not available in tier {tier:?}")]
pub struct FeatureLocked {
    pub feature: Feature,
    pub tier: TierId,
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

impl Entitlements {
    /// Keeps only the features in `catalogue` (the edition's registered
    /// features); everything else is dropped.
    pub fn retain_catalogue(&mut self, catalogue: &[Feature]) {
        self.features.retain(|f| catalogue.contains(f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: Feature = Feature::new("document_export");
    const HISTORY: Feature = Feature::new("session_history");

    #[allow(deprecated)]
    #[test]
    fn test_deprecated_constants_and_is_service_bound_keep_today_values() {
        assert_eq!(Feature::DocumentExport.id(), "document_export");
        assert_eq!(Feature::SessionHistory.id(), "session_history");
        assert_eq!(Feature::ServerEnvironment.id(), "server_environment");
        assert_eq!(Feature::ChatAttachments.id(), "chat_attachments");
        assert_eq!(Feature::SelfHostedSync.id(), "self_hosted_sync");
        assert_eq!(Feature::ManagedAi.id(), "managed_ai");
        assert!(Feature::ManagedAi.is_service_bound());
        assert!(Feature::new("cloud_sync").is_service_bound());
        assert!(!Feature::DocumentExport.is_service_bound());
        assert!(!Feature::SessionHistory.is_service_bound());
        assert!(!Feature::ServerEnvironment.is_service_bound());
        assert!(!Feature::ChatAttachments.is_service_bound());
        assert!(!Feature::SelfHostedSync.is_service_bound());
    }

    #[test]
    fn test_wire_format_is_unchanged() {
        let mut e = Entitlements::free();
        e.tier = TierId::new("pro");
        e.features.insert(EXPORT);
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"tier":"pro","features":["document_export"],"seats":null,"expiresAt":null,"nonCommercial":false,"licensee":null}"#
        );
        let locked = Entitlements::free().require(&HISTORY).unwrap_err();
        assert_eq!(
            serde_json::to_string(&locked).unwrap(),
            r#"{"feature":"session_history","tier":"free"}"#
        );
        let back: Entitlements = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn test_unknown_and_malformed_features_are_skipped() {
        let json = r#"{"tier":"pro","features":["unknown_feature","Not-Valid","document_export"],"seats":null,"expiresAt":null,"nonCommercial":false,"licensee":null}"#;
        let e: Entitlements = serde_json::from_str(json).unwrap();
        let ids: HashSet<&str> = e.features.iter().map(|f| f.id()).collect();
        assert_eq!(ids, HashSet::from(["unknown_feature", "document_export"]));
    }

    #[test]
    fn test_invalid_single_feature_fails_to_deserialise() {
        for bad in ["\"Not-Valid\"", "\"\"", "\"1a\"", "\"_a\"", "5"] {
            assert!(serde_json::from_str::<Feature>(bad).is_err(), "{bad}");
        }
        let long = format!("\"a{}\"", "b".repeat(64));
        assert!(serde_json::from_str::<Feature>(&long).is_err());
        let ok = format!("\"a{}\"", "b".repeat(63));
        assert!(serde_json::from_str::<Feature>(&ok).is_ok());
    }

    #[test]
    fn test_retain_catalogue_drops_unregistered_features() {
        let mut e = Entitlements::free();
        e.features.insert(EXPORT);
        e.features.insert(HISTORY);
        e.retain_catalogue(std::slice::from_ref(&EXPORT));
        assert!(e.has(&EXPORT));
        assert!(!e.has(&HISTORY));
        e.retain_catalogue(&[]);
        assert!(e.features.is_empty());
    }

    #[test]
    fn test_display_is_the_id() {
        assert_eq!(EXPORT.to_string(), "document_export");
    }

    #[test]
    fn test_free_entitlements_has_free_tier_and_empty_feature_set() {
        let entitlements = Entitlements::free();

        assert_eq!(entitlements.tier, TierId::FREE);
        assert!(entitlements.features.is_empty());
        assert_eq!(entitlements.seats, None);
        assert_eq!(entitlements.expires_at, None);
        assert!(!entitlements.non_commercial);
        assert_eq!(entitlements.licensee, None);
    }

    #[test]
    fn test_require_ok_when_feature_is_present() {
        let mut entitlements = Entitlements::free();
        entitlements.features.insert(EXPORT);

        assert!(entitlements.require(&EXPORT).is_ok());
    }

    #[test]
    fn test_require_returns_feature_locked_with_feature_and_tier_when_absent() {
        let entitlements = Entitlements::free();

        let err = entitlements
            .require(&EXPORT)
            .expect_err("not granted in Entitlements::free()");

        assert_eq!(err.feature, EXPORT);
        assert_eq!(err.tier, TierId::FREE);
    }

    #[test]
    fn test_fixed_entitlements_current_returns_configured_state() {
        let mut free = Entitlements::free();
        free.features.insert(HISTORY);
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

    #[test]
    fn test_every_former_tier_value_still_round_trips_and_unknown_is_kept() {
        for id in [
            "free",
            "personal",
            "pro",
            "team",
            "business",
            "enterprise",
            "some_future_tier",
        ] {
            let t: TierId = serde_json::from_str(&format!("\"{id}\"")).unwrap();
            assert_eq!(t.id(), id);
            assert_eq!(serde_json::to_string(&t).unwrap(), format!("\"{id}\""));
            let json = format!(
                r#"{{"tier":"{id}","features":[],"seats":null,"expiresAt":null,"nonCommercial":false,"licensee":null}}"#
            );
            let e: Entitlements = serde_json::from_str(&json).unwrap();
            assert_eq!(e.tier.id(), id);
        }
        assert_eq!(TierId::FREE.to_string(), "free");
    }

    #[test]
    fn test_feature_locked_carries_the_tier_of_the_entitlements() {
        let mut e = Entitlements::free();
        e.tier = TierId::new("edition_tier");
        let locked = e.require(&HISTORY).unwrap_err();
        assert_eq!(
            serde_json::to_string(&locked).unwrap(),
            r#"{"feature":"session_history","tier":"edition_tier"}"#
        );
    }
}
