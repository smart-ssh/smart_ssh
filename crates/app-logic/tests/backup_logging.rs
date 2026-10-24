//! Issue #664, S-7 (Spec 0094): key handling and export log events and counts
//! only. Its own test binary: the subscriber is process-wide state.

use std::io::Write;
use std::sync::{Arc, Mutex};

use app_logic::backup::{
    export_backup, start_import, BackupKey, ImportContext, ImportStart, NoSessions,
};
use app_logic::host_key_store::FileHostKeyStore;
use chrono::Utc;
use persistence_sqlite::SqliteProfileStore;
use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, CredentialStore, PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

const SERVER_NAME: &str = "prod-db-marker";
const PASSWORD: &str = "pw-marker-0664";

fn pass(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}

async fn fixture() -> (tempfile::TempDir, SqliteProfileStore, FileHostKeyStore) {
    fixture_named(SERVER_NAME, PASSWORD).await
}

async fn fixture_named(
    name: &str,
    password: &str,
) -> (tempfile::TempDir, SqliteProfileStore, FileHostKeyStore) {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
        .await
        .unwrap();
    let now = Utc::now();
    let sid = ServerId::new();
    let r = CredentialRef::new(format!("server:{}:password", sid.0));
    db.create_server(&Server {
        id: sid,
        name: name.into(),
        host: "example.invalid".into(),
        port: 22,
        username: "deploy".into(),
        group_id: None,
        tags: vec![],
        auth: AuthMethod::Password {
            credential_ref: r.clone(),
        },
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();
    let credentials = db.credential_store(tokio::runtime::Handle::current());
    let password = password.to_string();
    tokio::task::spawn_blocking(move || credentials.set(&r, pass(&password)).unwrap())
        .await
        .unwrap();
    let host_keys = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();
    (dir, db, host_keys)
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// S-7: key creation and export log events and counts only.
#[tokio::test(flavor = "multi_thread")]
async fn logs_contain_no_secret_path_or_server_name() {
    let (dir, db, host_keys) = fixture().await;
    let backup = db.backup_store();
    // The test body itself runs on this thread, where the subscriber is set.
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buffer.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let code = BackupKey::create_with_code(&backup).await.unwrap();
    BackupKey::set_passphrase(&backup, pass("log passphrase 12"))
        .await
        .unwrap();
    let path = dir.path().join("secret-dir-marker.sshbackup");
    export_backup(&backup, &host_keys, &path, "t")
        .await
        .unwrap();
    let _ = export_backup(&backup, &host_keys, &path, "t").await;

    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("backup export succeeded"), "log: {log}");
    for needle in [
        PASSWORD,
        SERVER_NAME,
        "log passphrase 12",
        "secret-dir-marker",
        &code.display_string()[..5],
    ] {
        assert!(!log.contains(needle), "log leaks {needle:?}: {log}");
    }
    import_logs_contain_no_secret_path_or_server_name(&buffer).await;
}

/// Test 19 / S-7, import side (run by the test above: one process-wide
/// subscriber, see there): event and counts only, for an accepted and a
/// rejected file.
async fn import_logs_contain_no_secret_path_or_server_name(buffer: &LogBuffer) {
    let (dir_a, db_a, hk_a) = fixture_named("file-server-marker", "file-pw-marker").await;
    let backup_a = db_a.backup_store();
    let code = BackupKey::create_with_code(&backup_a).await.unwrap();
    let file = dir_a.path().join("import-dir-marker.sshbackup");
    export_backup(&backup_a, &hk_a, &file, "t").await.unwrap();

    let (dir_b, db_b, hk_b) = fixture_named("local-server-marker", "local-pw-marker").await;
    let backup_b = db_b.backup_store();
    BackupKey::create_with_code(&backup_b).await.unwrap();
    let safety = dir_b.path().join("safety-dir-marker");

    buffer.0.lock().unwrap().clear();
    let ctx = ImportContext {
        store: &backup_b,
        host_keys: &hk_b,
        sessions: &NoSessions,
        safety_dir: &safety,
        folder_label: "Previous data",
        writer_version: "t",
    };
    let secret = || {
        ssh_manager_core::backup::BackupSecret::Code(
            ssh_manager_core::backup::RecoveryCode::parse(&code.display_string()).unwrap(),
        )
    };
    let ImportStart::NeedsConfirmation(pending) =
        start_import(&ctx, &file, secret()).await.unwrap()
    else {
        panic!("local data exists");
    };
    pending.confirm(&ctx).await.unwrap();
    let _ = start_import(
        &ctx,
        &file,
        ssh_manager_core::backup::BackupSecret::Passphrase(pass("wrong passphrase 1")),
    )
    .await;

    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("backup import succeeded"), "log: {log}");
    assert!(log.contains("backup import rejected"), "log: {log}");
    for needle in [
        "file-server-marker",
        "file-pw-marker",
        "local-server-marker",
        "local-pw-marker",
        "wrong passphrase 1",
        "import-dir-marker",
        "safety-dir-marker",
        &code.display_string()[..5],
    ] {
        assert!(!log.contains(needle), "log leaks {needle:?}: {log}");
    }
}
