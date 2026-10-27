//! `Wiring`/`Edition` (Spec 0038, Abschnitt 2): definiert, welche
//! Implementierungen (Entitlements, KI-Provider, Policy-Quellen,
//! Plugins) ein konkretes Binary in [`crate::run`]
//! einspeist. Community- und ein künftiges Official-Binary unterscheiden
//! sich nur im übergebenen `Wiring`.

use std::sync::Arc;

use ssh_manager_core::ai::AiProvider;
use ssh_manager_core::entitlements::{EntitlementProvider, Entitlements, FixedEntitlements};
use ssh_manager_core::filter::PolicySource;
use tauri::{Builder, Wry};

/// Nur für Anzeige/Updater relevant (Spec 0038, Abschnitt 2) — keine
/// eigene Verzweigung in der Filter-/Risiko-/Bestätigungslogik (s.
/// CLAUDE.md, "No special-casing").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edition {
    Community,
    #[allow(dead_code)]
    Official,
}

/// Ein Plugin-Hook (Spec 0038, Abschnitt 2): registriert ein zusätzliches
/// Tauri-Plugin auf dem in [`crate::run`] gebauten `Builder`, bevor die App
/// startet. Community nutzt aktuell keinen (`Wiring::community`s Liste ist
/// leer) — die Infrastruktur existiert für ein künftiges Official-Binary.
pub type PluginHook = Box<dyn FnOnce(Builder<Wry>) -> Builder<Wry> + Send>;

pub struct Wiring {
    pub entitlements: Arc<dyn EntitlementProvider>,
    pub ai_providers: Vec<Arc<dyn AiProvider>>,
    pub policy_sources: Vec<Arc<dyn PolicySource>>,
    /// Zusätzliche Tauri-Plugins.
    ///
    /// **Im Passwort-Modus** (Master-Passwort, Spec 0101) dürfen Hooks, die
    /// Befehle registrieren, diese erst **nach dem Entsperren** registrieren,
    /// sonst wären sie vor dem Entsperren aufrufbar (ADR 0095 §8).
    pub plugins: Vec<PluginHook>,
    /// Issue #462 (Spec 0038, 2a): extensions that keep their own tables in
    /// the encrypted application database. Their migrations run after the
    /// core migrations and **only after unlock** (never while the app is
    /// locked in master-password mode).
    pub db_extensions: Vec<crate::db_extensions::DbExtension>,
    /// Issue #669 (Spec 0038, section 2): background services. Started once
    /// per unlock after the database extensions are ready, stopped on lock
    /// and on exit (5 s timeout each). A failing service is skipped.
    /// Community registers none.
    pub services: Vec<Arc<dyn crate::services::BackgroundService>>,
    pub edition: Edition,
    /// Issue #16: zusätzliche Datenpfade einer Edition (z. B. eine
    /// Lizenzdatei), die die Einstellungen neben den eingebauten Pfaden
    /// anzeigen. Community liefert keine.
    pub extra_data_paths: Vec<app_logic::data_paths::ExtraDataPath>,
    /// Spec 0038, section 2a: per-server session context section, known
    /// secrets for redaction and removal callbacks. Community has none.
    pub session_extension: Option<Arc<dyn app_logic::session_extension::SessionExtension>>,
    /// Issue #504 (ADR 0141, Spec 0038 §2a): content extensions that attach
    /// their own content to user chat messages. The core stores only an
    /// opaque reference and tells them when chat sessions are removed.
    /// Names must match `[a-z0-9]+` and be unique; others are skipped.
    /// Community registers none.
    pub chat_content_extensions:
        Vec<Arc<dyn app_logic::chat_content_extension::ChatContentExtension>>,

    /// Issue #464 (Spec 0038, section 2): transforms approved commands and
    /// file content right before they are sent to the server. Community
    /// registers none, so text is sent unchanged.
    ///
    /// Superseded by [`Wiring::outgoing_transforms`] and will be removed in a
    /// later release. When set, it runs first, before the list entries.
    pub outgoing_transform: Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,
    /// Issue #893 (Spec 0038, section 2): several outgoing transforms. They
    /// run in this order on the approved text; each one receives the output
    /// of the previous one, the inserted secrets are collected, and the first
    /// rejection stops the chain. Community registers none.
    pub outgoing_transforms: Vec<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,

