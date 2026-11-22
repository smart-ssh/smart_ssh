//! Activity record foundation (Spec 0057, issue #1443): migration 0029 on a
//! filled database, recorded sessions outliving chats and servers, atomic
//! ledger appends, old ledger rows reading with default meta fields.

use sqlx::{Connection, Row};
use uuid::Uuid;

use ssh_manager_core::audit::{LedgerEntryContent, LedgerEntryMeta, LedgerSource};
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::test_support::{migrate_up_to, open_raw};
use crate::SqliteProfileStore;

const OLD_CONTENT: &str = r#"{"CommandProposed":{"command":"uptime"}}"#;

async fn exec(conn: &mut sqlx::SqliteConnection, sql: &'static str) {
    sqlx::query(sql)
        .execute(conn)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// A database at migration 0028 with a group tree, servers, tags, 3 chat
/// sessions, messages and ledger rows.
async fn database_at_0028() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite3");
    std::fs::File::create(&path).unwrap();
    migrate_up_to(&path, None, 28).await;
    let mut c = open_raw(&path, None).await;
    exec(&mut c, "INSERT INTO groups (id,name,parent_id,created_at,updated_at) VALUES ('g1','Prod',NULL,'t','t'),('g2','EU','g1','t','t')").await;
    exec(&mut c, "INSERT INTO servers (id,name,host,port,username,group_id,auth_method,created_at,updated_at) VALUES ('s1','Web','web.example',2222,'deploy','g2','\"Agent\"','t','t'),('s2','DB','db.example',22,'root',NULL,'\"Agent\"','t','t')").await;
    exec(
        &mut c,
        "INSERT INTO server_tags (server_id,tag) VALUES ('s1','b-tag'),('s1','a-tag')",
    )
    .await;
    exec(&mut c, "INSERT INTO chat_sessions (id,server_id,started_at,ended_at) VALUES ('00000000-0000-4000-8000-0000000000c1','s1','2026-01-01T00:00:00+00:00','2026-01-01T01:00:00+00:00'),('00000000-0000-4000-8000-0000000000c2','s1','2026-02-01T00:00:00+00:00',NULL),('00000000-0000-4000-8000-0000000000c3','s2','2026-03-01T00:00:00+00:00',NULL)").await;
    exec(&mut c, "INSERT INTO chat_messages (id,session_id,role,content_type,content,sequence,created_at) VALUES ('m1','00000000-0000-4000-8000-0000000000c1','user','text','\"hi\"',0,'t'),('m2','00000000-0000-4000-8000-0000000000c2','user','text','\"yo\"',0,'t')").await;
    for (id, session, seq) in [
        ("l1", "00000000-0000-4000-8000-0000000000c1", 0),
        ("l2", "00000000-0000-4000-8000-0000000000c1", 1),
        ("l3", "00000000-0000-4000-8000-0000000000c3", 0),
    ] {
        sqlx::query("INSERT INTO ledger_entries (id,session_id,sequence,source,entry_type,content,created_at) VALUES (?,?,?,'ai','command_proposed',?,'2026-01-01T00:00:00+00:00')")
            .bind(id).bind(session).bind(seq).bind(OLD_CONTENT)
            .execute(&mut c).await.unwrap();
    }
    c.close().await.unwrap();
    (dir, path)
}

async fn count(store: &SqliteProfileStore, table: &str) -> i64 {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) AS c FROM {table}"
    )))
    .fetch_one(&store.pool)
    .await
    .unwrap()
    .get("c")
}

