//! Issue #504 (ADR 0141): chat content extensions from `Wiring`.
//!
//! Called from `open_and_assemble` once the database is open and the
//! extension migrations ran.

use std::sync::Arc;

use app_logic::chat_content_extension::ChatContentRegistry;
use persistence_sqlite::SqliteProfileStore;

/// Registers the registry as removal observer of every chat-session delete
/// path and hands each extension the ids of the existing sessions once
/// (`reconcile`). Without a registered extension nothing happens.
pub(crate) async fn attach_chat_content(
    store: &SqliteProfileStore,
    chat_content: &Arc<ChatContentRegistry>,
) {
    if chat_content.is_empty() {
        return;
    }
    store.set_chat_removal_observer(chat_content.clone());
    match store.chat_session_ids().await {
        Ok(ids) => chat_content.reconcile(&ids),
        Err(err) => {
            tracing::warn!(error = %err, "chat content reconcile skipped: sessions not readable");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_logic::chat_content_extension::ChatContentExtension;
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct Seen {
        reconciled: Mutex<Vec<Vec<Uuid>>>,
    }
    struct Ext(Arc<Seen>);
    impl ChatContentExtension for Ext {
        fn name(&self) -> &str {
            "files"
        }
        fn reconcile(&self, live: &[Uuid]) {
            self.0.reconciled.lock().unwrap().push(live.to_vec());
        }
    }

    async fn empty_store() -> (tempfile::TempDir, SqliteProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn test_reconcile_is_called_once_with_the_existing_sessions() {
        let (_dir, store) = empty_store().await;
        let seen = Arc::new(Seen::default());
        let registry = Arc::new(ChatContentRegistry::new(vec![Arc::new(Ext(seen.clone()))]));
        attach_chat_content(&store, &registry).await;
        assert_eq!(*seen.reconciled.lock().unwrap(), vec![Vec::<Uuid>::new()]);
    }

    #[tokio::test]
    async fn test_without_extensions_nothing_is_attached() {
        let (_dir, store) = empty_store().await;
        attach_chat_content(&store, &Arc::new(ChatContentRegistry::default())).await;
        // No observer: deleting works and nothing is reported anywhere.
        let chat = store.chat_session_store();
        chat.delete_session(Uuid::new_v4()).await.unwrap();
    }
}
