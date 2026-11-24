//! Corrupt (blob) content yields a clean error, not a panic.

use chrono::Utc;
use sqlx::sqlite::SqliteConnectOptions;
use uuid::Uuid;

use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::testing::ServerBuilder;

use super::*;
use crate::SqliteProfileStore;

async fn in_memory_chat_session_store() -> (SqliteProfileStore, SqliteChatSessionStore) {
    let options = SqliteConnectOptions::new().filename(":memory:");
    let profile_store = SqliteProfileStore::connect_with(options).await.unwrap();
    let chat_store = profile_store.chat_session_store();
    (profile_store, chat_store)
}

async fn create_test_server(profile_store: &SqliteProfileStore) -> ServerId {
    let server = ServerBuilder::new().name("Test-Server").build();
    let id = server.id;
    profile_store.create_server(&server).await.unwrap();
    id
}

/// Ein Inhalt, der kein Text ist (eine nicht umgestellte Altzeile), ist
/// ein klarer Fehler — kein Panic, keine stille Lücke im Verlauf.
#[tokio::test]
async fn test_a_blob_message_yields_a_clean_error_not_a_panic() {
    let (profile_store, chat_store) = in_memory_chat_session_store().await;
    let server_id = create_test_server(&profile_store).await;
    let session_id = chat_store.create_session(&server_id, None).await.unwrap();
    sqlx::query(
        "INSERT INTO chat_messages \
         (id, session_id, role, content_type, content, sequence, created_at) \
         VALUES (?, ?, 'user', 'text', ?, 0, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id.to_string())
    .bind(vec![0u8; 40])
    .bind(Utc::now().to_rfc3339())
    .execute(&profile_store.pool)
    .await
    .unwrap();

    assert!(matches!(
        chat_store.load_session(session_id).await,
        Err(ChatSessionStoreError::Database(_))
    ));
}

/// Wie oben, für `load_summary`.
#[tokio::test]
async fn test_a_blob_summary_yields_a_clean_error_not_a_panic() {
    let (profile_store, chat_store) = in_memory_chat_session_store().await;
    let server_id = create_test_server(&profile_store).await;
    let session_id = chat_store.create_session(&server_id, None).await.unwrap();
    sqlx::query(
        "UPDATE chat_sessions SET summary_text = ?, summary_rounds_covered = 2 WHERE id = ?",
    )
    .bind(vec![0u8; 40])
    .bind(session_id.to_string())
    .execute(&profile_store.pool)
    .await
    .unwrap();

    assert!(matches!(
        chat_store.load_summary(session_id).await,
        Err(ChatSessionStoreError::Database(_))
    ));
}
