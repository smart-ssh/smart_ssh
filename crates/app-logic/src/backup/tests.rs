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

// ---------------------------------------------------------------- import
// Issue #665: Spec 0109 tests 2, 6, 7, 8, 12, 16, 19 (import part).

use persistence_sqlite::{
    AiProviderRow, GroupRow, ImportPlan, NoteRevisionRow, RuleRow, SecretRow, ServerRow, TagRow,
};
use uuid::Uuid;

const LABEL: &str = "Previous data";

async fn empty_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
        .await
        .unwrap();
    let host_keys = FileHostKeyStore::load(dir.path().join("host_keys.json")).unwrap();
    let backup = db.backup_store();
    Fixture {
        dir,
        db,
        backup,
        host_keys,
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// A consistent data set: two folders (child), `servers` servers (the second
/// uses the first as jump host), `rules` rules (the first bound to the first
/// server), `notes` notes, one AI provider, one host key. Names carry `tag`.
fn build(tag: &str, servers: usize, rules: usize, notes: usize) -> Snapshot {
    let t = now();
    let g1 = Uuid::new_v4();
    let g2 = Uuid::new_v4();
    let group = |id, name: &str, parent| GroupRow {
        id,
        name: format!("{tag}-{name}"),
        parent_id: parent,
        notes: String::new(),
        created_at: t.clone(),
        updated_at: t.clone(),
    };
    let mut snap = Snapshot {
        groups: vec![group(g1, "g1", None), group(g2, "g2", Some(g1))],
        ..Default::default()
    };
    let ids: Vec<Uuid> = (0..servers).map(|_| Uuid::new_v4()).collect();
    for (i, id) in ids.iter().enumerate() {
        let auth = AuthMethod::Password {
            credential_ref: CredentialRef::new(format!("server:{id}:password")),
        };
        snap.servers.push(ServerRow {
            id: *id,
            name: format!("{tag}-{i}"),
            host: format!("{tag}{i}.example.invalid"),
            port: 22,
            username: "u".into(),
            group_id: (i == 0).then_some(g2),
            auth_method: serde_json::to_string(&auth).unwrap(),
            notes: String::new(),
            jump_host_id: (i == 1).then(|| ids[0]),
            created_at: t.clone(),
            updated_at: t.clone(),
            post_ingest_policy: "balanced".into(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            start_directory: None,
        });
        snap.secrets.push(SecretRow {
            owner_ref: format!("server:{id}:password"),
            value: format!("pw-{tag}-{i}"),
        });
        snap.tags.push(TagRow {
            server_id: *id,
            tag: "prod".into(),
        });
    }
    for i in 0..rules {
        let bound = i == 0 && !ids.is_empty();
        snap.rules.push(RuleRow {
            id: Uuid::new_v4(),
            pattern_type: "glob".into(),
            pattern_value: format!("rm -rf /{tag}{i}*"),
            action: "deny".into(),
            scope_type: if bound { "server" } else { "global" }.into(),
            scope_value: bound.then(|| ids[0].to_string()),
            priority: i as i64,
            created_at: t.clone(),
            updated_at: t.clone(),
        });
    }
    for i in 0..notes {
        snap.notes.push(NoteRevisionRow {
            id: Uuid::new_v4(),
            target_type: "server".into(),
            target_id: ids[i % ids.len()],
            content: format!("note {tag} {i}"),
            editor_type: "user".into(),
            ai_provider: None,
            ai_model: None,
            created_at: t.clone(),
        });
    }
    let ai = Uuid::new_v4();
    snap.ai_providers.push(AiProviderRow {
        id: ai,
        provider_type: "openai".into(),
        display_name: format!("{tag}-ai"),
        base_url: None,
        model: "m".into(),
        supports_native_tool_calling: true,
        credential_ref: format!("ai-provider:{ai}"),
        is_active: true,
        created_at: t.clone(),
        updated_at: t,
        extra_headers: "{}".into(),
        attestation_url: None,
        max_tokens_override: None,
        web_research_enabled: false,
    });
    snap.secrets.push(SecretRow {
        owner_ref: format!("ai-provider:{ai}"),
        value: format!("key-{tag}"),
    });
    snap.host_keys.push(HostKeyRow {
        host: format!("{tag}.example.invalid"),
        port: 22,
        raw_key: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            ed25519_blob(tag.as_bytes()[0]),
        ),
    });
    snap
}

/// Puts exactly `snap` into the fixture (what the app does through the
/// import; here without the safety backup).
async fn seed(f: &Fixture, snap: &Snapshot) {
    let plan = ImportPlan {
        data: snap.to_database(),
        archive_folder: None,
        now: now(),
        pending_host_keys: serde_json::to_string(&snap.host_keys).unwrap(),
    };
    f.backup.apply_import(&plan).await.unwrap();
    complete_pending_host_keys(&f.backup, &f.host_keys)
        .await
        .unwrap();
}

/// The rows in a fixed order, to compare a built set with a read one.
fn sorted(mut s: Snapshot) -> Snapshot {
    s.servers.sort_by_key(|x| x.id);
    s.groups.sort_by_key(|x| x.id);
    s.rules.sort_by_key(|x| x.id);
    s.notes.sort_by_key(|x| x.id);
    s.tags
        .sort_by(|a, b| (a.server_id, &a.tag).cmp(&(b.server_id, &b.tag)));
    s.ai_providers.sort_by_key(|x| x.id);
    s.secrets.sort_by(|a, b| a.owner_ref.cmp(&b.owner_ref));
    s.host_keys
        .sort_by(|a, b| (&a.host, a.port).cmp(&(&b.host, b.port)));
    s
}

async fn state(f: &Fixture) -> Snapshot {
    Snapshot::read(&f.backup, &f.host_keys).await.unwrap()
}

/// Writes `snap` as a backup file under the fixture's own key.
async fn write_file(f: &Fixture, snap: &Snapshot, name: &str) -> std::path::PathBuf {
    let slots = BackupKey::active_slots(&f.backup).await.unwrap();
    let bytes = ssh_manager_core::backup::seal(&ssh_manager_core::backup::SealInput {
        sections: &snap.to_sections().unwrap(),
        slots: &slots,
        created_at: 0,
        writer_version: "t",
        extensions: &[],
        kdf: ssh_manager_core::backup::KdfParams::WRITE,
    })
    .unwrap();
    let path = f.dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn ctx<'a>(f: &'a Fixture, safety: &'a Path) -> ImportContext<'a> {
    ImportContext {
        store: &f.backup,
        host_keys: &f.host_keys,
        sessions: &NoSessions,
        safety_dir: safety,
        folder_label: LABEL,
        writer_version: "t",
    }
}

fn pending(start: ImportStart) -> PendingImport {
    match start {
        ImportStart::NeedsConfirmation(p) => *p,
        ImportStart::Imported(_) => panic!("expected a confirmation request"),
    }
}

/// Tests 2 and 8: a round trip into an empty installation gives the same
/// content with the file's IDs, and creates no safety backup and no folder.
#[tokio::test(flavor = "multi_thread")]
async fn import_into_empty_installation_keeps_ids_and_creates_nothing_else() {
    let a = empty_fixture().await;
    seed(&a, &build("A", 3, 4, 5)).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "a.sshbackup").await;

    let b = empty_fixture().await;
    let safety = b.dir.path().join("safety");
    let start = start_import(&ctx(&b, &safety), &file, code_secret(&code))
        .await
        .unwrap();
    let ImportStart::Imported(summary) = start else {
        panic!("empty local data imports without asking");
    };
    assert_eq!(state(&b).await, state(&a).await);
    assert_eq!(summary.safety_backup, None);
    assert_eq!(summary.previous_data_folder, None);
    assert!(!safety.exists());
    assert!(!state(&b)
        .await
        .groups
        .iter()
        .any(|g| g.name.contains(LABEL)));
    // The marker is gone and the host keys were written.
    assert_eq!(b.backup.pending_host_keys().await.unwrap(), None);
    assert_eq!(b.host_keys.entries().unwrap().len(), 1);
}

/// I3: without the confirm call nothing changes; the numbers are those of
/// the validated content (test 6).
#[tokio::test(flavor = "multi_thread")]
async fn import_over_data_reports_counts_and_changes_nothing_without_confirm() {
    let a = empty_fixture().await;
    seed(&a, &build("F", 15, 42, 11)).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "a.sshbackup").await;

    let local = empty_fixture().await;
    seed(&local, &build("L", 12, 30, 8)).await;
    let before = state(&local).await;
    let safety = local.dir.path().join("safety");
    let p = pending(
        start_import(&ctx(&local, &safety), &file, code_secret(&code))
            .await
            .unwrap(),
    );
    let pv = p.preview();
    assert_eq!(
        (pv.local.servers, pv.local.rules, pv.local.notes),
        (12, 30, 8)
    );
    assert_eq!(
        (pv.file.servers, pv.file.rules, pv.file.notes),
        (15, 42, 11)
    );
    assert_eq!((pv.local.groups, pv.local.ai_providers), (2, 1));
    assert_eq!(pv.safety_backup_dir, safety);
    drop(p);
    assert_eq!(state(&local).await, before);
    assert!(!safety.exists());
}

