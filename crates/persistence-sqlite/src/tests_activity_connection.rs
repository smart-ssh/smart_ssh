//! Connection entries, terminal-only sessions, activity ticks and crash
//! recovery of the activity record (Spec 0057, issue #1447).

use chrono::{Duration, Utc};
use sqlx::Row;
use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::testing::ServerBuilder;

use crate::SqliteProfileStore;

async fn store() -> (SqliteProfileStore, ServerId) {
    let s = SqliteProfileStore::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"),
    )
    .await
    .unwrap();
    let server = ServerBuilder::new().name("S").build();
    let id = server.id;
    s.create_server(&server).await.unwrap();
    (s, id)
}

async fn scalar(s: &SqliteProfileStore, sql: &'static str, id: Uuid) -> Option<String> {
    sqlx::query(sql)
        .bind(id.to_string())
        .fetch_optional(&s.pool)
        .await
        .unwrap()
        .and_then(|r| r.get::<Option<String>, _>(0))
}

async fn connected_entry(s: &SqliteProfileStore, id: Uuid) {
    s.ledger_store()
        .append_entry(
            id,
            LedgerSource::User,
            &LedgerEntryContent::Connection {
                event: ssh_manager_core::audit::LedgerConnectionEvent::Connected,
                detail: None,
            },
        )
        .await
        .unwrap();
}

async fn count_chat_rows(s: &SqliteProfileStore, id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) FROM chat_sessions WHERE id = ?")
        .bind(id.to_string())
        .fetch_one(&s.pool)
        .await
        .unwrap()
        .get(0)
}

#[tokio::test]
async fn test_terminal_only_session_is_kept_as_terminal_on_end() {
    let (s, server) = store().await;
    let chat = s.chat_session_store();
    let id = chat.create_session(&server, None).await.unwrap();
    connected_entry(&s, id).await;
    chat.end_or_discard(id).await.unwrap();
    assert_eq!(count_chat_rows(&s, id).await, 0);
    assert_eq!(
        scalar(&s, "SELECT kind FROM activity_sessions WHERE id = ?", id).await,
        Some("terminal".into())
    );
    assert!(scalar(
        &s,
        "SELECT ended_at FROM activity_sessions WHERE id = ?",
        id
    )
    .await
    .is_some());
}

#[tokio::test]
async fn test_session_with_messages_stays_chat_on_end() {
    let (s, server) = store().await;
    let chat = s.chat_session_store();
    let id = chat.create_session(&server, None).await.unwrap();
    connected_entry(&s, id).await;
    chat.append_message(
        id,
        &ChatMessage {
            role: Role::User,
            content: MessageContent::Text("hi".into()),
        },
    )
    .await
    .unwrap();
    chat.end_or_discard(id).await.unwrap();
    assert_eq!(count_chat_rows(&s, id).await, 1);
    assert_eq!(
        scalar(&s, "SELECT kind FROM activity_sessions WHERE id = ?", id).await,
        Some("chat".into())
    );
}

#[tokio::test]
async fn test_record_failed_connection_writes_ended_terminal_session_with_code() {
    let (s, server) = store().await;
    let id = s
        .activity_session_store()
        .record_failed_connection(&server, "SSH_AUTH_FAILED")
        .await
        .unwrap();
    assert_eq!(
        scalar(&s, "SELECT kind FROM activity_sessions WHERE id = ?", id).await,
        Some("terminal".into())
    );
    assert!(scalar(
        &s,
        "SELECT ended_at FROM activity_sessions WHERE id = ?",
        id
    )
    .await
    .is_some());
    let content = scalar(
        &s,
        "SELECT content FROM ledger_entries WHERE session_id = ?",
        id,
    )
    .await
    .unwrap();
    assert!(content.contains("SSH_AUTH_FAILED"), "{content}");
    assert!(content.contains("\"failed\""), "{content}");
}

#[tokio::test]
async fn test_activity_tick_moves_last_activity_and_adds_entry() {
    let (s, server) = store().await;
    let id = s
        .chat_session_store()
        .create_session(&server, None)
        .await
        .unwrap();
    assert!(scalar(
        &s,
        "SELECT last_activity_at FROM activity_sessions WHERE id = ?",
        id
    )
    .await
    .is_none());
    s.ledger_store()
        .append_activity_tick(id, LedgerSource::User)
        .await
        .unwrap();
    assert!(scalar(
        &s,
        "SELECT last_activity_at FROM activity_sessions WHERE id = ?",
        id
    )
    .await
    .is_some());
    assert_eq!(
        scalar(
            &s,
            "SELECT entry_type FROM ledger_entries WHERE session_id = ?",
            id
        )
        .await,
        Some("activity_tick".into())
    );
}

#[tokio::test]
async fn test_crash_recovery_closes_open_sessions_at_last_activity() {
    let (s, server) = store().await;
    let chat = s.chat_session_store();
    let empty = chat.create_session(&server, None).await.unwrap();
    let with_msg = chat.create_session(&server, None).await.unwrap();
    chat.append_message(
        with_msg,
        &ChatMessage {
            role: Role::User,
            content: MessageContent::Text("hi".into()),
        },
    )
    .await
    .unwrap();
    s.ledger_store()
        .append_activity_tick(empty, LedgerSource::User)
        .await
        .unwrap();
    let last = scalar(
        &s,
        "SELECT last_activity_at FROM activity_sessions WHERE id = ?",
        empty,
    )
    .await
    .unwrap();

    let closed = s
        .activity_session_store()
        .close_open_sessions(Utc::now() + Duration::seconds(5))
        .await
        .unwrap();
    assert_eq!(closed, 2);
    // Empty crashed chat becomes terminal, ended at the last activity.
    assert_eq!(count_chat_rows(&s, empty).await, 0);
    assert_eq!(
        scalar(&s, "SELECT kind FROM activity_sessions WHERE id = ?", empty).await,
        Some("terminal".into())
    );
    assert_eq!(
        scalar(
            &s,
            "SELECT ended_at FROM activity_sessions WHERE id = ?",
            empty
        )
        .await,
        Some(last)
    );
    // A chat with messages stays a chat; no activity → ended at started_at.
    assert_eq!(count_chat_rows(&s, with_msg).await, 1);
    assert_eq!(
        scalar(
            &s,
            "SELECT kind FROM activity_sessions WHERE id = ?",
            with_msg
        )
        .await,
        Some("chat".into())
    );
    assert_eq!(
        scalar(
            &s,
            "SELECT ended_at FROM activity_sessions WHERE id = ?",
            with_msg
        )
        .await,
        scalar(
            &s,
            "SELECT started_at FROM activity_sessions WHERE id = ?",
            with_msg
        )
        .await
    );
}

#[tokio::test]
async fn test_crash_recovery_leaves_sessions_started_after_cutoff_open() {
    let (s, server) = store().await;
    let cutoff = Utc::now() - Duration::seconds(60);
    let id = s
        .chat_session_store()
        .create_session(&server, None)
        .await
        .unwrap();
    let closed = s
        .activity_session_store()
        .close_open_sessions(cutoff)
        .await
        .unwrap();
    assert_eq!(closed, 0);
    assert!(scalar(
        &s,
        "SELECT ended_at FROM activity_sessions WHERE id = ?",
        id
    )
    .await
    .is_none());
    assert_eq!(count_chat_rows(&s, id).await, 1);
}
