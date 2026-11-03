//! Chat persistence handles of a session.

use std::sync::Arc;

use tokio::sync::Mutex as AsyncMutex;

/// Issue #871: where a session persists its chat (chat session store, ledger,
/// chat session id) plus the chat content extension registry.
pub struct ChatPersistence {
    /// Spec 0034: Persistenz-Anbindung für diese Sitzung. `None` für
    /// Sitzungen, die bewusst keine `chat_sessions`-Zeile bekommen sollen
    /// (Tests; s. Abschnitt 10 zu MCP — MCP-ausgelöste Aktionen laufen
    /// ohnehin nie über `run_chat_turn`/eine eigene `Session`, s.
    /// `app_shell::mcp_backend`, insofern betrifft dieses Feld sie gar nicht
    /// erst). Konkreter Store-Typ statt Trait-Abstraktion — derselbe
    /// Präzedenzfall wie `SqlitePromptHistoryStore` in `AppState` (Spec
    /// 0015): kein `core`-Trait für diese Art Hilfs-Store, anders als
    /// `ProfileStore`/`PolicyStore`/`AiProvider`.
    pub chat_session_store: Option<persistence_sqlite::SqliteChatSessionStore>,
    /// Spec 0057, §1: das Session-Ledger dieser Sitzung. Bewusst
    /// UNABHÄNGIG vom `persist`-Flag von `push_history_scoped` (das MCP-
    /// Herkunft von `chat_messages` ausschließt, Spec 0034/0040) — das
    /// Ledger soll MCP-Aktivität gerade erfassen (Spec 0057, §1.1: Quelle
    /// `mcp-agent`). Gleiches `None`-Verhalten wie `chat_session_store`
    /// (Tests; Sitzungen ohne eigene `chat_sessions`-Zeile).
    pub ledger_store: Option<persistence_sqlite::SqliteLedgerStore>,
    /// Die `chat_sessions.id`-Zeile dieser laufenden Sitzung — `None`, bis
    /// `app_shell::commands::connect_session` sie anlegt (bzw. bei
    /// `resume_chat_session`, Teil 2, auf die wiederverwendete Zeile
    /// gesetzt wird). `AsyncMutex` statt `StdMutex`, weil
    /// `history_push_and_persist` sie über einen `.await`-Punkt hinweg
    /// hält (der eigentliche `INSERT`).
    pub chat_session_id: AsyncMutex<Option<uuid::Uuid>>,
    /// Issue #505: chat content extensions (`AppState.chat_content`); the
    /// send path seals extension parts through it.
    pub chat_content: Arc<crate::chat_content_extension::ChatContentRegistry>,
}
