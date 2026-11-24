//! Issue #904: every persisted server column survives
//! create (profile store) → backup snapshot → restore → read back.

use chrono::Utc;
use sqlx::sqlite::SqliteConnectOptions;

use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, Group, GroupId, PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::backup_store::{DatabaseSnapshot, ImportPlan};
use crate::server_columns::SERVER_COLUMNS;
use crate::SqliteProfileStore;

async fn memory_store() -> SqliteProfileStore {
    SqliteProfileStore::connect_with(SqliteConnectOptions::new().filename(":memory:"))
        .await
        .unwrap()
}

fn cred(s: &str) -> CredentialRef {
    CredentialRef::new(s)
}

fn auth_methods() -> Vec<AuthMethod> {
    vec![
        AuthMethod::Password {
            credential_ref: cred("server:x:password"),
        },
        AuthMethod::PrivateKey {
            credential_ref: cred("server:x:key"),
            passphrase_ref: Some(cred("server:x:passphrase")),
        },
        AuthMethod::Agent,
        AuthMethod::NoPassword,
        AuthMethod::Certificate {
            cert_ref: cred("server:x:cert"),
            key_ref: cred("server:x:key"),
        },
        AuthMethod::IdentityFile {
            path: "~/.ssh/id_ed25519".into(),
            passphrase_ref: Some(cred("server:x:passphrase")),
        },
    ]
}

/// Every field set to a value that differs from its default.
fn full_server(auth: AuthMethod, group: GroupId, jump: ServerId) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: "all-fields".into(),
        host: "all.example.com".into(),
        port: 2222,
        username: "root".into(),
        group_id: Some(group),
        tags: vec!["alpha".into(), "beta".into()],
        auth,
        notes: "some notes".into(),
        jump_host: Some(jump),
        post_ingest_policy: PostIngestPolicy::Strict,
        ai_injection_check_enabled: true,
        sftp_server_path: Some("/usr/lib/sftp-server".into()),
        elevation_allowed: true,
        start_directory: Some("/srv/app".into()),
        created_at: now - chrono::Duration::days(3),
        updated_at: now,
    }
}

async fn by_id(snap: &DatabaseSnapshot, id: ServerId) -> crate::backup_store::ServerRow {
    snap.servers.iter().find(|s| s.id == id.0).unwrap().clone()
}

#[tokio::test]
async fn every_server_field_survives_create_backup_restore() {
    assert_ne!(PostIngestPolicy::Strict, PostIngestPolicy::default());
    for auth in auth_methods() {
        let src = memory_store().await;
        let now = Utc::now();
        let group = Group {
            id: GroupId::new(),
            name: "g".into(),
            parent_id: None,
            notes: String::new(),
            created_at: now,
            updated_at: now,
        };
        src.create_group(&group).await.unwrap();
        let mut jump = full_server(AuthMethod::Agent, group.id, ServerId::new());
        jump.jump_host = None;
        jump.name = "jump".into();
        src.create_server(&jump).await.unwrap();
        let server = full_server(auth.clone(), group.id, jump.id);
        src.create_server(&server).await.unwrap();
        // The copy marker is not part of the domain type; set it as an import would.
        sqlx::query("UPDATE servers SET is_copy = 1 WHERE id = ?")
            .bind(server.id.0.to_string())
            .execute(&src.pool)
            .await
            .unwrap();

        let snap = src.backup_store().read_snapshot().await.unwrap();
        assert!(by_id(&snap, server.id).await.is_copy);

        let dst = memory_store().await;
        dst.backup_store()
            .apply_import(&ImportPlan {
                data: snap.clone(),
                archive_folder: None,
                now: now.to_rfc3339(),
                pending_host_keys: "[]".into(),
            })
            .await
            .unwrap();

        assert_eq!(
            dst.get_server(&server.id).await.unwrap(),
            server,
            "{auth:?}"
        );
        assert_eq!(dst.get_server(&jump.id).await.unwrap(), jump);
        let restored = dst.backup_store().read_snapshot().await.unwrap();
        assert_eq!(
            by_id(&restored, server.id).await,
            by_id(&snap, server.id).await
        );
    }
}

#[tokio::test]
async fn update_server_writes_every_editable_column_and_keeps_the_copy_marker() {
    let store = memory_store().await;
    let now = Utc::now();
    let group = Group {
        id: GroupId::new(),
        name: "g".into(),
        parent_id: None,
        notes: String::new(),
        created_at: now,
        updated_at: now,
    };
    store.create_group(&group).await.unwrap();
    let mut jump = full_server(AuthMethod::Agent, group.id, ServerId::new());
    jump.jump_host = None;
    store.create_server(&jump).await.unwrap();
    let mut server = full_server(AuthMethod::Agent, group.id, jump.id);
    server.jump_host = None;
    server.ai_injection_check_enabled = false;
    server.post_ingest_policy = PostIngestPolicy::default();
    server.sftp_server_path = None;
    server.start_directory = None;
    server.group_id = None;
    store.create_server(&server).await.unwrap();
    sqlx::query("UPDATE servers SET is_copy = 1")
        .execute(&store.pool)
        .await
        .unwrap();

    let mut changed = full_server(AuthMethod::NoPassword, group.id, jump.id);
    changed.id = server.id;
    changed.created_at = server.created_at;
    store.update_server(&changed).await.unwrap();
    assert_eq!(store.get_server(&server.id).await.unwrap(), changed);
    let snap = store.backup_store().read_snapshot().await.unwrap();
    assert!(by_id(&snap, server.id).await.is_copy);
}

#[test]
fn column_list_is_unique_and_starts_with_the_key() {
    assert_eq!(SERVER_COLUMNS[0], "id");
    let mut sorted = SERVER_COLUMNS.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), SERVER_COLUMNS.len());
}

/// Issue #1194: a server row that predates the column (or a plain create with
/// the default) reads as allowed; the column default is `1`.
#[tokio::test]
async fn elevation_allowed_defaults_to_true_for_rows_without_a_value() {
    let store = memory_store().await;
    let mut server = full_server(AuthMethod::Agent, GroupId::new(), ServerId::new());
    server.group_id = None;
    server.jump_host = None;
    server.elevation_allowed = true;
    store.create_server(&server).await.unwrap();
    sqlx::query("ALTER TABLE servers DROP COLUMN elevation_allowed")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE servers ADD COLUMN elevation_allowed INTEGER NOT NULL DEFAULT 1")
        .execute(&store.pool)
        .await
        .unwrap();
    assert!(
        store
            .get_server(&server.id)
            .await
            .unwrap()
            .elevation_allowed
    );
}

/// Issue #1194: a backup written before the field existed restores as allowed.
#[tokio::test]
async fn backup_row_without_elevation_allowed_deserializes_as_true() {
    let store = memory_store().await;
    let mut server = full_server(AuthMethod::Agent, GroupId::new(), ServerId::new());
    server.group_id = None;
    server.jump_host = None;
    server.elevation_allowed = false;
    store.create_server(&server).await.unwrap();
    let snap = store.backup_store().read_snapshot().await.unwrap();
    let mut value = serde_json::to_value(by_id(&snap, server.id).await).unwrap();
    assert_eq!(value["elevation_allowed"], false);
    value.as_object_mut().unwrap().remove("elevation_allowed");
    let row: crate::backup_store::ServerRow = serde_json::from_value(value).unwrap();
    assert!(row.elevation_allowed);
}
