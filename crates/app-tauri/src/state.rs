//! Von Tauri verwalteter Shared State (Spec 0007, Abschnitt 3).

use std::sync::Arc;

use uuid::Uuid;

use persistence_sqlite::{SqliteAiProviderStore, SqlitePolicyStore, SqlitePromptHistoryStore};
use ssh_manager_core::profiles::{CredentialStore, ProfileStore};
use ssh_manager_core::ssh::HostKeyStore;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionUserDecision, HostKeyUserDecision};
use crate::session::SessionManager;

pub type SessionId = Uuid;
/// Kennung eines einzelnen `chat-action-proposed`-Vorschlags innerhalb
/// einer Session — global eindeutig (nicht nur pro Session), damit
/// `AppState.pending_action_confirmations` eine flache Map bleiben kann statt
/// verschachtelt nach `SessionId` sortieren zu müssen.
pub type ActionId = Uuid;

pub struct AppState {
    pub sessions: SessionManager,
    pub profile_store: Arc<dyn ProfileStore>,
    // `CredentialStore` deklariert (anders als `ProfileStore`) keinen
    // `Send + Sync`-Bound im Trait selbst (s. `core::profiles::credentials`)
    // — hier explizit im Objekttyp ergänzt, damit `Arc<dyn CredentialStore
    // + Send + Sync>` als Tauri-`State` (verlangt `Send + Sync + 'static`)
    // taugt, ohne den Trait selbst (der auch synchron/nicht-App-spezifisch
    // bleiben soll) anzufassen.
    pub credential_store: Arc<dyn CredentialStore + Send + Sync>,
    pub ai_provider_store: Arc<SqliteAiProviderStore>,
    pub host_key_store: Arc<dyn HostKeyStore>,
    /// Spec 0009: echte, persistente Filter-Regeln statt des bisherigen
    /// `NoRulesPolicyStore`-Platzhalters (s. `crate::policy`-Moduldoc). Kein
    /// `Arc<dyn PolicyStore>` wie bei `profile_store`: `SqlitePolicyStore`
    /// ist `Clone` (s. dortiger Doc-Kommentar) und wird an mehreren Stellen
    /// per Wert in eine neue `FilterEngine` verschoben (`connect`,
    /// `evaluate_explained`) — ein zusätzlicher `Arc` bräuchte es dafür
    /// nicht, ein `.clone()` reicht (klont nur den intern bereits
    /// referenzgezählten `SqlitePool`).
    pub policy_store: SqlitePolicyStore,
    /// Spec 0015: pro-Server-Prompt-Historie für die Pfeiltasten-Navigation
    /// im Chat-Eingabefeld — wie `policy_store` `Clone` statt `Arc<dyn ...>`
    /// (kein eigener Trait, teilt sich nur den `SqlitePool`).
    pub prompt_history_store: SqlitePromptHistoryStore,

    /// Wartende `connect()`-Aufrufe, die auf `confirm_host_key` warten (s.
    /// `crate::commands::connect`). Schlüssel ist die `SessionId`, die
    /// `connect()` bereits vor dem eigentlichen Verbindungsaufbau vergibt
    /// (s. dortiger Kommentar) — pro `SessionId` kann zu jedem Zeitpunkt
    /// höchstens ein `connect()`-Versuch aktiv sein, ein flacher Schlüssel
    /// reicht daher.
    pub pending_host_key_confirmations: ConfirmationRegistry<SessionId, HostKeyUserDecision>,
    /// Wartende `respond_to_action`-Aufrufe (Confirm-Pfad der Kernschleife,
    /// Spec 0007 Abschnitt 6).
    pub pending_action_confirmations: ConfirmationRegistry<ActionId, ActionUserDecision>,
    /// Spec 0027: ein Eintrag pro aktuell laufendem, abbrechbarem
    /// `SuggestCommand`-Aufruf (`execute_suggested_command` registriert vor
    /// `execute_cancellable`, `commands::cancel_running_command` löst auf).
    /// Denselben generischen Typ wiederverwendet wie die beiden Registries
    /// oben — hier trägt der Wert keine Nutzdaten (`()`), nur "jetzt
    /// abbrechen". `Arc`, weil `Session` (die `execute_suggested_command`
    /// tatsächlich aufruft) sich bei `connect()` einen eigenen, billigen
    /// Klon hält, statt dass jede Aufrufkette bis dorthin extra
    /// `AppState` durchreichen müsste (dieselbe Begründung wie bei
    /// `Session::risk_second_opinion_provider`, Spec 0026).
    pub running_command_cancellations: Arc<ConfirmationRegistry<ActionId, ()>>,
}