/// I4, I5, test 6: safety backup, folder with new IDs, re-pointed references,
/// the file's IDs for the new content.
#[tokio::test(flavor = "multi_thread")]
async fn confirmed_import_moves_old_servers_into_a_dated_folder() {
    let a = empty_fixture().await;
    let file_snap = build("F", 15, 42, 11);
    seed(&a, &file_snap).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "a.sshbackup").await;

    let local = empty_fixture().await;
    let old = build("L", 12, 30, 8);
    seed(&local, &old).await;
    let device_code = BackupKey::create_with_code(&local.backup).await.unwrap();
    let safety = local.dir.path().join("safety");
    let c = ctx(&local, &safety);
    let summary = pending(start_import(&c, &file, code_secret(&code)).await.unwrap())
        .confirm(&c)
        .await
        .unwrap();

    // A safety backup exists and opens with the device code.
    let path = summary.safety_backup.unwrap();
    assert!(path.starts_with(&safety));
    assert_eq!(
        sorted(open_file(&path, &code_secret(&device_code)).unwrap()),
        sorted(old.clone())
    );

    let after = state(&local).await;
    let folder_name = format!("{LABEL} {}", chrono::Local::now().format("%Y-%m-%d"));
    assert_eq!(summary.previous_data_folder.as_deref(), Some(&*folder_name));
    let folder = after
        .groups
        .iter()
        .find(|g| g.name == folder_name)
        .expect("folder");
    assert_eq!(folder.parent_id, None);

    // The 15 imported servers carry the file's IDs and rows.
    for s in &file_snap.servers {
        assert!(after.servers.contains(s), "imported server with its id");
    }
    // The 12 old servers moved: new IDs, same names.
    let old_ids: Vec<Uuid> = old.servers.iter().map(|s| s.id).collect();
    let moved: Vec<&ServerRow> = after
        .servers
        .iter()
        .filter(|s| s.name.starts_with("L-"))
        .collect();
    assert_eq!(moved.len(), 12);
    assert!(moved.iter().all(|s| !old_ids.contains(&s.id)));
    // Folder structure: g1 under the folder, g2 under g1, server 0 in g2,
    // the others directly in the folder.
    let g1 = after.groups.iter().find(|g| g.name == "L-g1").unwrap();
    let g2 = after.groups.iter().find(|g| g.name == "L-g2").unwrap();
    assert_eq!(g1.parent_id, Some(folder.id));
    assert_eq!(g2.parent_id, Some(g1.id));
    let m = |n: &str| moved.iter().find(|s| s.name == n).unwrap();
    assert_eq!(m("L-0").group_id, Some(g2.id));
    assert_eq!(m("L-2").group_id, Some(folder.id));
    // Jump host, secrets, notes and server-bound rules follow the copy.
    assert_eq!(m("L-1").jump_host_id, Some(m("L-0").id));
    for s in &moved {
        assert!(s.auth_method.contains(&format!("server:{}:", s.id)));
        let secret = after
            .secrets
            .iter()
            .find(|x| x.owner_ref == format!("server:{}:password", s.id))
            .expect("secret of the copy");
        assert_eq!(secret.value, format!("pw-L-{}", &s.name[2..]));
    }
    let moved_notes = after
        .notes
        .iter()
        .filter(|n| moved.iter().any(|s| s.id == n.target_id))
        .count();
    assert_eq!(moved_notes, 8);
    assert!(after
        .rules
        .iter()
        .any(|r| r.scope_type == "server"
            && r.scope_value.as_deref() == Some(&m("L-0").id.to_string())));
    // Unbound rules, AI providers and host keys are the file's.
    assert!(!after
        .rules
        .iter()
        .any(|r| r.pattern_value.contains("/L") && r.scope_type == "global"));
    assert_eq!(after.ai_providers, file_snap.ai_providers);
    assert_eq!(after.host_keys, file_snap.host_keys);
    // I9: the device's backup key is untouched.
    assert!(BackupKey::status(&local.backup).await.unwrap().has_code);
}

