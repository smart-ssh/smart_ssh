//! Issue #664: backup key, snapshot and export (Spec 0109 tests 1, 14, 15, 18,
//! 19 export part).

use std::path::Path;

use chrono::Utc;
use persistence_sqlite::{SqliteBackupStore, SqliteProfileStore};
use secrecy::SecretString;
use ssh_manager_core::backup::{open, BackupError, BackupSecret, RecoveryCode};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, CredentialStore, Group, GroupId, PostIngestPolicy, ProfileStore,
    Server,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::HostKeyStore;

use super::*;
use crate::host_key_store::FileHostKeyStore;

const SERVER_NAME: &str = "prod-db-marker";
const PASSWORD: &str = "pw-marker-0664";

struct Fixture {
    dir: tempfile::TempDir,
    db: SqliteProfileStore,
    backup: SqliteBackupStore,
    host_keys: FileHostKeyStore,
}

fn ed25519_blob(fill: u8) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&11u32.to_be_bytes());
    b.extend_from_slice(b"ssh-ed25519");
    b.extend_from_slice(&32u32.to_be_bytes());
    b.extend_from_slice(&[fill; 32]);
    b
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
        .await
        .unwrap();
    let now = Utc::now();
    let group = Group {
        id: GroupId::new(),
        name: "g".into(),
        parent_id: None,
        notes: String::new(),
        created_at: now,
        updated_at: now,
    };
    db.create_group(&group).await.unwrap();
    let sid = ServerId::new();
    let server = Server {
        id: sid,
        name: SERVER_NAME.into(),
        host: "example.invalid".into(),
        port: 22,
        username: "deploy".into(),
        group_id: Some(group.id),
        tags: vec!["prod".into()],
        auth: AuthMethod::Password {
            credential_ref: CredentialRef::new(format!("server:{}:password", sid.0)),
        },
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    };
    db.create_server(&server).await.unwrap();
    let credentials = db.credential_store(tokio::runtime::Handle::current());
    let AuthMethod::Password { credential_ref } = &server.auth else {
        unreachable!()
    };
    // The credential store blocks a worker thread; do it off the runtime.
    let r = credential_ref.clone();
    tokio::task::spawn_blocking(move || {
        credentials
            .set(&r, SecretString::from(PASSWORD.to_string()))
            .unwrap()
    })
    .await
    .unwrap();
    let host_keys = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();
    host_keys
        .trust("example.invalid", 22, &ed25519_blob(1))
        .unwrap();
    let backup = db.backup_store();
    Fixture {
        dir,
        db,
        backup,
        host_keys,
    }
}

fn code_secret(code: &RecoveryCode) -> BackupSecret {
    BackupSecret::Code(RecoveryCode::parse(&code.display_string()).unwrap())
}

fn pass(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}

fn open_file(path: &Path, secret: &BackupSecret) -> Result<Snapshot, BackupError> {
    let bytes = std::fs::read(path).unwrap();
    let opened = open(&bytes, secret, KNOWN_SECTIONS)?;
    Ok(Snapshot::from_sections(&opened.sections).unwrap())
}

async fn export_to(f: &Fixture, name: &str) -> std::path::PathBuf {
    let path = f.dir.path().join(name);
    export_backup(&f.backup, &f.host_keys, &path, "0.0.0-test")
        .await
        .unwrap();
    path
}

/// X2/K2: without a backup key nothing is written and no key is created.
#[tokio::test(flavor = "multi_thread")]
async fn export_without_key_asks_for_the_first_export_flow() {
    let f = fixture().await;
    let path = f.dir.path().join("a.sshbackup");
    let err = export_backup(&f.backup, &f.host_keys, &path, "t")
        .await
        .unwrap_err();
    assert_eq!(err, BackupOpError::KeyRequired);
    assert!(!path.exists());
    assert!(!BackupKey::status(&f.backup).await.unwrap().exists);
}

