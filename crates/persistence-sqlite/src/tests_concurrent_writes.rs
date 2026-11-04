//! Issue #903: appends and history writes are transactional, and
//! `chat_messages(session_id, sequence)` is unique (migration 0027).

use std::path::Path;

use chrono::Utc;
use sqlx::{Connection, Row, SqliteConnection};
use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::crypto::DatabaseKey;
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

use crate::test_support::{migrate_up_to, open_raw};
use crate::SqliteProfileStore;

fn test_key() -> DatabaseKey {
    DatabaseKey::from_root_key(&ssh_manager_core::crypto::RootKey::for_tests([9; 32]))
}

async fn file_store(dir: &Path) -> SqliteProfileStore {
    SqliteProfileStore::connect_encrypted(&dir.join("smart-ssh.db"), &test_key())
        .await
        .expect("encrypted file store opens")
}

async fn create_server(store: &SqliteProfileStore) -> ServerId {
    let now = Utc::now();
    let server = Server {
        id: ServerId::new(),
        name: "s".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "u".to_string(),
        group_id: None,
        tags: vec![],
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    };
    let id = server.id;
    store.create_server(&server).await.unwrap();
    id
}

fn msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: MessageContent::Text(text.to_string()),
    }
}

async fn sequences(store: &SqliteProfileStore, table: &str, session: Uuid) -> Vec<i64> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT sequence FROM {table} WHERE session_id = ? ORDER BY sequence"
    )))
    .bind(session.to_string())
    .fetch_all(&store.pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_append_message_gets_distinct_consecutive_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(dir.path()).await;
    let server = create_server(&store).await;
    let chat = store.chat_session_store();
    let session = chat.create_session(&server, None).await.unwrap();

    let (ma, mb) = (msg("a"), msg("b"));
    let (a, b) = tokio::join!(
        chat.append_message(session, &ma),
        chat.append_message(session, &mb),
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        sequences(&store, "chat_messages", session).await,
        vec![0, 1]
    );
    store.pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_ledger_appends_get_distinct_consecutive_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(dir.path()).await;
    let server = create_server(&store).await;
    let session = store
        .chat_session_store()
        .create_session(&server, None)
        .await
        .unwrap();
    let ledger = store.ledger_store();
    let entry = |c: &str| LedgerEntryContent::CommandProposed {
        command: c.to_string(),
    };

    let (ea, eb) = (entry("a"), entry("b"));
    let (a, b) = tokio::join!(
        ledger.append_entry(session, LedgerSource::Ai, &ea),
        ledger.append_entry(session, LedgerSource::Ai, &eb),
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        sequences(&store, "ledger_entries", session).await,
        vec![0, 1]
    );
    store.pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_prompt_history_record_respects_cap_and_dedup() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(dir.path()).await;
    let server = create_server(&store).await;
    let history = store.prompt_history_store();

    // Two identical concurrent records must not duplicate the latest entry.
    let (a, b) = tokio::join!(
        history.record(&server, "same"),
        history.record(&server, "same")
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(history.list(&server).await.unwrap(), vec!["same"]);

    // Many concurrent distinct records never exceed the cap.
    let mut tasks = Vec::new();
    for i in 0..230 {
        let history = history.clone();
        tasks.push(tokio::spawn(async move {
            history.record(&server, &format!("p{i}")).await
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(history.list(&server).await.unwrap().len(), 200);
    store.pool.close().await;
}

/// Migration 0027 on an encrypted database at version 0026.
#[tokio::test(flavor = "multi_thread")]
async fn test_migration_0027_renumbers_duplicates_and_enforces_uniqueness() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart-ssh.db");
    let key = test_key();
    // Create the (empty, encrypted) file, then migrate it to 0026.
    let options = crate::encryption::encrypted_connect_options(&path, &key);
    SqliteConnection::connect_with(&options)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    migrate_up_to(&path, Some(&key), 26).await;

    let mut conn = open_raw(&path, Some(&key)).await;
    sqlx::query(
        "INSERT INTO servers (id, name, host, port, username, auth_method, notes, \
         post_ingest_policy, ai_injection_check_enabled, created_at, updated_at) \
         VALUES ('srv', 'n', 'h', 22, 'u', '\"Agent\"', '', 'Strict', 1, '2026-01-01', '2026-01-01')",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    for s in ["sess-a", "sess-b"] {
        sqlx::query(
            "INSERT INTO chat_sessions (id, server_id, started_at) VALUES (?, 'srv', '2026-01-01')",
        )
        .bind(s)
        .execute(&mut conn)
        .await
        .unwrap();
    }
    // (session, id, sequence, created_at)
    let rows = [
        ("sess-a", "a-3", 1, "2026-01-01T00:00:03Z"),
        ("sess-a", "a-1", 0, "2026-01-01T00:00:01Z"),
        ("sess-a", "a-2", 0, "2026-01-01T00:00:02Z"),
        ("sess-a", "a-4", 1, "2026-01-01T00:00:03Z"),
        ("sess-b", "b-1", 0, "2026-01-01T00:00:01Z"),
        ("sess-b", "b-2", 5, "2026-01-01T00:00:02Z"),
    ];
    for (session, id, seq, at) in rows {
        sqlx::query(
            "INSERT INTO chat_messages (id, session_id, role, content_type, content, sequence, created_at) \
             VALUES (?, ?, 'user', 'text', '{\"Text\":\"x\"}', ?, ?)",
        )
        .bind(id)
        .bind(session)
        .bind(seq)
        .bind(at)
        .execute(&mut conn)
        .await
        .unwrap();
    }
    conn.close().await.unwrap();

    let store = SqliteProfileStore::connect_encrypted(&path, &key)
        .await
        .expect("migrates to the current version");
    let read = |session: &'static str| {
        let pool = store.pool.clone();
        async move {
            sqlx::query(
                "SELECT id, sequence FROM chat_messages WHERE session_id = ? ORDER BY sequence",
            )
            .bind(session)
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|r| (r.get::<String, _>("id"), r.get::<i64, _>("sequence")))
            .collect::<Vec<_>>()
        }
    };
    let s = |id: &str, n: i64| (id.to_string(), n);
    assert_eq!(
        read("sess-a").await,
        vec![s("a-1", 0), s("a-2", 1), s("a-3", 2), s("a-4", 3)]
    );
    assert_eq!(read("sess-b").await, vec![s("b-1", 0), s("b-2", 5)]);

    let dup = sqlx::query(
        "INSERT INTO chat_messages (id, session_id, role, content_type, content, sequence, created_at) \
         VALUES ('dup', 'sess-b', 'user', 'text', '{}', 5, '2026-01-02')",
    )
    .execute(&store.pool)
    .await;
    assert!(
        dup.is_err(),
        "duplicate (session_id, sequence) must be rejected"
    );
    store.pool.close().await;
}
