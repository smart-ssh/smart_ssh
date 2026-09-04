//! Von Tauri verwalteter Shared State (Spec 0007, Abschnitt 3).

use std::sync::Arc;

use uuid::Uuid;

use persistence_sqlite::{
    SqliteAiProviderStore, SqliteChatSessionStore, SqlitePolicyStore, SqlitePromptHistoryStore,
};
use ssh_manager_core::entitlements::EntitlementProvider;
use ssh_manager_core::profiles::{CredentialStore, ProfileStore};
use ssh_manager_core::shared::ServerId;
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
    /// (kein eigener Trait, teilt sich nur den `SqlitePool`). Spec 0040,
    /// Abschnitt 7: `None`, wenn der Verschlüsselungsschlüssel beim
    /// Start nicht aufgelöst werden konnte (gesperrtes/verweigertes
    /// OS-Schlüsselbund) — degradiert dann zu "keine Prompt-Historie
    /// diese Sitzung", statt den App-Start ganz zu verhindern (s.
    /// `lib::build_app_state`-Doc-Kommentar).
    pub prompt_history_store: Option<SqlitePromptHistoryStore>,
    /// Spec 0034: persistente Chat-Sitzungen/-Nachrichten — wie
    /// `policy_store`/`prompt_history_store` `Clone` statt `Arc<dyn ...>`.
    /// Spec 0040, Abschnitt 7: `None` aus demselben Grund wie
    /// `prompt_history_store` oben — degradiert zu "kein Chat-Verlauf wird
    /// gespeichert/kann fortgesetzt werden" für die laufende App-Instanz.
    pub chat_session_store: Option<SqliteChatSessionStore>,
    /// Spec 0037, Abschnitt 2/3 (D5): aktueller Entitlement-Stand — die
    /// Community Edition kennt aktuell nur den einen festen Zustand
    /// `FixedEntitlements(Entitlements::free())`, kein Lizenzschlüssel-
    /// Mechanismus in diesem Schritt (s. `lib::build_app_state`). `Arc<dyn
    /// ...>` wie `profile_store`/`credential_store`, nicht `Clone` wie
    /// `policy_store`: ein künftiges Lizenzschlüssel-/Managed-Backend
    /// (privates Repo) wird keinen billig klonbaren `SqlitePool`-artigen
    /// internen Zustand haben.
    ///
    /// `#[allow(dead_code)]`: dieser Schritt führt bewusst nur das
    /// Entitlements-Vokabular ein (Spec 0037, Abschnitt 1: "kennt nur die
    /// Typen"), ohne einen einzigen tatsächlich gegateten Command — Word-
    /// Export (der einzige Kandidat) wurde stattdessen komplett entfernt
    /// (Abschnitt 4), statt gegatet zu werden. Das Feld bleibt bis zum
    /// ersten echten `require(...)`-Aufruf (künftiges gegatetes Feature,
    /// privates Repo) unvermeidlich ungelesen — derselbe "Vokabular, noch
    /// nirgends verwendet"-Fall wie bei `ssh_manager_core::session`s Typen
    /// (Spec 0037, Abschnitt 7), dort für `core` (als Bibliothek von der
    /// Dead-Code-Analyse ohnehin ausgenommen), hier explizit markiert, weil
    /// `app-tauri` auch ein Binary-Target hat.
    #[allow(dead_code)]
    pub entitlements: Arc<dyn EntitlementProvider>,

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
    /// Spec 0028: Zustand des lokalen MCP-Servers — getrennt von den
    /// übrigen, rein persistenten Einstellungen (`tauri-plugin-store`,
    /// s. `crate::mcp_settings`), weil Token/Allow-Liste/laufender Server
    /// **live** sein müssen (ein "Neu generieren" muss das alte Token
    /// sofort invalidieren, kein Neustart der App nötig).
    pub mcp: McpState,
}

pub struct McpState {
    /// Live-veränderlich (s. `mcp_server::SharedToken`-Doc-Kommentar) —
    /// bei "Neu generieren" wird hier direkt hineingeschrieben, ein bereits
    /// laufender HTTP-Server sieht die Änderung beim nächsten Tool-Call.
    pub token: mcp_server::SharedToken,
    /// Spec 0028, Abschnitt 6: welche Server über MCP ansprechbar sind —
    /// im Speicher gehalten (nicht bei jedem Tool-Call aus dem Store
    /// gelesen), damit `crate::mcp_backend` sie synchron ohne zusätzliche
    /// Async-I/O prüfen kann; `crate::mcp_settings` hält diesen Cache und
    /// den persistierten Store synchron.
    pub allowed_servers: std::sync::Mutex<std::collections::HashSet<ServerId>>,
    /// `Some`, während der HTTP-Server läuft — `crate::mcp_settings`
    /// startet/stoppt ihn und ersetzt diesen Wert entsprechend.
    pub runtime: tokio::sync::Mutex<Option<mcp_server::McpServerHandle>>,
}

impl Default for McpState {
    /// Der `SharedToken`-Startwert ist irrelevant, solange kein Server
    /// läuft (`runtime` startet als `None`) — `crate::mcp_settings::
    /// get_mcp_server_settings` synchronisiert ihn beim ersten Aufruf mit
    /// dem persistierten (oder frisch generierten) Token.
    fn default() -> Self {
        Self {
            token: mcp_server::SharedToken::new(Uuid::new_v4().to_string()),
            allowed_servers: std::sync::Mutex::new(std::collections::HashSet::new()),
            runtime: tokio::sync::Mutex::new(None),
        }
    }
}