    /// Issue #610 (Spec 0038, section 2): observers told about every
    /// completed AI request of a session (ids, provider, purpose, usage;
    /// never content). Passive: they cannot influence the chat flow.
    /// Community registers none.
    pub ai_usage_observers: Vec<Arc<dyn app_logic::ai_usage::AiUsageObserver>>,

    /// Issue #876 (Spec 0037, section 2): the edition's catalogue of the
    /// entitlement features it gates. Only these reach the frontend
    /// (`get_entitlements`, `entitlements:changed`); other ids from the
    /// provider are dropped. Duplicates are logged and skipped at startup.
    /// Community registers none.
    pub entitlement_features: Vec<ssh_manager_core::entitlements::Feature>,
}

impl Wiring {
    /// Community-Edition-Verdrahtung.
    ///
    /// **Scope-Hinweis (s. begleitende ADR 0035):** `ai_providers` und
    /// `policy_sources` bleiben hier bewusst leer, obwohl Spec 0038
    /// Abschnitt 2 sie im Beispiel mit `ai_providers::all_byok()` bzw.
    /// `SqlitePolicySource::new(..)` skizziert. Beide Skizzen setzen
    /// Zustand voraus, der in diesem Schritt schlicht nicht existiert: die
    /// tatsächlichen `AiProvider`-Instanzen dieser Codebasis werden pro
    /// Server-Verbindung aus einem nutzerhinterlegten API-Key gebaut
    /// (`ai_provider_factory::build_ai_provider`), nicht als feste Liste
    /// beim App-Start; die SQLite-`PolicySource` hängt an der erst in
    /// `build_app_state` (nach `Wiring::community()`) geöffneten
    /// Datenbankverbindung. `entitlements` ist der einzige der drei in
    /// Abschnitt 1 genannten Fälle, der sich ohne diese Abhängigkeiten
    /// sauber aus dem bisher fest verdrahteten Setup-Code lösen lässt —
    /// genau das tut diese Funktion.
    pub fn community() -> Self {
        Self {
            entitlements: Arc::new(FixedEntitlements(Entitlements::free())),
            ai_providers: Vec::new(),
            policy_sources: Vec::new(),
            services: Vec::new(),
            plugins: Vec::new(),
            db_extensions: Vec::new(),
            edition: Edition::Community,
            extra_data_paths: Vec::new(),
            session_extension: None,
            chat_content_extensions: Vec::new(),

            outgoing_transform: None,
            outgoing_transforms: Vec::new(),
            ai_usage_observers: Vec::new(),
            entitlement_features: Vec::new(),
        }
    }
}