#[tokio::test]
async fn test_migration_0029_keeps_rows_and_links_snapshots() {
    let (_dir, path) = database_at_0028().await;
    let store = SqliteProfileStore::connect_plaintext(&path).await.unwrap();

    assert_eq!(count(&store, "chat_sessions").await, 3);
    assert_eq!(count(&store, "chat_messages").await, 2);
    assert_eq!(count(&store, "ledger_entries").await, 3);
    assert_eq!(count(&store, "servers").await, 2);
    assert_eq!(count(&store, "groups").await, 2);

    let rows = sqlx::query("SELECT id, session_id, sequence, source, entry_type, CAST(content AS TEXT) AS content, created_at, redaction_incomplete, action_id FROM ledger_entries ORDER BY id")
        .fetch_all(&store.pool).await.unwrap();
    let got: Vec<(String, String, i64)> = rows
        .iter()
        .map(|r| (r.get("id"), r.get("session_id"), r.get("sequence")))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "l1".into(),
                "00000000-0000-4000-8000-0000000000c1".into(),
                0
            ),
            (
                "l2".into(),
                "00000000-0000-4000-8000-0000000000c1".into(),
                1
            ),
            (
                "l3".into(),
                "00000000-0000-4000-8000-0000000000c3".into(),
                0
            )
        ]
    );
    for r in &rows {
        assert_eq!(r.get::<String, _>("content"), OLD_CONTENT);
        assert_eq!(
            r.get::<String, _>("created_at"),
            "2026-01-01T00:00:00+00:00"
        );
        assert_eq!(r.get::<String, _>("entry_type"), "command_proposed");
        assert_eq!(r.get::<i64, _>("redaction_incomplete"), 0);
        assert_eq!(r.get::<Option<String>, _>("action_id"), None);
    }

    // Every chat session has exactly one linked recorded session with the
    // snapshot of its server.
    let links: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id, activity_session_id FROM chat_sessions ORDER BY id")
            .fetch_all(&store.pool)
            .await
            .unwrap();
    assert_eq!(links.len(), 3);
    assert_eq!(count(&store, "activity_sessions").await, 3);
    for (_, link) in &links {
        assert!(link.is_some(), "every chat session is linked");
    }
    let row = sqlx::query(
        "SELECT * FROM activity_sessions WHERE id = '00000000-0000-4000-8000-0000000000c1'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("kind"), "chat");
    assert_eq!(row.get::<String, _>("server_id"), "s1");
    assert_eq!(row.get::<String, _>("server_name"), "Web");
    assert_eq!(row.get::<String, _>("server_host"), "web.example");
    assert_eq!(row.get::<i64, _>("server_port"), 2222);
    assert_eq!(row.get::<String, _>("server_user"), "deploy");
    assert_eq!(row.get::<String, _>("group_ids"), r#"["g1","g2"]"#);
    assert_eq!(row.get::<String, _>("group_names"), r#"["Prod","EU"]"#);
    assert_eq!(row.get::<String, _>("tags"), r#"["a-tag","b-tag"]"#);
    assert_eq!(row.get::<Option<String>, _>("time_zone"), None);
    assert_eq!(
        row.get::<String, _>("started_at"),
        "2026-01-01T00:00:00+00:00"
    );
    assert_eq!(
        row.get::<Option<String>, _>("ended_at").as_deref(),
        Some("2026-01-01T01:00:00+00:00")
    );

    // The server without a group gets empty group lists.
    let ungrouped = sqlx::query(
        "SELECT group_ids, group_names FROM activity_sessions \
         WHERE id = '00000000-0000-4000-8000-0000000000c3'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(ungrouped.get::<String, _>("group_ids"), "[]");
    assert_eq!(ungrouped.get::<String, _>("group_names"), "[]");

    // Every link points at an existing recorded session.
    let dangling: i64 = sqlx::query(
        "SELECT COUNT(*) AS c FROM chat_sessions c \
         WHERE c.activity_session_id IS NULL OR NOT EXISTS \
         (SELECT 1 FROM activity_sessions a WHERE a.id = c.activity_session_id)",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap()
    .get("c");
    assert_eq!(dangling, 0);
}

#[tokio::test]
async fn test_old_ledger_row_deserializes_with_default_meta() {
    let (_dir, path) = database_at_0028().await;
    let store = SqliteProfileStore::connect_plaintext(&path).await.unwrap();
    let entries = store
        .ledger_store()
        .load_entries(Uuid::parse_str("00000000-0000-4000-8000-0000000000c1").unwrap())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].content,
        LedgerEntryContent::CommandProposed {
            command: "uptime".to_string()
        }
    );
    assert_eq!(entries[0].meta, LedgerEntryMeta::default());
}

#[test]
fn test_old_meta_json_without_new_fields_uses_defaults() {
    let meta: LedgerEntryMeta = serde_json::from_str("{}").unwrap();
    assert_eq!(meta, LedgerEntryMeta::default());
}

fn test_server(name: &str) -> Server {
    let now = chrono::Utc::now();
    Server {
        id: ServerId::new(),
        name: name.to_string(),
        host: "snap.example".to_string(),
        port: 22,
        username: "ops".to_string(),
        group_id: None,
        tags: vec!["t1".to_string()],
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        elevation_allowed: true,
        start_directory: None,
        created_at: now,
        updated_at: now,
    }
}

async fn store_with_chat() -> (SqliteProfileStore, Server, Uuid) {
    let store = SqliteProfileStore::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"),
    )
    .await
    .unwrap();
    let server = test_server("Snapshot-Server");
    store.create_server(&server).await.unwrap();
    let chat = store
        .chat_session_store()
        .create_session(&server.id, None)
        .await
        .unwrap();
    (store, server, chat)
}

fn tick_content() -> LedgerEntryContent {
    LedgerEntryContent::CommandProposed {
        command: "ls".to_string(),
    }
}

#[tokio::test]
async fn test_new_chat_session_creates_and_links_recorded_session() {
    let (store, server, chat) = store_with_chat().await;
    let linked: Option<String> =
        sqlx::query_scalar("SELECT activity_session_id FROM chat_sessions WHERE id = ?")
            .bind(chat.to_string())
            .fetch_one(&store.pool)
            .await
            .unwrap();
    let linked = Uuid::parse_str(&linked.expect("linked")).unwrap();
    let session = store
        .activity_session_store()
        .load(linked)
        .await
        .unwrap()
        .expect("recorded session exists");
    assert_eq!(session.kind, "chat");
    assert_eq!(session.server_name, server.name);
    assert_eq!(session.tags, vec!["t1".to_string()]);
}

#[tokio::test]
async fn test_deleting_a_chat_keeps_ledger_and_recorded_session() {
    let (store, _server, chat) = store_with_chat().await;
    store
        .ledger_store()
        .append_entry(chat, LedgerSource::Ai, &tick_content())
        .await
        .unwrap();
    store
        .chat_session_store()
        .delete_session(chat)
        .await
        .unwrap();
    assert_eq!(count(&store, "ledger_entries").await, 1);
    assert_eq!(count(&store, "activity_sessions").await, 1);
}

#[tokio::test]
async fn test_deleting_a_server_keeps_record_and_snapshot() {
    let (store, server, chat) = store_with_chat().await;
    store
        .ledger_store()
        .append_entry(chat, LedgerSource::Ai, &tick_content())
        .await
        .unwrap();
    store.delete_server(&server.id).await.unwrap();
    assert_eq!(count(&store, "chat_sessions").await, 0);
    assert_eq!(count(&store, "ledger_entries").await, 1);
    let session = store
        .activity_session_store()
        .load(chat)
        .await
        .unwrap()
        .expect("recorded session survives");
    assert_eq!(session.server_name, "Snapshot-Server");
    assert_eq!(session.server_host, "snap.example");
}

#[tokio::test]
async fn test_empty_chat_discard_drops_its_unused_recorded_session() {
    let (store, _server, chat) = store_with_chat().await;
    assert!(store
        .chat_session_store()
        .delete_if_empty(chat)
        .await
        .unwrap());
    assert_eq!(count(&store, "activity_sessions").await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_appends_get_distinct_sequences() {
    let (store, _server, chat) = store_with_chat().await;
    let ledger = store.ledger_store();
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let ledger = ledger.clone();
        tasks.push(tokio::spawn(async move {
            ledger
                .append_entry(chat, LedgerSource::McpAgent, &tick_content())
                .await
        }));
    }
    for t in tasks {
        t.await.unwrap().expect("append succeeds");
    }
    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT sequence FROM ledger_entries WHERE session_id = ? ORDER BY sequence",
    )
    .bind(chat.to_string())
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(sequences, (0..16).collect::<Vec<i64>>());
}

#[tokio::test]
async fn test_failed_append_counts_and_new_entry_types_round_trip() {
    let (store, _server, chat) = store_with_chat().await;
    let ledger = store.ledger_store();
    // Unknown session: constraint failure, nothing to count against.
    assert!(ledger
        .append_entry(Uuid::new_v4(), LedgerSource::Ai, &tick_content())
        .await
        .is_err());
    // A failure on a real session increments its counter (the insert is
    // made to fail by a trigger).
    sqlx::query("CREATE TRIGGER fail_ledger BEFORE INSERT ON ledger_entries BEGIN SELECT RAISE(ABORT, 'boom'); END")
        .execute(&store.pool)
        .await
        .unwrap();
    assert!(ledger
        .append_entry(chat, LedgerSource::Ai, &tick_content())
        .await
        .is_err());
    sqlx::query("DROP TRIGGER fail_ledger")
        .execute(&store.pool)
        .await
        .unwrap();
    let session = store
        .activity_session_store()
        .load(chat)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.failed_writes, 1);

    let meta = LedgerEntryMeta {
        action_id: Some("a-1".to_string()),
        redaction_incomplete: true,
        rule_snapshot: None,
    };
    for content in [
        LedgerEntryContent::ActivityTick,
        LedgerEntryContent::FileRead {
            path: "/etc/hosts".to_string(),
            bytes: Some(10),
            sha256: None,
        },
    ] {
        ledger
            .append(chat, LedgerSource::Ai, &content, &meta)
            .await
            .unwrap();
    }
    let loaded = ledger.load_entries(chat).await.unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].content, LedgerEntryContent::ActivityTick);
    assert_eq!(loaded[0].meta, meta);
}