/// I4 step 1 and I5: a failure inside the transaction, and a failing safety
/// backup, leave the database unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn failures_leave_the_database_unchanged() {
    let a = empty_fixture().await;
    seed(&a, &build("F", 3, 3, 3)).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "a.sshbackup").await;

    let local = empty_fixture().await;
    seed(&local, &build("L", 4, 4, 4)).await;
    BackupKey::create_with_code(&local.backup).await.unwrap();
    let before = state(&local).await;
    let hk_before = local.host_keys.entries().unwrap();
    let safety = local.dir.path().join("safety");
    let c = ctx(&local, &safety);

    for step in ["archive", "replace"] {
        let p = pending(start_import(&c, &file, code_secret(&code)).await.unwrap());
        let err = p.confirm_with(&c, &mut |s| s == step).await.unwrap_err();
        assert!(matches!(err, BackupOpError::Store(_)), "{err:?}");
        assert_eq!(state(&local).await, before, "after failing at {step}");
        assert_eq!(local.host_keys.entries().unwrap(), hk_before);
        assert_eq!(local.backup.pending_host_keys().await.unwrap(), None);
    }

    // The safety backup cannot be written: abort before step 2.
    let blocker = local.dir.path().join("afile");
    std::fs::write(&blocker, b"x").unwrap();
    let bad_dir = blocker.join("sub");
    let c = ctx(&local, &bad_dir);
    let p = pending(start_import(&c, &file, code_secret(&code)).await.unwrap());
    assert_eq!(p.confirm(&c).await.unwrap_err(), BackupOpError::Write);
    assert_eq!(state(&local).await, before);

    // No backup key on this device: the first-export flow is needed.
    let nokey = empty_fixture().await;
    seed(&nokey, &build("N", 1, 1, 1)).await;
    let nokey_before = state(&nokey).await;
    let safety = nokey.dir.path().join("safety");
    let c = ctx(&nokey, &safety);
    let p = pending(start_import(&c, &file, code_secret(&code)).await.unwrap());
    assert_eq!(p.confirm(&c).await.unwrap_err(), BackupOpError::KeyRequired);
    assert_eq!(state(&nokey).await, nokey_before);
}

