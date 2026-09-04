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
    pub plugins: Vec<PluginHook>,
    pub edition: Edition,
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
            edition: Edition::Community,
        }
    }
}
