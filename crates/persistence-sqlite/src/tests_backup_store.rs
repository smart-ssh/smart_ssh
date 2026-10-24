//! Issue #664 (Spec 0109 C1, C2, X2, K4): the backup snapshot and the
//! reserved backup-key entry.

use secrecy::SecretString;
use uuid::Uuid;

use crate::{SqliteProfileStore, BACKUP_KEY_REF};

const SERVER: &str = "11111111-1111-4111-8111-111111111111";
const GROUP: &str = "22222222-2222-4222-8222-222222222222";
const RULE: &str = "33333333-3333-4333-8333-333333333333";
const NOTE: &str = "44444444-4444-4444-8444-444444444444";
const PROVIDER: &str = "55555555-5555-4555-8555-555555555555";

async fn exec(store: &SqliteProfileStore, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&store.pool)
        .await
        .unwrap();
}

async fn seeded(dir: &std::path::Path) -> SqliteProfileStore {
    let store = SqliteProfileStore::connect_plaintext(&dir.join("t.db"))
        .await
        .unwrap();
    let nil = Uuid::nil();
    for sql in [
        format!("INSERT INTO groups (id,name,notes,created_at,updated_at) VALUES ('{GROUP}','g','','t','t')"),
        format!("INSERT INTO servers (id,name,host,port,username,group_id,auth_method,notes,created_at,updated_at) VALUES ('{SERVER}','srv','h',22,'u','{GROUP}','{{\"type\":\"agent\"}}','','t','t')"),
        format!("INSERT INTO servers (id,name,host,port,username,auth_method,notes,created_at,updated_at) VALUES ('{nil}','local','l',22,'u','{{}}','','t','t')"),
        format!("INSERT INTO server_tags (server_id,tag) VALUES ('{SERVER}','prod')"),
        format!("INSERT INTO filter_rules (id,pattern_type,pattern_value,action,scope_type,scope_value,priority,created_at,updated_at) VALUES ('{RULE}','glob','rm *','deny','server','{SERVER}',1,'t','t')"),
        format!("INSERT INTO filter_rules (id,pattern_type,pattern_value,action,scope_type,scope_value,priority,created_at,updated_at) VALUES ('{}','glob','x','deny','server','{nil}',1,'t','t')", Uuid::new_v4()),
        format!("INSERT INTO note_revisions (id,target_type,target_id,content,editor_type,created_at) VALUES ('{NOTE}','server','{SERVER}','n','user','t')"),
        format!("INSERT INTO ai_provider_configs (id,provider_type,display_name,model,credential_ref,created_at,updated_at) VALUES ('{PROVIDER}','openai','p','m','ai-provider:{PROVIDER}','t','t')"),
        format!("INSERT INTO secrets (ref,value,updated_at) VALUES ('server:{SERVER}:password','pw','t')"),
        format!("INSERT INTO secrets (ref,value,updated_at) VALUES ('ai-provider:{PROVIDER}','sk','t')"),
        format!("INSERT INTO chat_sessions (id,server_id,started_at) VALUES ('{}','{SERVER}','t')", Uuid::new_v4()),
        "CREATE TABLE ext_demo_items (id INTEGER PRIMARY KEY, n TEXT)".into(),
        "INSERT INTO ext_demo_items (n) VALUES ('secret-ext')".into(),
    ] {
        exec(&store, &sql).await;
    }
    store
        .backup_store()
        .store_backup_key(&SecretString::from("KEYBLOB-marker".to_string()))
        .await
        .unwrap();
    store
}

#[tokio::test]
async fn snapshot_has_every_c1_row_and_no_c2_data() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path()).await;
    let snap = store.backup_store().read_snapshot().await.unwrap();
    assert_eq!(snap.groups.len(), 1);
    assert_eq!(snap.servers.len(), 1, "the local pseudo-server is excluded");
    assert_eq!(snap.servers[0].id.to_string(), SERVER);
    assert_eq!(snap.tags.len(), 1);
    assert_eq!(
        snap.rules.len(),
        1,
        "the rule of the local pseudo-server is excluded"
    );
    assert_eq!(snap.notes.len(), 1);
    assert_eq!(snap.ai_providers.len(), 1);
    let refs: Vec<&str> = snap.secrets.iter().map(|s| s.owner_ref.as_str()).collect();
    assert_eq!(refs.len(), 2);
    assert!(
        !refs.contains(&BACKUP_KEY_REF),
        "C2: the backup key is not content"
    );
    let dump = format!("{:?}{:?}{:?}", snap.servers, snap.rules, snap.notes);
    assert!(!dump.contains("secret-ext") && !dump.contains("KEYBLOB"));
}

/// X2: a write between the reads of two tables does not appear partially.
/// The writer is started between the reads and runs on another task; it is
/// committed only after the snapshot transaction ended.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_is_one_consistent_cut() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path()).await;
    let new_group = "66666666-6666-4666-8666-666666666666";
    let new_server = "77777777-7777-4777-8777-777777777777";
    let backup = store.backup_store();
    let writer = std::cell::RefCell::new(None);
    let snap = backup
        .read_snapshot_with(|table| {
            if table == "groups" {
                let pool = store.pool.clone();
                *writer.borrow_mut() = Some(tokio::spawn(async move {
                    for sql in [
                        format!("INSERT INTO groups (id,name,notes,created_at,updated_at) VALUES ('{new_group}','late','','t','t')"),
                        format!("INSERT INTO servers (id,name,host,port,username,group_id,auth_method,notes,created_at,updated_at) VALUES ('{new_server}','late','h',22,'u','{new_group}','{{}}','','t','t')"),
                        format!("INSERT INTO secrets (ref,value,updated_at) VALUES ('server:{new_server}:password','late','t')"),
                    ] {
                        sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&pool).await.unwrap();
                    }
                }));
                // Give the writer every chance to get in between the reads.
                return futures_yield();
            }
            futures_yield()
        })
        .await
        .unwrap();
    assert_eq!(snap.groups.len(), 1);
    assert_eq!(
        snap.servers.len(),
        1,
        "the late server must not show up without its group"
    );
    assert_eq!(snap.secrets.len(), 2);
    let handle = writer.borrow_mut().take().unwrap();
    handle.await.unwrap();
    let again = backup.read_snapshot().await.unwrap();
    assert_eq!(
        again.servers.len(),
        2,
        "the writes did happen; they are in the next snapshot"
    );
    assert_eq!(again.groups.len(), 2);
}

async fn futures_yield() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

/// K4: the entry survives the secret handling that enumerates references.
#[tokio::test]
async fn backup_key_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path()).await;
    use secrecy::ExposeSecret;
    let loaded = store
        .backup_store()
        .load_backup_key()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.expose_secret(), "KEYBLOB-marker");
}