/// Test 1 / first acceptance criterion: a second export needs no input and
/// both files open with the same code; the content is complete.
#[tokio::test(flavor = "multi_thread")]
async fn two_exports_open_with_the_same_code() {
    let f = fixture().await;
    let code = BackupKey::create_with_code(&f.backup).await.unwrap();
    let a = export_to(&f, "a.sshbackup").await;
    let b = export_to(&f, "b.sshbackup").await;
    let secret = code_secret(&code);
    let sa = open_file(&a, &secret).unwrap();
    let sb = open_file(&b, &secret).unwrap();
    assert_eq!(sa, sb);
    let c = sa.counts();
    assert_eq!(
        (c.servers, c.groups, c.tags, c.secrets, c.host_keys),
        (1, 1, 1, 1, 1)
    );
    assert_eq!(sa.secrets[0].value, PASSWORD);
    // New data key and nonces per file (X5): the files differ.
    assert_ne!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    // C4: nothing in the clear.
    let raw = std::fs::read(&a).unwrap();
    let hay = String::from_utf8_lossy(&raw);
    assert!(!hay.contains(PASSWORD) && !hay.contains(SERVER_NAME));
}

/// A creating call on a device with a key does not replace it.
#[tokio::test(flavor = "multi_thread")]
async fn create_with_code_refuses_to_replace_an_existing_key() {
    let f = fixture().await;
    BackupKey::create_with_code(&f.backup).await.unwrap();
    assert_eq!(
        BackupKey::create_with_code(&f.backup).await.unwrap_err(),
        BackupOpError::KeyAlreadyExists
    );
}

/// K6 / test 14.
#[tokio::test(flavor = "multi_thread")]
async fn new_code_applies_to_later_files_only() {
    let f = fixture().await;
    let old = BackupKey::create_with_code(&f.backup).await.unwrap();
    let before = export_to(&f, "before.sshbackup").await;
    let new = BackupKey::new_code(&f.backup).await.unwrap();
    let after = export_to(&f, "after.sshbackup").await;
    assert!(open_file(&before, &code_secret(&old)).is_ok());
    assert!(open_file(&after, &code_secret(&new)).is_ok());
    assert_eq!(
        open_file(&after, &code_secret(&old)).unwrap_err(),
        BackupError::CannotOpen
    );
    assert_eq!(
        open_file(&before, &code_secret(&new)).unwrap_err(),
        BackupError::CannotOpen
    );
}

/// K5 / test 15.
#[tokio::test(flavor = "multi_thread")]
async fn passphrase_rules_and_coexistence_with_the_code() {
    let f = fixture().await;
    let code = BackupKey::create_with_code(&f.backup).await.unwrap();
    // Shorter than 12 characters: rejected, nothing changes.
    assert_eq!(
        BackupKey::set_passphrase(&f.backup, pass("elevenchars"))
            .await
            .unwrap_err(),
        BackupOpError::Format(BackupError::PassphraseTooShort)
    );
    assert!(!BackupKey::status(&f.backup).await.unwrap().has_passphrase);

    BackupKey::set_passphrase(&f.backup, pass("first passphrase 1"))
        .await
        .unwrap();
    let status = BackupKey::status(&f.backup).await.unwrap();
    assert!(status.has_code && status.has_passphrase);
    let both = export_to(&f, "both.sshbackup").await;
    assert!(open_file(&both, &code_secret(&code)).is_ok());
    assert!(open_file(&both, &BackupSecret::Passphrase(pass("first passphrase 1"))).is_ok());

    // A second passphrase replaces the first for new files.
    BackupKey::set_passphrase(&f.backup, pass("second passphrase 2"))
        .await
        .unwrap();
    let replaced = export_to(&f, "replaced.sshbackup").await;
    assert!(open_file(
        &replaced,
        &BackupSecret::Passphrase(pass("second passphrase 2"))
    )
    .is_ok());
    assert_eq!(
        open_file(
            &replaced,
            &BackupSecret::Passphrase(pass("first passphrase 1"))
        )
        .unwrap_err(),
        BackupError::CannotOpen
    );
    assert!(open_file(&replaced, &code_secret(&code)).is_ok());
    // The earlier file still opens with the earlier passphrase.
    assert!(open_file(&both, &BackupSecret::Passphrase(pass("first passphrase 1"))).is_ok());
}