/// Issue #893: the single transform the session gets. The old
/// `outgoing_transform` runs first, then `outgoing_transforms` in order.
/// Zero transforms give `None` (text is sent as is), one is passed
/// through, two or more are combined into a composite.
pub(crate) fn merge(
    w: &mut Wiring,
) -> Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>> {
    let mut all: Vec<_> = w.outgoing_transform.take().into_iter().collect();
    all.append(&mut w.outgoing_transforms);
    match all.len() {
        0 => None,
        1 => all.pop(),
        _ => Some(Arc::new(
            ssh_manager_core::outgoing::CompositeOutgoingTransform::new(all),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #504: Community registers no chat content extension.
    #[test]
    fn test_community_registers_no_chat_content_extensions() {
        assert!(Wiring::community().chat_content_extensions.is_empty());
    }

    /// Issue #893: Community registers no outgoing transform, so the session
    /// gets `None`.
    #[test]
    fn test_community_registers_no_outgoing_transforms() {
        let mut w = Wiring::community();
        assert!(w.outgoing_transforms.is_empty());
        assert!(w.outgoing_transform.is_none());
        assert!(merge(&mut w).is_none());
    }

    /// Issue #893: zero -> `None`, one -> passed through, two or more ->
    /// composite; the old field runs first.
    #[tokio::test]
    async fn test_take_outgoing_transform_orders_old_field_first() {
        use ssh_manager_core::outgoing::{
            OutgoingKind, OutgoingTransform, Prepared, Rejection, ServerScope,
        };
        struct Append(&'static str);
        #[async_trait::async_trait]
        impl OutgoingTransform for Append {
            async fn prepare_outgoing(
                &self,
                _: &ServerScope,
                _: OutgoingKind,
                text: &str,
            ) -> Result<Prepared, Rejection> {
                Ok(Prepared::new(format!("{text}{}", self.0), Vec::new()))
            }
        }
        let scope = ServerScope {
            server_id: Default::default(),
            group_chain: Vec::new(),
            tags: Vec::new(),
        };
        let run = |t: Arc<dyn OutgoingTransform>| {
            let scope = scope.clone();
            async move {
                use secrecy::ExposeSecret;
                t.prepare_outgoing(&scope, OutgoingKind::Command, "x")
                    .await
                    .unwrap()
                    .text()
                    .expose_secret()
                    .to_string()
            }
        };

        let mut w = Wiring::community();
        w.outgoing_transforms = vec![Arc::new(Append("1"))];
        assert_eq!(run(merge(&mut w).unwrap()).await, "x1");

        let mut w = Wiring::community();
        w.outgoing_transform = Some(Arc::new(Append("0")));
        w.outgoing_transforms = vec![Arc::new(Append("1")), Arc::new(Append("2"))];
        assert_eq!(run(merge(&mut w).unwrap()).await, "x012");
        assert!(w.outgoing_transform.is_none() && w.outgoing_transforms.is_empty());
    }

    /// Issue #610: Community registers no AI usage observer.
    #[test]
    fn test_community_registers_no_ai_usage_observers() {
        assert!(Wiring::community().ai_usage_observers.is_empty());
    }
}

/// The deduplicated catalogue of registered entitlement features, managed as
/// Tauri state. Filters what the frontend sees.
pub struct EntitlementCatalogue(Vec<ssh_manager_core::entitlements::Feature>);

impl EntitlementCatalogue {
    /// Builds the catalogue; logs and skips duplicates.
    pub fn new(features: Vec<ssh_manager_core::entitlements::Feature>) -> Self {
        let mut unique: Vec<ssh_manager_core::entitlements::Feature> = Vec::new();
        for f in features {
            if unique.contains(&f) {
                tracing::warn!(feature = %f, "duplicate entitlement feature skipped");
            } else {
                unique.push(f);
            }
        }
        Self(unique)
    }

    /// The entitlements as the frontend may see them: only registered
    /// features; unknown ids are dropped (logged at debug, id only).
    pub fn filter(
        &self,
        mut e: ssh_manager_core::entitlements::Entitlements,
    ) -> ssh_manager_core::entitlements::Entitlements {
        for f in &e.features {
            if !self.0.contains(f) {
                tracing::debug!(feature = %f, "unregistered entitlement feature dropped");
            }
        }
        e.retain_catalogue(&self.0);
        e
    }
}

#[cfg(test)]
mod entitlement_catalogue_tests {
    use super::*;
    use ssh_manager_core::entitlements::{Entitlements, Feature};

    const A: Feature = Feature::new("feature_a");
    const B: Feature = Feature::new("feature_b");

    fn granting_a_and_b() -> Entitlements {
        let mut e = Entitlements::free();
        e.features.insert(A);
        e.features.insert(B);
        e
    }

    #[test]
    fn test_registered_catalogue_passes_only_its_features() {
        let c = EntitlementCatalogue::new(vec![A, A]);
        let out = c.filter(granting_a_and_b());
        assert_eq!(out.features.len(), 1);
        assert!(out.has(&A));
    }

    #[test]
    fn test_community_catalogue_returns_no_features() {
        let c = EntitlementCatalogue::new(Wiring::community().entitlement_features);
        assert!(c.filter(granting_a_and_b()).features.is_empty());
    }
}
