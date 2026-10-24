//! Issue #664, S-7 (Spec 0094): key handling and export log events and counts
//! only. Its own test binary: the subscriber is process-wide state.

use std::io::Write;
use std::sync::{Arc, Mutex};

use app_logic::backup::{export_backup, BackupKey};
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
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
        .await
        .unwrap();
    let now = Utc::now();
    let sid = ServerId::new();
    let r = CredentialRef::new(format!("server:{}:password", sid.0));
    db.create_server(&Server {
        id: sid,
        name: SERVER_NAME.into(),
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
    tokio::task::spawn_blocking(move || credentials.set(&r, pass(PASSWORD)).unwrap())
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
}