/// A cyclic `groups.parent_id` chain must not hang the migration backfill.
#[tokio::test(flavor = "multi_thread")]
async fn test_migration_0029_terminates_on_cyclic_group_chain() {
    let (_dir, path) = database_at_0028().await;
    let mut c = open_raw(&path, None).await;
    exec(&mut c, "PRAGMA foreign_keys = OFF").await;
    exec(&mut c, "UPDATE groups SET parent_id = 'g2' WHERE id = 'g1'").await;
    c.close().await.unwrap();
    let store = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        SqliteProfileStore::connect_plaintext(&path),
    )
    .await
    .expect("migration must terminate")
    .unwrap();
    assert_eq!(count(&store, "activity_sessions").await, 3);
    let row = sqlx::query(
        "SELECT group_ids, group_names FROM activity_sessions \
         WHERE id = '00000000-0000-4000-8000-0000000000c1'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("group_ids"), r#"["g1","g2"]"#);
    assert_eq!(row.get::<String, _>("group_names"), r#"["Prod","EU"]"#);
}

/// Creating a chat on a server below a cyclic group chain terminates.
#[tokio::test(flavor = "multi_thread")]
async fn test_create_session_terminates_on_cyclic_group_chain() {
    let store = SqliteProfileStore::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"),
    )
    .await
    .unwrap();
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO groups (id,name,parent_id,created_at,updated_at) VALUES ('g1','A','g2','t','t'),('g2','B','g1','t','t')")
        .execute(&store.pool)
        .await
        .unwrap();
    let mut server = test_server("Cyclic");
    store.create_server(&server).await.unwrap();
    server.group_id = None;
    sqlx::query("UPDATE servers SET group_id = 'g1' WHERE id = ?")
        .bind(server.id.0.to_string())
        .execute(&store.pool)
        .await
        .unwrap();
    let chat = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        store.chat_session_store().create_session(&server.id, None),
    )
    .await
    .expect("create_session must terminate")
    .unwrap();
    let snap = store
        .activity_session_store()
        .load(chat)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap.group_ids, vec!["g2", "g1"]);
    assert_eq!(snap.group_names, vec!["B", "A"]);
}