/// Test 16: a crash between commit and the host-key write is completed at the
/// next start.
#[tokio::test(flavor = "multi_thread")]
async fn pending_host_keys_are_completed_at_start() {
    let f = empty_fixture().await;
    seed(&f, &build("O", 1, 1, 1)).await;
    let new = build("N", 1, 1, 1);
    let plan = ImportPlan {
        data: new.to_database(),
        archive_folder: None,
        now: now(),
        pending_host_keys: serde_json::to_string(&new.host_keys).unwrap(),
    };
    f.backup.apply_import(&plan).await.unwrap();
    // "Crash": the file still holds the old keys, the marker is set, and the
    // marker is not content.
    assert_eq!(f.host_keys.entries().unwrap().len(), 1);
    assert_eq!(f.host_keys.entries().unwrap()[0].0, "O.example.invalid");
    assert!(f.backup.pending_host_keys().await.unwrap().is_some());
    assert!(state(&f)
        .await
        .secrets
        .iter()
        .all(|s| s.owner_ref.starts_with("server:") || s.owner_ref.starts_with("ai-provider:")));
    // The next start: a fresh store over the same file.
    let reloaded = FileHostKeyStore::load(f.dir.path().join("host_keys.json")).unwrap();
    assert!(complete_pending_host_keys(&f.backup, &reloaded)
        .await
        .unwrap());
    assert_eq!(reloaded.entries().unwrap()[0].0, "N.example.invalid");
    assert_eq!(f.backup.pending_host_keys().await.unwrap(), None);
    assert!(!complete_pending_host_keys(&f.backup, &reloaded)
        .await
        .unwrap());
}

/// Tests 12 and U5/U6/S-3: every kind of bad content rejects the whole file
/// and changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn invalid_content_is_rejected_as_a_whole() {
    let local = empty_fixture().await;
    seed(&local, &build("L", 2, 2, 2)).await;
    let code = BackupKey::create_with_code(&local.backup).await.unwrap();
    let before = state(&local).await;
    let safety = local.dir.path().join("safety");

    type Mutate = fn(&mut Snapshot);
    let cases: Vec<(&str, Mutate)> = vec![
        ("invalid regex deny rule", |s| {
            s.rules[1].pattern_type = "regex".into();
            s.rules[1].pattern_value = "(".into();
        }),
        ("unknown action", |s| s.rules[1].action = "maybe".into()),
        ("unknown scope", |s| s.rules[1].scope_type = "planet".into()),
        ("overlong pattern", |s| {
            s.rules[1].pattern_value = "a".repeat(70_000)
        }),
        ("rule for a missing server", |s| {
            s.rules[0].scope_value = Some(Uuid::new_v4().to_string())
        }),
        ("dangling folder", |s| {
            s.servers[0].group_id = Some(Uuid::new_v4())
        }),
        ("dangling jump host", |s| {
            s.servers[0].jump_host_id = Some(Uuid::new_v4())
        }),
        ("duplicate id", |s| {
            let c = s.servers[0].clone();
            s.servers.push(c);
        }),
        ("jump host cycle", |s| {
            s.servers[0].jump_host_id = Some(s.servers[1].id);
            s.servers[1].jump_host_id = Some(s.servers[0].id);
        }),
        ("self jump host", |s| {
            s.servers[0].jump_host_id = Some(s.servers[0].id)
        }),
        ("folder cycle", |s| {
            s.groups[0].parent_id = Some(s.groups[1].id)
        }),
        ("credential of another owner", |s| {
            s.servers[0].auth_method = r#"{"Password":{"credential_ref":"app:backup_key"}}"#.into()
        }),
        ("secret without owner", |s| {
            s.secrets.push(SecretRow {
                owner_ref: format!("server:{}:password", Uuid::new_v4()),
                value: "x".into(),
            })
        }),
        ("secret with a device reference", |s| {
            s.secrets.push(SecretRow {
                owner_ref: persistence_sqlite::BACKUP_KEY_REF.into(),
                value: "x".into(),
            })
        }),
        ("note for a missing entry", |s| {
            s.notes[0].target_id = Uuid::new_v4()
        }),
        ("unknown server setting", |s| {
            s.servers[0].post_ingest_policy = "lax".into()
        }),
        ("nil id", |s| s.servers[0].id = Uuid::nil()),
    ];
    for (name, mutate) in cases {
        let mut bad = build("F", 2, 2, 2);
        mutate(&mut bad);
        let file = write_file(&local, &bad, "bad.sshbackup").await;
        let err = start_import(&ctx(&local, &safety), &file, code_secret(&code))
            .await
            .unwrap_err();
        assert!(matches!(err, BackupOpError::Invalid(_)), "{name}: {err:?}");
        assert_eq!(state(&local).await, before, "{name}");
        assert!(!safety.exists(), "{name}");
    }

    // Counter-proof: the store itself would take the invalid rule; it is the
    // validation layer that keeps it out.
    let probe = empty_fixture().await;
    let mut bad = build("F", 1, 2, 1);
    bad.rules[1].pattern_type = "regex".into();
    bad.rules[1].pattern_value = "(".into();
    seed(&probe, &bad).await;
    assert!(state(&probe)
        .await
        .rules
        .iter()
        .any(|r| r.pattern_value == "("));
}

