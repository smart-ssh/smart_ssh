//! Issue #113: der Startschritt — Hinweis genau einmal, sichtbarer Abbruch
//! bei einem Fehler.

use std::sync::Mutex;

use chrono::Utc;

use persistence_sqlite::test_support::{
    field_encrypted_blob_count, insert_legacy_prompt_history_row,
};
use persistence_sqlite::{ConnectFailureKind, SqliteProfileStore};
use ssh_manager_core::crypto::legacy_field_content::encrypt_for_tests;
use ssh_manager_core::crypto::DatabaseKey;
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use super::decrypt_field_encrypted_content;
use crate::database_startup::{
    NewMasterPassword, StartupAbort, StartupChoice, StartupDialog, StartupPrompt,
};

const K: [u8; 32] = [0x33; 32];

/// Zeichnet nur den Hinweis auf; jede Frage wäre hier ein Fehler.
#[derive(Default)]
struct NoticeRecorder {
    removed: Mutex<Vec<u64>>,
}

impl StartupPrompt for NoticeRecorder {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        panic!("no dialog expected, got {dialog:?}");
    }
    fn confirm_start_over(&self, _renamed_to: Option<&str>) -> bool {
        panic!("no confirmation expected");
    }
    fn confirm_generate_new_key(&self) -> bool {
        panic!("no confirmation expected");
    }
    fn notify_started_over(&self, renamed_to: &str) {
        panic!("no start-over expected, got {renamed_to}");
    }
    fn notify_unreadable_history_removed(&self, removed: u64) {
        self.removed.lock().unwrap().push(removed);
    }
    fn ask_for_new_master_password(&self) -> Option<NewMasterPassword> {
        panic!("no master password expected");
    }
    fn can_ask_for_a_password(&self) -> bool {
        false
    }
}

async fn store_with_server(dir: &std::path::Path) -> (SqliteProfileStore, ServerId) {
    let store = SqliteProfileStore::connect_encrypted(
        &dir.join("smart-ssh.db"),
        &DatabaseKey::from_root_key(&ssh_manager_core::crypto::RootKey::for_tests(K)),
    )
    .await
    .expect("test database opens");
    let now = Utc::now();
    let server = Server {
        id: ServerId::new(),
        name: "Test-Server".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
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
    store.create_server(&server).await.unwrap();
    (store, server.id)
}

/// Entfernte Einträge → genau ein Hinweis mit ihrer Anzahl; ein zweiter
/// Start zeigt ihn nicht noch einmal.
#[tokio::test(flavor = "multi_thread")]
async fn test_removed_entries_are_announced_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let (store, server_id) = store_with_server(dir.path()).await;
    insert_legacy_prompt_history_row(&store, &server_id, encrypt_for_tests(&K, "lesbar")).await;
    for _ in 0..2 {
        insert_legacy_prompt_history_row(&store, &server_id, encrypt_for_tests(&[9u8; 32], "x"))
            .await;
    }
    let prompt = NoticeRecorder::default();

    decrypt_field_encrypted_content(
        &store,
        &ssh_manager_core::crypto::RootKey::for_tests(K),
        &prompt,
    )
    .await
    .expect("conversion succeeds");
    decrypt_field_encrypted_content(
        &store,
        &ssh_manager_core::crypto::RootKey::for_tests(K),
        &prompt,
    )
    .await
    .expect("second start succeeds");

    assert_eq!(*prompt.removed.lock().unwrap(), vec![2]);
    assert_eq!(field_encrypted_blob_count(&store).await, 0);
    assert_eq!(
        store.prompt_history_store().list(&server_id).await.unwrap(),
        vec!["lesbar".to_string()]
    );
    store.close().await;
}

/// Nichts entfernt → kein Hinweis.
#[tokio::test(flavor = "multi_thread")]
async fn test_no_notice_when_every_entry_was_readable() {
    let dir = tempfile::tempdir().unwrap();
    let (store, server_id) = store_with_server(dir.path()).await;
    insert_legacy_prompt_history_row(&store, &server_id, encrypt_for_tests(&K, "lesbar")).await;
    let prompt = NoticeRecorder::default();

    decrypt_field_encrypted_content(
        &store,
        &ssh_manager_core::crypto::RootKey::for_tests(K),
        &prompt,
    )
    .await
    .expect("conversion succeeds");

    assert!(prompt.removed.lock().unwrap().is_empty());
    store.close().await;
}

/// Ein Fehler der Umstellung beendet den Start sichtbar mit eigenem Fall —
/// kein Weiterlauf, kein Hinweis.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_failed_conversion_stops_the_start_visibly() {
    let dir = tempfile::tempdir().unwrap();
    let (store, server_id) = store_with_server(dir.path()).await;
    insert_legacy_prompt_history_row(&store, &server_id, encrypt_for_tests(&K, "lesbar")).await;
    // Ein geschlossener Pool lässt jeden Zugriff scheitern.
    store.close().await;
    let prompt = NoticeRecorder::default();

    let result = decrypt_field_encrypted_content(
        &store,
        &ssh_manager_core::crypto::RootKey::for_tests(K),
        &prompt,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(StartupAbort::Fatal {
                kind: ConnectFailureKind::FieldContentDecryptionFailed,
                ..
            })
        ),
        "{result:?}"
    );
    assert!(prompt.removed.lock().unwrap().is_empty());
}