async fn corrupt_load(column: &str, value: &str) -> String {
    let (store, _server, chat) = store_with_chat().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE activity_sessions SET {column} = ? WHERE id = ?"
    )))
    .bind(value)
    .bind(chat.to_string())
    .execute(&store.pool)
    .await
    .unwrap();
    let err = store
        .activity_session_store()
        .load(chat)
        .await
        .expect_err("corrupt snapshot must fail to load");
    assert!(matches!(
        err,
        crate::ledger_store::LedgerStoreError::CorruptSession { .. }
    ));
    assert_eq!(
        ssh_manager_core::ipc_error::ErrorCode::code(&err),
        crate::error_kind::StoreErrorKind::Corrupt.code()
    );
    let msg = err.to_string();
    assert!(
        msg.contains("Sitzung") && !msg.contains("Ledger-Eintrag"),
        "{msg}"
    );
    assert!(msg.contains(&chat.to_string()), "{msg}");
    msg
}

#[tokio::test(flavor = "multi_thread")]
async fn test_load_reports_corrupt_group_ids() {
    assert!(corrupt_load("group_ids", "not json")
        .await
        .contains("group_ids"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_load_reports_corrupt_group_names() {
    assert!(corrupt_load("group_names", "{")
        .await
        .contains("group_names"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_load_reports_corrupt_tags() {
    assert!(corrupt_load("tags", "[1]").await.contains("tags"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_load_reports_out_of_range_port() {
    let (store, _server, chat) = store_with_chat().await;
    for bad in [65536_i64, -1] {
        sqlx::query("UPDATE activity_sessions SET server_port = ? WHERE id = ?")
            .bind(bad)
            .bind(chat.to_string())
            .execute(&store.pool)
            .await
            .unwrap();
        let err = store.activity_session_store().load(chat).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("server_port"), "{msg}");
        assert!(
            msg.contains("Sitzung") && !msg.contains("Ledger-Eintrag"),
            "{msg}"
        );
        assert_eq!(
            ssh_manager_core::ipc_error::ErrorCode::code(&err),
            crate::error_kind::StoreErrorKind::Corrupt.code()
        );
    }
}

#[tokio::test]
async fn test_restore_as_copy_moves_activity_record_to_the_server_copy() {
    use crate::backup_store::{DatabaseSnapshot, ImportPlan};
    let (store, server, chat) = store_with_chat().await;
    // A record of a server that is not part of the archive (already deleted).
    let gone = Uuid::new_v4();
    sqlx::query("INSERT INTO activity_sessions (id,kind,server_id,server_name,server_host,server_port,server_user,started_at) VALUES (?, 'mcp', ?, 'Gone', 'gone.example', 22, 'u', 't')")
        .bind(Uuid::new_v4().to_string())
        .bind(gone.to_string())
        .execute(&store.pool)
        .await
        .unwrap();

    store
        .backup_store()
        .apply_import(&ImportPlan {
            data: DatabaseSnapshot::default(),
            archive_folder: Some("Previous data".into()),
            now: "2026-01-01T00:00:00+00:00".into(),
            pending_host_keys: "[]".into(),
        })
        .await
        .unwrap();

    let chat_server: String =
        sqlx::query_scalar("SELECT server_id FROM chat_sessions WHERE id = ?")
            .bind(chat.to_string())
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_ne!(chat_server, server.id.0.to_string());
    let row = sqlx::query(
        "SELECT server_id, server_name, server_host FROM activity_sessions WHERE id = ?",
    )
    .bind(chat.to_string())
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("server_id"), chat_server);
    assert_eq!(row.get::<String, _>("server_name"), "Snapshot-Server");
    assert_eq!(row.get::<String, _>("server_host"), "snap.example");
    let other: String =
        sqlx::query_scalar("SELECT server_id FROM activity_sessions WHERE server_name = 'Gone'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(other, gone.to_string());
}