/// A good file with the same content as the local data is accepted (the
/// validation does not reject what the app itself writes).
#[tokio::test(flavor = "multi_thread")]
async fn second_import_on_the_same_day_gets_a_numbered_folder() {
    let a = empty_fixture().await;
    seed(&a, &build("F", 2, 2, 2)).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "a.sshbackup").await;

    let local = empty_fixture().await;
    seed(&local, &build("L", 2, 2, 2)).await;
    BackupKey::create_with_code(&local.backup).await.unwrap();
    let safety = local.dir.path().join("safety");
    let c = ctx(&local, &safety);
    let day = chrono::Local::now().format("%Y-%m-%d");
    let first = pending(start_import(&c, &file, code_secret(&code)).await.unwrap())
        .confirm(&c)
        .await
        .unwrap();
    let second = pending(start_import(&c, &file, code_secret(&code)).await.unwrap())
        .confirm(&c)
        .await
        .unwrap();
    assert_eq!(
        first.previous_data_folder.as_deref(),
        Some(&*format!("{LABEL} {day}"))
    );
    assert_eq!(
        second.previous_data_folder.as_deref(),
        Some(&*format!("{LABEL} {day} (2)"))
    );
    // Two different safety files; none overwritten (I4).
    assert_ne!(first.safety_backup, second.safety_backup);
}

/// I6: the safety backup restores the state before the import.
#[tokio::test(flavor = "multi_thread")]
async fn safety_backup_is_itself_importable_and_restores_the_old_state() {
    let a = empty_fixture().await;
    seed(&a, &build("F", 3, 3, 3)).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "a.sshbackup").await;

    let local = empty_fixture().await;
    let old = build("L", 3, 3, 3);
    seed(&local, &old).await;
    let device_code = BackupKey::create_with_code(&local.backup).await.unwrap();
    let safety = local.dir.path().join("safety");
    let c = ctx(&local, &safety);
    let summary = pending(start_import(&c, &file, code_secret(&code)).await.unwrap())
        .confirm(&c)
        .await
        .unwrap();
    let safety_file = summary.safety_backup.unwrap();

    let restored = pending(
        start_import(&c, &safety_file, code_secret(&device_code))
            .await
            .unwrap(),
    )
    .confirm(&c)
    .await
    .unwrap();
    assert!(restored.previous_data_folder.is_some());
    let after = state(&local).await;
    for s in &old.servers {
        assert!(after.servers.contains(s), "old server back with its id");
    }
    for s in &old.secrets {
        assert!(after.secrets.contains(s));
    }
    for r in &old.rules {
        assert!(after.rules.contains(r));
    }
    assert_eq!(after.ai_providers, old.ai_providers);
    assert_eq!(after.host_keys, old.host_keys);
}

/// Test 7 / C3 / S-5: a key-file path from the file is stored as it is and
/// never opened; where the file does not exist, a connection attempt meets
/// "not found" with the path.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn key_file_paths_are_not_opened_by_the_import() {
    use ssh_manager_core::ssh::{KeyFileError, KeyFileReader};
    let a = empty_fixture().await;
    let mut snap = build("K", 2, 0, 0);
    // A path that does not exist, and a FIFO: opening it for reading would
    // block until the test times out.
    let fifo = a.dir.path().join("fifo-key");
    let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let missing = "/definitely/not/here/id_ed25519".to_string();
    for (i, path) in [missing.clone(), fifo.to_str().unwrap().to_string()]
        .into_iter()
        .enumerate()
    {
        snap.servers[i].auth_method = serde_json::to_string(&AuthMethod::IdentityFile {
            path,
            passphrase_ref: None,
        })
        .unwrap();
        let id = snap.servers[i].id;
        snap.secrets
            .retain(|s| s.owner_ref != format!("server:{id}:password"));
    }
    seed(&a, &snap).await;
    let code = BackupKey::create_with_code(&a.backup).await.unwrap();
    let file = export_to(&a, "k.sshbackup").await;

    let b = empty_fixture().await;
    let safety = b.dir.path().join("safety");
    let start = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        start_import(&ctx(&b, &safety), &file, code_secret(&code)),
    )
    .await
    .expect("the import must not open key files")
    .unwrap();
    assert!(matches!(start, ImportStart::Imported(_)));
    let imported = state(&b).await;
    assert!(imported.servers[0].auth_method.contains("IdentityFile"));
    let Err(err) = crate::key_files::OsKeyFileReader::default().read(&missing, true) else {
        panic!("the file does not exist");
    };
    assert_eq!(err, KeyFileError::NotFound { path: missing });
    assert_eq!(err.code(), "KEY_FILE_NOT_FOUND");
}

