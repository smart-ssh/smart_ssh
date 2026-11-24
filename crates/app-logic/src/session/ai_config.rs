//! AI-related state of a session: provider, budget, context, summary.

use std::sync::{Arc, Mutex as StdMutex};

use ssh_manager_core::ai::{AiProvider, SessionContext};
use tokio::sync::Mutex as AsyncMutex;

/// Issue #871: the AI configuration and conversation state of a session
/// (provider, rate-limit budget, context, summary, request pacing). Field
/// locks are unchanged: each field keeps its own mutex.
pub struct AiConfig {
    pub provider: Box<dyn AiProvider>,
    /// Spec 0061: der Rate-Limit-Budget-Wächter für `ai_provider` (aus
    /// `AppState.rate_limit_registry` bei `connect()` aufgelöst, s.
    /// `risk_second_opinion_budget`-Doc-Kommentar zur Begründung, warum
    /// dieses separate Feld nötig ist).
    pub provider_budget: Arc<ai_providers::ProviderBudgetGuard>,
    pub context: AsyncMutex<SessionContext>,
    /// Für `NoteEditor::Ai { provider, model }` bei `ProposeNoteUpdate`
    /// (Spec 0003, Abschnitt 5.3) — aus der aktiven `AiProviderConfig` zum
    /// Verbindungszeitpunkt übernommen, damit spätere Notiz-Historie
    /// nachvollziehbar bleibt, welcher Provider/welches Modell den
    /// Vorschlag gemacht hat.
    pub label: String,
    pub model: String,
    /// Spec 0057, §4.1: die ungefencten Rohbestandteile des aktuellen
    /// System-Prompts (Basis-Text, Notiz-Sektionen nach Scope geordnet —
    /// seit Spec 0064 OHNE die Remote-OS-Info, die lebt jetzt als eigene,
    /// gefencte Verlaufs-Nachricht, s. `crate::compaction::
    /// SystemContextParts`-Doc-Kommentar) — parallel zu `context.lock()
    /// .await.system_context`
    /// (dem bereits zusammengesetzten String) gepflegt, s.
    /// `crate::compaction::SystemContextParts`-Doc-Kommentar zur
    /// Begründung, warum das getrennt statt nur aus dem fertigen String
    /// gehalten wird. Aktualisiert an denselben zwei Stellen wie `context`:
    /// `app_shell::commands::connect_session` (initial) und
    /// `send_chat_message_impl` (bei jeder Nutzer-Nachricht neu gebaut).
    pub system_context_parts: AsyncMutex<crate::compaction::SystemContextParts>,
    /// Spec 0057, §3.1: geschätztes Kontextfenster (Token) des bei
    /// `connect()` aktiven KI-Providers/-Modells — einmalig aufgelöst über
    /// `crate::compaction::model_context_window_tokens`, analog zu
    /// `ai_provider_label`/`ai_model` oben (ein während der Sitzung
    /// geänderter aktiver Provider greift wie dort erst bei der nächsten
    /// `connect()`).
    pub model_context_window_tokens: usize,
    /// Spec 0057, §2: die aktuelle rollierende Zusammenfassung alter,
    /// bereits aus dem KI-Kontext verdichteter Gesprächsrunden — `None`,
    /// solange Schritt 1 der Kompaktierungs-Leiter (`compaction::
    /// compact_for_send`) noch nie greifen musste, oder nach einem
    /// wiederaufgenommenen Resume ohne zuvor gespeicherte Summary.
    /// Aktualisiert ausschließlich von `compaction::
    /// compact_rounds_with_summary` (nie direkt), persistiert
    /// wie die Chat-Historie/das Ledger, geschützt durch die Verschlüsselung
    /// der Datenbankdatei (Spec 0036, Spec 0057 §2.3),
    /// s. `persistence_sqlite::SqliteChatSessionStore::save_summary`.
    pub summary: AsyncMutex<Option<crate::compaction::RollingSummary>>,
    /// Issue #601: real input sizes reported by the provider, used to
    /// correct the compaction estimate (in memory, per session).
    pub context_usage: StdMutex<crate::compaction::ContextUsageTracker>,
    /// Spec 0051, Teil 2: Zeitpunkt des letzten `AiProvider::send()`-
    /// Aufrufs dieser Sitzung — über alle KI-Anfrage-Arten hinweg
    /// (Haupt-Chat, Risiko-Zweitmeinung, Einschleusungs-Check, Auto-Titel,
    /// Notiz-Vorschlag) und unabhängig von der konkreten `AiProvider`-
    /// Instanz: `risk_second_opinion_provider`/`injection_check_provider`
    /// können denselben API-Key/dasselbe Rate-Limit-Kontingent wie
    /// `ai_provider` teilen, ohne dass die App das unterscheiden könnte —
    /// s. `crate::orchestration::wait_for_ai_request_slot`, das dieses Feld unmittelbar
    /// vor jedem `send()`-Aufruf konsultiert. `None` vor dem ersten Aufruf.
    pub request_paced_at: AsyncMutex<Option<tokio::time::Instant>>,
}