/// K2 alternative: a passphrase as the first and only slot.
#[tokio::test(flavor = "multi_thread")]
async fn passphrase_can_be_the_only_slot() {
    let f = fixture().await;
    BackupKey::set_passphrase(&f.backup, pass("only passphrase 12"))
        .await
        .unwrap();
    let s = BackupKey::status(&f.backup).await.unwrap();
    assert!(s.exists && !s.has_code && s.has_passphrase);
    let path = export_to(&f, "p.sshbackup").await;
    assert!(open_file(&path, &BackupSecret::Passphrase(pass("only passphrase 12"))).is_ok());
}

/// C2: the backup key is in no secret listing and not in the file; the stored
/// key holds neither the code nor the passphrase.
#[tokio::test(flavor = "multi_thread")]
async fn backup_key_is_not_content() {
    let f = fixture().await;
    let code = BackupKey::create_with_code(&f.backup).await.unwrap();
    let path = export_to(&f, "a.sshbackup").await;
    let snap = open_file(&path, &code_secret(&code)).unwrap();
    assert!(snap
        .secrets
        .iter()
        .all(|s| s.owner_ref != persistence_sqlite::BACKUP_KEY_REF));
    let snap = Snapshot::read(&f.backup, &f.host_keys).await.unwrap();
    assert!(snap
        .secrets
        .iter()
        .all(|s| s.owner_ref != persistence_sqlite::BACKUP_KEY_REF));
    // K4: the code itself is not stored.
    use secrecy::ExposeSecret;
    let blob = f.backup.load_backup_key().await.unwrap().unwrap();
    assert!(!blob.expose_secret().contains(&code.display_string()[..5]));
    drop(f.db);
}

/// X4: a file that does not verify is reported and removed.
#[tokio::test(flavor = "multi_thread")]
async fn failed_read_back_removes_the_file() {
    let f = fixture().await;
    BackupKey::create_with_code(&f.backup).await.unwrap();
    let path = f.dir.path().join("bad.sshbackup");
    let err = export_backup_with(&f.backup, &f.host_keys, &path, "t", &mut |p| {
        let mut bytes = std::fs::read(p).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(p, bytes).unwrap();
    })
    .await
    .unwrap_err();
    assert_eq!(err, BackupOpError::VerificationFailed);
    assert!(!path.exists());
}

/// Strict typed reading (U2): an unknown field in a known structure fails.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_sections_are_read_strictly() {
    let f = fixture().await;
    let snap = Snapshot::read(&f.backup, &f.host_keys).await.unwrap();
    let mut sections = snap.to_sections().unwrap();
    assert_eq!(Snapshot::from_sections(&sections).unwrap(), snap);
    let tags = sections.iter_mut().find(|s| s.kind == 3).unwrap();
    *tags = ssh_manager_core::backup::Section::new(
        3,
        1,
        true,
        br#"[{"server_id":"11111111-1111-4111-8111-111111111111","tag":"x","extra":1}]"#.to_vec(),
    );
    assert!(Snapshot::from_sections(&sections).is_err());
}

/// C2: the secret move ignores the reserved entry, so it is never moved,
/// compared or deleted as an orphan.
#[test]
fn secret_move_skips_the_reserved_backup_key_entry() {
    let reserved = CredentialRef::new(persistence_sqlite::BACKUP_KEY_REF.to_string());
    assert!(!crate::secret_migration::is_migratable(&reserved));
}