// ---- Issue #670: read_snapshot and apply_changes -------------------------

async fn seeded_fixture() -> (Fixture, Snapshot) {
    let f = empty_fixture().await;
    let snap = build("C", 3, 3, 3);
    seed(&f, &snap).await;
    (f, snap)
}

/// `read_snapshot` holds exactly what the export holds.
#[tokio::test(flavor = "multi_thread")]
async fn read_snapshot_equals_the_export_content() {
    let (f, seeded) = seeded_fixture().await;
    let code = BackupKey::create_with_code(&f.backup).await.unwrap();
    let file = export_to(&f, "a.sshbackup").await;
    let exported = open_file(&file, &code_secret(&code)).unwrap();
    let read = read_snapshot(&f.backup, &f.host_keys).await.unwrap();
    assert_eq!(sorted(read.clone()), sorted(exported));
    assert_eq!(sorted(read), sorted(seeded));
}

/// Inserts, updates and deletes across all entity types give exactly the
/// expected state and fire the data-change notification.
#[tokio::test(flavor = "multi_thread")]
async fn apply_changes_inserts_updates_and_deletes_across_all_types() {
    let (f, base) = seeded_fixture().await;
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    f.db.data_changes().subscribe(std::sync::Arc::new(move |k| {
        tx.lock().unwrap().send(k.clone()).unwrap();
    }));

    let t = now();
    let mut expected = base.clone();
    let mut cs = ChangeSet::default();

    // groups: update g2's name, add g3, delete nothing (g1 has children).
    let mut g2 = expected.groups[1].clone();
    g2.name = "renamed".into();
    let g3 = GroupRow {
        id: Uuid::new_v4(),
        name: "g3".into(),
        parent_id: None,
        notes: String::new(),
        created_at: t.clone(),
        updated_at: t.clone(),
    };
    cs.groups = vec![Change::Upsert(g2.clone()), Change::Upsert(g3.clone())];
    expected.groups[1] = g2;
    expected.groups.push(g3.clone());

    // servers: update s2, delete s1 (cascade), insert s4 in g3.
    let s1 = expected.servers[1].clone();
    let mut s2 = expected.servers[2].clone();
    s2.name = "s2-new".into();
    let id4 = Uuid::new_v4();
    let mut s4 = s2.clone();
    s4.id = id4;
    s4.group_id = Some(g3.id);
    s4.auth_method = serde_json::to_string(&AuthMethod::Agent).unwrap();
    cs.servers = vec![
        Change::Delete(s1.id),
        Change::Upsert(s2.clone()),
        Change::Upsert(s4.clone()),
    ];
    expected.servers.retain(|s| s.id != s1.id);
    expected.servers.iter_mut().for_each(|s| {
        if s.id == s2.id {
            *s = s2.clone();
        }
    });
    expected.servers.push(s4.clone());
    expected.tags.retain(|x| x.server_id != s1.id);
    expected
        .secrets
        .retain(|x| !x.owner_ref.contains(&s1.id.to_string()));
    expected.notes.retain(|n| n.target_id != s1.id);

    // tags: add one, remove one.
    let new_tag = TagRow {
        server_id: id4,
        tag: "new".into(),
    };
    let removed_tag = (expected.tags[0].server_id, expected.tags[0].tag.clone());
    cs.tags = vec![
        Change::Upsert(new_tag.clone()),
        Change::Delete(removed_tag.clone()),
    ];
    expected.tags.push(new_tag);
    expected
        .tags
        .retain(|x| (x.server_id, x.tag.clone()) != removed_tag);

    // rules: update one, insert one, delete one.
    let mut r_up = expected.rules[1].clone();
    r_up.pattern_value = "shutdown*".into();
    let r_new = RuleRow {
        id: Uuid::new_v4(),
        pattern_type: "glob".into(),
        pattern_value: "reboot*".into(),
        action: "confirm".into(),
        scope_type: "global".into(),
        scope_value: None,
        priority: 9,
        created_at: t.clone(),
        updated_at: t.clone(),
    };
    let r_del = expected.rules[2].id;
    cs.rules = vec![
        Change::Upsert(r_up.clone()),
        Change::Upsert(r_new.clone()),
        Change::Delete(r_del),
    ];
    expected.rules.iter_mut().for_each(|r| {
        if r.id == r_up.id {
            *r = r_up.clone();
        }
    });
    expected.rules.push(r_new);
    expected.rules.retain(|r| r.id != r_del);
    // the rule bound to s0 stays; none was bound to s1.

    // notes: add a revision, delete one of s0/s2.
    let n_new = NoteRevisionRow {
        id: Uuid::new_v4(),
        target_type: "group".into(),
        target_id: g3.id,
        content: "hello".into(),
        editor_type: "user".into(),
        ai_provider: None,
        ai_model: None,
        created_at: t.clone(),
    };
    let n_del = expected
        .notes
        .iter()
        .find(|n| n.target_id == s2.id)
        .map(|n| n.id);
    cs.notes = vec![Change::Upsert(n_new.clone())];
    if let Some(id) = n_del {
        cs.notes.push(Change::Delete(id));
        expected.notes.retain(|n| n.id != id);
    }
    expected.notes.push(n_new);

    // AI provider: update, plus a new one with its secret; secret update.
    let mut ai = expected.ai_providers[0].clone();
    ai.display_name = "ai-new".into();
    let ai2_id = Uuid::new_v4();
    let mut ai2 = ai.clone();
    ai2.id = ai2_id;
    ai2.credential_ref = format!("ai-provider:{ai2_id}");
    ai2.is_active = false;
    let sec_ai2 = SecretRow {
        owner_ref: format!("ai-provider:{ai2_id}"),
        value: "k2".into(),
    };
    let sec_s4 = SecretRow {
        owner_ref: format!("server:{id4}:password"),
        value: "pw4".into(),
    };
    cs.ai_providers = vec![Change::Upsert(ai.clone()), Change::Upsert(ai2.clone())];
    cs.secrets = vec![
        Change::Upsert(sec_ai2.clone()),
        Change::Upsert(sec_s4.clone()),
    ];
    expected.ai_providers[0] = ai;
    expected.ai_providers.push(ai2);
    expected.secrets.push(sec_ai2);
    expected.secrets.push(sec_s4);
    // s4 uses the agent method, so its extra credential would be orphaned
    // by the validation of own references only if it were referenced; it is
    // an owned credential of an existing server and therefore valid.

    // host keys: add one, delete the existing one.
    let hk_new = HostKeyRow {
        host: "new.example.invalid".into(),
        port: 2222,
        raw_key: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            ed25519_blob(9),
        ),
    };
    let hk_old = (base.host_keys[0].host.clone(), base.host_keys[0].port);
    cs.host_keys = vec![Change::Upsert(hk_new.clone()), Change::Delete(hk_old)];
    expected.host_keys = vec![hk_new];

    let summary = apply_changes(&f.backup, &f.host_keys, &cs).await.unwrap();
    assert!(!summary.host_keys_pending);
    let after = state(&f).await;
    assert_eq!(sorted(after), sorted(expected));
    assert_eq!(f.backup.pending_host_keys().await.unwrap(), None);
    assert_eq!(f.host_keys.entries().unwrap().len(), 1);
    let kinds = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("views are told to reload");
    assert!(kinds.contains(&ssh_manager_core::data_change::DataKind::Servers));
    assert!(kinds.contains(&ssh_manager_core::data_change::DataKind::Rules));
}

