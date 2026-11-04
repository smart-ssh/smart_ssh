//! Chat persistence handles of a session.

use std::sync::Arc;

use std::collections::HashSet;
use std::sync::Mutex as StdMutex;

use ssh_manager_core::sync::lock_tolerating_poison;
use tokio::sync::Mutex as AsyncMutex;

/// Issue #551 / #922: chat sessions whose first-answer title attempt was
/// already started in this process — the request runs at most once per
/// session. Owned by `AppState`; an entry is removed when its chat session
/// is deleted, so the set cannot outgrow the stored sessions.
#[derive(Default)]
pub struct TitleAttempts(StdMutex<HashSet<uuid::Uuid>>);

impl TitleAttempts {
    /// `true` exactly once per chat session id (until [`Self::forget`]).
    pub fn claim(&self, id: uuid::Uuid) -> bool {
        lock_tolerating_poison(&self.0).insert(id)
    }

    /// Removes the entry of a deleted chat session.
    pub fn forget(&self, id: uuid::Uuid) {
        lock_tolerating_poison(&self.0).remove(&id);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        lock_tolerating_poison(&self.0).len()
    }
}

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
    /// Issue #922: process-wide set of title attempts (`AppState.title_attempts`).
    pub title_attempts: Arc<TitleAttempts>,
}
