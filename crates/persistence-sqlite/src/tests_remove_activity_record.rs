//! Issue #1506: migration 0030 drops the activity record (`ledger_entries`,
//! `activity_sessions`, `chat_sessions.activity_session_id`) and leaves chat
//! sessions and chat messages untouched.

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, Row, SqliteConnection};
use std::path::Path;

use crate::test_support::{applied_migrations, migrate_up_to, open_raw};

async fn empty_database(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("smart-ssh.db");
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .expect("create database")
    .close()
    .await
    .expect("close");
    path
}

async fn table_exists(conn: &mut SqliteConnection, name: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(name)
    .fetch_one(conn)
    .await
    .unwrap()
        > 0
}

async fn chat_columns(conn: &mut SqliteConnection) -> Vec<String> {
    sqlx::query("SELECT name FROM pragma_table_info('chat_sessions') ORDER BY cid")
        .fetch_all(conn)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect()
}

/// Rows of `chat_sessions` (without the dropped column) and `chat_messages`.
async fn chat_snapshot(
    conn: &mut SqliteConnection,
) -> (Vec<Vec<Option<String>>>, Vec<Vec<Option<String>>>) {
    let sessions = sqlx::query(
        "SELECT id, server_id, title, started_at, ended_at, ai_provider_id FROM chat_sessions ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap()
    .iter()
    .map(|r| (0..6).map(|i| r.get::<Option<String>, _>(i)).collect())
    .collect();
    let messages = sqlx::query(
        "SELECT id, session_id, role, content_type, content, CAST(sequence AS TEXT), created_at \
         FROM chat_messages ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap()
    .iter()
    .map(|r| (0..7).map(|i| r.get::<Option<String>, _>(i)).collect())
    .collect();
    (sessions, messages)
}

async fn seed_chat(conn: &mut SqliteConnection) {
    for sql in [
        "INSERT INTO servers (id, name, host, port, username, auth_method, created_at, updated_at) \
         VALUES ('s1', 'srv', 'h.example', 22, 'u', '{}', 't', 't')",
        "INSERT INTO chat_sessions (id, server_id, title, started_at) VALUES ('c1', 's1', 'T', 'a')",
        "INSERT INTO chat_sessions (id, server_id, title, started_at) VALUES ('c2', 's1', NULL, 'b')",
        "INSERT INTO chat_messages (id, session_id, role, content_type, content, sequence, created_at) \
         VALUES ('m1', 'c1', 'user', 'text', '\"hello\"', 0, 'a')",
        "INSERT INTO chat_messages (id, session_id, role, content_type, content, sequence, created_at) \
         VALUES ('m2', 'c1', 'assistant', 'text', '\"hi\"', 1, 'b')",
    ] {
        sqlx::query(sql).execute(&mut *conn).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_migration_0030_drops_the_record_and_keeps_chat_rows_from_0029() {
    let dir = tempfile::tempdir().unwrap();
    let path = empty_database(dir.path()).await;
    migrate_up_to(&path, None, 29).await;
    let mut conn = open_raw(&path, None).await;
    seed_chat(&mut conn).await;
    sqlx::query(
        "INSERT INTO activity_sessions (id, kind, server_id, server_name, server_host, server_port, \
         server_user, started_at) VALUES ('c1', 'chat', 's1', 'srv', 'h.example', 22, 'u', 'a')",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query("UPDATE chat_sessions SET activity_session_id = id WHERE id = 'c1'")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, created_at) \
         VALUES ('l1', 'c1', 0, 'ai', 'command_proposed', x'00', 'a')",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    let before = chat_snapshot(&mut conn).await;
    assert!(chat_columns(&mut conn)
        .await
        .contains(&"activity_session_id".to_string()));
    conn.close().await.unwrap();

    migrate_up_to(&path, None, 30).await;

    let mut conn = open_raw(&path, None).await;
    assert_eq!(chat_snapshot(&mut conn).await, before);
    assert_eq!(before.0.len(), 2);
    assert_eq!(before.1.len(), 2);
    assert!(!table_exists(&mut conn, "ledger_entries").await);
    assert!(!table_exists(&mut conn, "activity_sessions").await);
    assert!(!chat_columns(&mut conn)
        .await
        .contains(&"activity_session_id".to_string()));
    conn.close().await.unwrap();
    assert_eq!(
        applied_migrations(&path, None).await.last().copied(),
        Some(30)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_migration_0030_also_migrates_a_database_that_never_had_0029() {
    let dir = tempfile::tempdir().unwrap();
    let path = empty_database(dir.path()).await;
    migrate_up_to(&path, None, 28).await;
    let mut conn = open_raw(&path, None).await;
    seed_chat(&mut conn).await;
    // The pre-0029 ledger table (0011) holds a row of its own.
    sqlx::query(
        "INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, created_at) \
         VALUES ('l1', 'c1', 0, 'ai', 'command_proposed', x'00', 'a')",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    let before = chat_snapshot(&mut conn).await;
    conn.close().await.unwrap();

    migrate_up_to(&path, None, 30).await;

    let mut conn = open_raw(&path, None).await;
    assert_eq!(chat_snapshot(&mut conn).await, before);
    assert!(!table_exists(&mut conn, "ledger_entries").await);
    assert!(!table_exists(&mut conn, "activity_sessions").await);
    assert!(!chat_columns(&mut conn)
        .await
        .contains(&"activity_session_id".to_string()));
    conn.close().await.unwrap();
}