fn first_server_change(base: &Snapshot, edit: impl FnOnce(&mut ServerRow)) -> ChangeSet {
    let mut s = base.servers[2].clone();
    edit(&mut s);
    ChangeSet {
        servers: vec![Change::Upsert(s)],
        ..Default::default()
    }
}

/// An invalid rule, a dangling reference or a jump-host cycle anywhere in
/// the set changes nothing, even when other entries of the set are fine.
#[tokio::test(flavor = "multi_thread")]
async fn invalid_change_sets_change_nothing() {
    let (f, base) = seeded_fixture().await;
    let before = state(&f).await;
    let t = now();
    let good_group = Change::Upsert(GroupRow {
        id: Uuid::new_v4(),
        name: "fine".into(),
        parent_id: None,
        notes: String::new(),
        created_at: t.clone(),
        updated_at: t.clone(),
    });
    let rule = |pattern_type: &str, pattern: &str, scope: &str, value: Option<String>| RuleRow {
        id: Uuid::new_v4(),
        pattern_type: pattern_type.into(),
        pattern_value: pattern.into(),
        action: "deny".into(),
        scope_type: scope.into(),
        scope_value: value,
        priority: 1,
        created_at: t.clone(),
        updated_at: t.clone(),
    };
    let mut sets: Vec<(&str, ChangeSet)> = Vec::new();
    // invalid regex (U6)
    sets.push((
        "invalid rule",
        ChangeSet {
            rules: vec![Change::Upsert(rule("regex", "^(unclosed", "global", None))],
            ..Default::default()
        },
    ));
    // unknown action: a downgraded rule must not slip in either
    let mut bad_action = rule("glob", "x", "global", None);
    bad_action.action = "ignore".into();
    sets.push((
        "unknown action",
        ChangeSet {
            rules: vec![Change::Upsert(bad_action)],
            ..Default::default()
        },
    ));
    // rule for a missing server
    sets.push((
        "dangling rule",
        ChangeSet {
            rules: vec![Change::Upsert(rule(
                "glob",
                "x",
                "server",
                Some(Uuid::new_v4().to_string()),
            ))],
            ..Default::default()
        },
    ));
    // dangling group and jump host
    let ghost = Uuid::new_v4();
    sets.push((
        "dangling folder",
        first_server_change(&base, |s| s.group_id = Some(ghost)),
    ));
    sets.push((
        "dangling jump host",
        first_server_change(&base, |s| s.jump_host_id = Some(ghost)),
    ));
    // cycle: s0 -> s1 while s1 -> s0
    let mut s0 = base.servers[0].clone();
    s0.jump_host_id = Some(base.servers[1].id);
    sets.push((
        "jump host cycle",
        ChangeSet {
            servers: vec![Change::Upsert(s0)],
            ..Default::default()
        },
    ));
    // deleting a jump host that is still used
    sets.push((
        "deleted jump host still in use",
        ChangeSet {
            servers: vec![Change::Delete(base.servers[0].id)],
            ..Default::default()
        },
    ));
    // credential of another owner
    sets.push((
        "foreign credential",
        ChangeSet {
            secrets: vec![Change::Upsert(SecretRow {
                owner_ref: format!("server:{ghost}:password"),
                value: "x".into(),
            })],
            ..Default::default()
        },
    ));
    for (name, mut cs) in sets {
        cs.groups.push(good_group.clone());
        cs.host_keys.push(Change::Delete((
            base.host_keys[0].host.clone(),
            base.host_keys[0].port,
        )));
        let err = apply_changes(&f.backup, &f.host_keys, &cs)
            .await
            .expect_err(name);
        assert!(matches!(err, BackupOpError::Invalid(_)), "{name}: {err:?}");
        assert_eq!(state(&f).await, before, "{name}");
        assert_eq!(f.host_keys.entries().unwrap().len(), 1, "{name}");
        assert_eq!(f.backup.pending_host_keys().await.unwrap(), None, "{name}");
    }
}

