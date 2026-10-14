//! `Wiring`/`Edition` (Spec 0038, Abschnitt 2): definiert, welche
//! Implementierungen (Entitlements, KI-Provider, Policy-Quellen,
//! Sync-Backends, Plugins) ein konkretes Binary in [`crate::run`]
//! einspeist. Community- und ein künftiges Official-Binary unterscheiden
//! sich nur im übergebenen `Wiring`.

use std::sync::Arc;

use ssh_manager_core::ai::AiProvider;
use ssh_manager_core::entitlements::{EntitlementProvider, Entitlements, FixedEntitlements};
use ssh_manager_core::filter::PolicySource;
use ssh_manager_core::session::SyncBackend;
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
    pub sync_backends: Vec<Arc<dyn SyncBackend>>,
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
    pub edition: Edition,
    /// Issue #16: zusätzliche Datenpfade einer Edition (z. B. eine
    /// Lizenzdatei), die die Einstellungen neben den eingebauten Pfaden
    /// anzeigen. Community liefert keine.
    pub extra_data_paths: Vec<app_logic::data_paths::ExtraDataPath>,
    /// Spec 0038, section 2a: per-server session context section, known
    /// secrets for redaction and removal callbacks. Community has none.
    pub session_extension: Option<Arc<dyn app_logic::session_extension::SessionExtension>>,
    /// Issue #469 (Spec 0110): Quellen für Chat-Anhänge. Der Kern kann keine
    /// Dateien auswählen; eine Edition registriert hier, wie eine opake
    /// Anhang-ID in einen Anhang aufgelöst wird. Community liefert keine.
    pub chat_attachment_resolvers: Vec<Arc<dyn ssh_manager_core::ai::ChatAttachmentResolver>>,
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
            // Spec 0037, Abschnitt 6: kein Git-Sync-Backend in dieser Spec.
            sync_backends: Vec::new(),
            plugins: Vec::new(),
            db_extensions: Vec::new(),
            edition: Edition::Community,
            extra_data_paths: Vec::new(),
            session_extension: None,
            chat_attachment_resolvers: Vec::new(),
            chat_content_extensions: Vec::new(),

            outgoing_transform: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #469: Community registers no attachment resolver.
    #[test]
    fn test_community_registers_no_chat_attachment_resolvers() {
        assert!(Wiring::community().chat_attachment_resolvers.is_empty());
    }

    /// Issue #504: Community registers no chat content extension.
    #[test]
    fn test_community_registers_no_chat_content_extensions() {
        assert!(Wiring::community().chat_content_extensions.is_empty());
    }
}
