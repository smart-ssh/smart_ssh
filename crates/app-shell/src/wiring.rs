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
    ///
    /// Superseded by [`Wiring::session_extensions`] and will be removed in a
    /// later release. When set, it counts as the first entry of that list.
    pub session_extension: Option<Arc<dyn app_logic::session_extension::SessionExtension>>,
    /// Issue #892 (Spec 0038, section 2a): any number of session extensions,
    /// in registration order. The core combines them (context sections
    /// joined, secrets united, removal callbacks to all). Community
    /// registers none.
    pub session_extensions: Vec<Arc<dyn app_logic::session_extension::SessionExtension>>,
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
    pub outgoing_transform: Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,

    /// Issue #610 (Spec 0038, section 2): observers told about every
    /// completed AI request of a session (ids, provider, purpose, usage;
    /// never content). Passive: they cannot influence the chat flow.
    /// Community registers none.
    pub ai_usage_observers: Vec<Arc<dyn app_logic::ai_usage::AiUsageObserver>>,
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
            session_extensions: Vec::new(),
            chat_content_extensions: Vec::new(),

            outgoing_transform: None,
            ai_usage_observers: Vec::new(),
        }
    }
}

impl Wiring {
    /// The single `Option` the rest of the app works with: `None` without
    /// extensions, the extension itself for exactly one, otherwise a
    /// [`CompositeSessionExtension`] (legacy field first).
    pub fn combined_session_extension(
        &self,
    ) -> Option<Arc<dyn app_logic::session_extension::SessionExtension>> {
        let mut all: Vec<_> = self
            .session_extension
            .iter()
            .chain(self.session_extensions.iter())
            .cloned()
            .collect();
        match all.len() {
            0 => None,
            1 => all.pop(),
            _ => Some(Arc::new(
                app_logic::session_extension::CompositeSessionExtension::new(all),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #892: Community registers no session extension, builds no composite.
    #[test]
    fn test_community_builds_no_session_extension() {
        let w = Wiring::community();
        assert!(w.session_extensions.is_empty());
        assert!(w.combined_session_extension().is_none());
    }

    struct Named(&'static str);

    #[async_trait::async_trait]
    impl app_logic::session_extension::SessionExtension for Named {
        async fn context_section(
            &self,
            _scope: &app_logic::session_extension::ServerScope,
            _language: app_logic::system_prompt::PromptLanguage,
        ) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    /// Issue #892: legacy field and list entry are both active, legacy first.
    #[tokio::test]
    async fn test_legacy_field_and_list_entry_are_both_active_legacy_first() {
        let mut w = Wiring::community();
        w.session_extension = Some(Arc::new(Named("legacy")));
        w.session_extensions.push(Arc::new(Named("listed")));
        let combined = w.combined_session_extension().expect("combined");
        let scope = app_logic::session_extension::ServerScope {
            server_id: ssh_manager_core::shared::ServerId::new(),
            group_chain: Vec::new(),
            tags: Vec::new(),
        };
        let text = combined
            .context_section(&scope, app_logic::system_prompt::PromptLanguage::En)
            .await
            .expect("section");
        assert_eq!(text, "legacy\n\nlisted");
    }

    /// Issue #892: only the legacy field behaves as before (no wrapper needed).
    #[tokio::test]
    async fn test_only_legacy_field_passes_through() {
        let mut w = Wiring::community();
        w.session_extension = Some(Arc::new(Named("legacy")));
        assert!(w.combined_session_extension().is_some());
    }

    /// Issue #504: Community registers no chat content extension.
    #[test]
    fn test_community_registers_no_chat_content_extensions() {
        assert!(Wiring::community().chat_content_extensions.is_empty());
    }

    /// Issue #610: Community registers no AI usage observer.
    #[test]
    fn test_community_registers_no_ai_usage_observers() {
        assert!(Wiring::community().ai_usage_observers.is_empty());
    }
}