/// A failure in the middle of the transaction changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn failure_inside_the_transaction_changes_nothing() {
    let (f, base) = seeded_fixture().await;
    let before = state(&f).await;
    let mut s2 = base.servers[2].clone();
    s2.name = "changed".into();
    let cs = ChangeSet {
        servers: vec![Change::Upsert(s2), Change::Delete(base.servers[1].id)],
        host_keys: vec![Change::Delete((
            base.host_keys[0].host.clone(),
            base.host_keys[0].port,
        ))],
        ..Default::default()
    };
    for step in ["upsert", "delete"] {
        let err = apply_changes_with(&f.backup, &f.host_keys, &cs, &mut |s| s == step)
            .await
            .unwrap_err();
        assert!(matches!(err, BackupOpError::Store(_)), "{err:?}");
        assert_eq!(state(&f).await, before, "after failing at {step}");
        assert_eq!(f.backup.pending_host_keys().await.unwrap(), None);
    }
    apply_changes(&f.backup, &f.host_keys, &cs).await.unwrap();
    assert_ne!(state(&f).await, before);
}

/// Deleting a server removes the same dependent rows as the UI delete:
/// tags, notes with revisions, credentials and server-bound rules.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_server_removes_its_dependents() {
    let (f, base) = seeded_fixture().await;
    let victim = base.servers[2].id;
    let bound = RuleRow {
        id: Uuid::new_v4(),
        pattern_type: "glob".into(),
        pattern_value: "ls*".into(),
        action: "allow".into(),
        scope_type: "server".into(),
        scope_value: Some(victim.to_string()),
        priority: 5,
        created_at: now(),
        updated_at: now(),
    };
    let add = ChangeSet {
        rules: vec![Change::Upsert(bound.clone())],
        ..Default::default()
    };
    apply_changes(&f.backup, &f.host_keys, &add).await.unwrap();
    let before = state(&f).await;
    assert!(before.notes.iter().any(|n| n.target_id == victim));
    assert!(before.tags.iter().any(|t| t.server_id == victim));

    let del = ChangeSet {
        servers: vec![Change::Delete(victim)],
        ..Default::default()
    };
    apply_changes(&f.backup, &f.host_keys, &del).await.unwrap();
    let after = state(&f).await;
    assert!(!after.servers.iter().any(|s| s.id == victim));
    assert!(!after.tags.iter().any(|t| t.server_id == victim));
    assert!(!after.notes.iter().any(|n| n.target_id == victim));
    assert!(!after.rules.iter().any(|r| r.id == bound.id));
    assert!(!after
        .secrets
        .iter()
        .any(|s| s.owner_ref.starts_with(&format!("server:{victim}:"))));
    // Everything else is untouched.
    assert_eq!(after.servers.len(), before.servers.len() - 1);
    assert_eq!(after.groups, before.groups);
    assert_eq!(after.ai_providers, before.ai_providers);
}
