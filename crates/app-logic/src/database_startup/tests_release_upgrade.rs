//! Issue #15: the startup path end to end, on a real database file.
//!
//! - **Repeated start:** a database from the current release (the newest
//!   fixture in `persistence-sqlite/tests/fixtures/releases/`) goes through
//!   the whole startup — conversion to SQLCipher (Spec 0101, A6), the
//!   remaining migrations, the secret migration (A10) — and then starts a
//!   second time. The second start must leave schema and data exactly as
//!   the first one left them.
//! - **Downgrade:** a database that carries a migration this build does not
//!   know (what a newer release leaves behind) is opened through the same
//!   startup path. It must end in `SchemaTooNew` with the dialog text from
//!   Spec 0059, and the data directory must stay byte for byte as it was.
//!
//! "Startup path" means what `app_shell::open_and_assemble` runs before the
//! app state is assembled: [`open_or_prepare_database`] and
//! [`crate::secret_migration::migrate_secrets_into_database`].

use std::sync::Mutex;

use secrecy::{ExposeSecret, SecretString};

use credentials_keyring::KeychainAvailability;
use persistence_sqlite::test_support::{
    align_migration_checksums, apply_future_migration, current_release_fixture, directory_contents,
    max_known_migration_version, snapshot_database, FixtureEncryption, RELEASE_FIXTURE_ROOT_KEY,
};
use persistence_sqlite::{detect_database_file_state, DatabaseFileState};
use ssh_manager_core::crypto::{DatabaseKey, CHAT_CONTENT_ENCRYPTION_KEY_REF};
use ssh_manager_core::profiles::{CredentialRef, CredentialStore, ProfileStore};
use uuid::Uuid;

use super::*;
use crate::startup_error_messages::{db_connect_failure_text, Language};
use crate::test_support::InMemoryCredentialStore;

/// The secrets the release fixture's servers and provider refer to — in
/// 0.5.2 they lived in the OS keychain (see `generate_v0.5.2.rs` for the
/// IDs).
fn release_secret_refs() -> Vec<(String, String)> {
    let server_a = Uuid::from_u128(0x0011);
    let server_b = Uuid::from_u128(0x0012);
    let provider = Uuid::from_u128(0x0021);
    vec![
        (
            format!("server:{server_a}:password"),
            "Password-r052".to_string(),
        ),
        (
            format!("server:{server_b}:private_key"),
            "PrivateKey-r052".to_string(),
        ),
        (
            format!("server:{server_b}:passphrase"),
            "Passphrase-r052".to_string(),
        ),
        (format!("ai-provider:{provider}"), "ApiKey-r052".to_string()),
    ]
}

/// The OS keychain of a user of the current release: K plus the secrets.
fn release_keychain() -> InMemoryCredentialStore {
    use base64::Engine;
    let keychain = InMemoryCredentialStore::default();
    {
        let mut secrets = keychain.secrets.lock().unwrap();
        secrets.insert(
            CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
            SecretString::from(
                base64::engine::general_purpose::STANDARD.encode(RELEASE_FIXTURE_ROOT_KEY),
            ),
        );
        for (reference, value) in release_secret_refs() {
            secrets.insert(reference, SecretString::from(value));
        }
    }
    keychain
}

fn keychain_entries(keychain: &InMemoryCredentialStore) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = keychain
        .secrets
        .lock()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.expose_secret().to_string()))
        .collect();
    entries.sort();
    entries
}

fn database_key() -> DatabaseKey {
    DatabaseKey::from_root_key(&RELEASE_FIXTURE_ROOT_KEY)
}

/// A prompt for starts that must not ask anything: every dialog is
/// recorded and answered with "Quit", so an unexpected dialog ends the
/// start instead of changing something.
#[derive(Default)]
struct NoDialogExpected {
    asked: Mutex<Vec<StartupDialog>>,
}

impl NoDialogExpected {
    fn asked(&self) -> Vec<StartupDialog> {
        self.asked.lock().unwrap().clone()
    }
}

impl StartupPrompt for NoDialogExpected {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        self.asked.lock().unwrap().push(dialog);
        StartupChoice::Quit
    }
    fn confirm_start_over(&self, _renamed_to: Option<&str>) -> bool {
        false
    }
    fn confirm_generate_new_key(&self) -> bool {
        false
    }
    fn notify_started_over(&self, renamed_to: &str) {
        panic!("no start-over expected, got a rename to {renamed_to}");
    }
    fn ask_for_new_master_password(&self) -> Option<NewMasterPassword> {
        panic!("no master password expected");
    }
    fn can_ask_for_a_password(&self) -> bool {
        false
    }
}

/// One start as the app runs it in keychain mode, up to the point where
/// the app state is assembled. Closes the database afterwards, like the
/// end of the app does.
async fn start_app(
    db_path: &std::path::Path,
    keychain: &InMemoryCredentialStore,
    prompt: &NoDialogExpected,
) -> Result<(), StartupAbort> {
    let opened = open_or_prepare_database(
        db_path,
        RootKeyAccess::Keychain(keychain),
        KeychainAvailability::Available,
        prompt,
    )
    .await?;
    let database_credentials = opened
        .store
        .credential_store(tokio::runtime::Handle::current());
    let migrated = crate::secret_migration::migrate_secrets_into_database(
        &opened.store,
        keychain,
        &database_credentials,
        prompt,
        false,
    )
    .await;
    opened.store.close().await;
    migrated
}

/// Copies the current release's fixture into `dir` as the app's database.
async fn release_database(dir: &std::path::Path) -> std::path::PathBuf {
    let fixture = current_release_fixture();
    let db_path = dir.join("smart-ssh.db");
    std::fs::copy(fixture.path(), &db_path).expect("fixture can be copied");
    let file_key = match fixture.encryption {
        FixtureEncryption::Plaintext => None,
        FixtureEncryption::Sqlcipher => Some(database_key()),
    };
    // Windows checks the migrations out with CRLF (see the helper).
    align_migration_checksums(&db_path, file_key.as_ref()).await;
    db_path
}

/// Issue #15, repeated start: the first start converts the release's
/// plaintext file, migrates it and moves the secrets; the second start on
/// the same data directory changes nothing — not the schema, not a row
/// (including `_sqlx_migrations` with its timestamps and the secret
/// migration state), not the keychain.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_second_start_on_a_release_database_changes_neither_schema_nor_data() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = release_database(dir.path()).await;
    assert_eq!(
        current_release_fixture().encryption,
        FixtureEncryption::Plaintext,
        "this test covers the conversion path; with a SQLCipher fixture as the current \
         release, keep a plaintext one for this test"
    );
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Plaintext
    );
    let before_first_start = {
        let snapshot = snapshot_database(&db_path, None).await;
        assert_eq!(
            snapshot.tables["_sqlx_migrations"].rows.len() as i64,
            current_release_fixture().schema_version
        );
        snapshot
    };
    let keychain = release_keychain();
    let prompt = NoDialogExpected::default();

    // --- First start: conversion, migrations, secret migration.
    start_app(&db_path, &keychain, &prompt)
        .await
        .unwrap_or_else(|abort| panic!("first start failed: {abort:?}"));
    assert!(
        prompt.asked().is_empty(),
        "first start asked {:?}",
        prompt.asked()
    );
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Other,
        "the first start must have converted the file"
    );
    let after_first_start = snapshot_database(&db_path, Some(&database_key())).await;
    before_first_start.assert_preserved_in(&after_first_start, "first start");
    assert_eq!(
        after_first_start.tables["_sqlx_migrations"].rows.len() as i64,
        max_known_migration_version(),
        "the first start must have applied every migration of this build"
    );
    let keychain_after_first_start = keychain_entries(&keychain);
    assert_eq!(
        keychain_after_first_start
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>(),
        vec![CHAT_CONTENT_ENCRYPTION_KEY_REF],
        "the secrets must have moved out of the keychain"
    );

    // --- Second start on the same data directory.
    start_app(&db_path, &keychain, &prompt)
        .await
        .unwrap_or_else(|abort| panic!("second start failed: {abort:?}"));
    assert!(
        prompt.asked().is_empty(),
        "second start asked {:?}",
        prompt.asked()
    );
    let after_second_start = snapshot_database(&db_path, Some(&database_key())).await;
    assert_eq!(
        after_first_start, after_second_start,
        "the second start changed the schema or data"
    );
    assert_eq!(keychain_entries(&keychain), keychain_after_first_start);

    // And the data is still what the release wrote — secrets included,
    // now read from the database.
    let store = SqliteProfileStore::connect_encrypted(&db_path, &database_key())
        .await
        .unwrap();
    let hosts: Vec<String> = store
        .list_servers()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.host)
        .collect();
    assert!(
        hosts.contains(&"host-a-r052.example".to_string()),
        "{hosts:?}"
    );
    let database_credentials = store.credential_store(tokio::runtime::Handle::current());
    for (reference, value) in release_secret_refs() {
        let stored = database_credentials
            .get(&CredentialRef::new(reference.clone()))
            .unwrap_or_else(|err| panic!("{reference} missing from the database: {err:?}"));
        assert_eq!(stored.expose_secret(), value);
    }
    store.close().await;
}

/// Issue #15, repeated start on a fresh installation: no database, K in
/// the keychain. The first start creates the encrypted file, the second
/// one opens it and changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_second_start_on_a_fresh_installation_changes_neither_schema_nor_data() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("smart-ssh.db");
    let keychain = release_keychain();
    keychain
        .secrets
        .lock()
        .unwrap()
        .retain(|k, _| k == CHAT_CONTENT_ENCRYPTION_KEY_REF);
    let prompt = NoDialogExpected::default();

    start_app(&db_path, &keychain, &prompt)
        .await
        .unwrap_or_else(|abort| panic!("first start failed: {abort:?}"));
    let after_first_start = snapshot_database(&db_path, Some(&database_key())).await;
    start_app(&db_path, &keychain, &prompt)
        .await
        .unwrap_or_else(|abort| panic!("second start failed: {abort:?}"));
    let after_second_start = snapshot_database(&db_path, Some(&database_key())).await;

    assert!(prompt.asked().is_empty(), "asked {:?}", prompt.asked());
    assert_eq!(after_first_start, after_second_start);
}

/// Issue #15, downgrade: a database written by a newer release — a real
/// `sqlx` migration this build does not know — goes through the normal
/// startup path. Expected: `SchemaTooNew` with both version numbers, the
/// Spec 0059 dialog text, no dialog, no keychain write, and every file in
/// the data directory byte-identical afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_database_from_a_newer_release_stops_the_start_and_stays_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = release_database(dir.path()).await;
    let keychain = release_keychain();
    let prompt = NoDialogExpected::default();

    // This build's user upgrades (first start) …
    start_app(&db_path, &keychain, &prompt)
        .await
        .unwrap_or_else(|abort| panic!("upgrade start failed: {abort:?}"));
    // … then a newer release adds its migration …
    let max_known = max_known_migration_version();
    let future_version = max_known + 1;
    apply_future_migration(
        &db_path,
        Some(&database_key()),
        future_version,
        "future_release",
        "CREATE TABLE future_release_table (id INTEGER PRIMARY KEY, note TEXT NOT NULL);\n\
         INSERT INTO future_release_table (note) VALUES ('written-by-a-newer-release');\n",
    )
    .await;
    // … and the user goes back to this build.
    let files_before = directory_contents(dir.path());
    assert!(
        files_before.keys().all(|name| name == "smart-ssh.db"),
        "only the closed database file may be there before the start: {:?}",
        files_before.keys()
    );
    let snapshot_before = snapshot_database(&db_path, Some(&database_key())).await;
    let keychain_before = keychain_entries(&keychain);

    let abort = open_or_prepare_database(
        &db_path,
        RootKeyAccess::Keychain(&keychain),
        KeychainAvailability::Available,
        &prompt,
    )
    .await
    .err()
    .expect("a database from a newer release must not open");
    // The database file itself, right after the start returned — before
    // anything else had a chance to touch it.
    assert!(
        std::fs::read(&db_path).unwrap() == files_before["smart-ssh.db"],
        "the failed start modified the database file"
    );

    let kind = match abort {
        StartupAbort::Fatal { kind, .. } => kind,
        other => panic!("expected a fatal startup error, got {other:?}"),
    };
    assert_eq!(
        kind,
        ConnectFailureKind::SchemaTooNew {
            applied_version: future_version,
            max_known_version: max_known,
        }
    );
    assert!(prompt.asked().is_empty(), "asked {:?}", prompt.asked());
    assert_eq!(keychain_entries(&keychain), keychain_before);

    // The dialog text the app shows for exactly this error.
    let log_dir = dir.path().join("logs");
    let en = db_connect_failure_text(&kind, &db_path, &log_dir, Language::En);
    assert!(
        en.message.contains(&format!(
            "The database was created by a newer version of Smart SSH (database version \
             {future_version}, this build knows versions up to {max_known})."
        )),
        "{}",
        en.message
    );
    assert!(en
        .message
        .contains("Please install the latest version of Smart SSH."));
    let de = db_connect_failure_text(&kind, &db_path, &log_dir, Language::De);
    assert!(
        de.message.contains(&format!(
            "(Datenbank-Version {future_version}, dieses Programm kennt Versionen bis \
             {max_known})"
        )),
        "{}",
        de.message
    );

    // No file added or left behind, not a byte changed. The failed open
    // drops its connection pool without awaiting the close, so SQLite's
    // `-wal`/`-shm` of that connection can still exist for a moment after
    // the start returned (observed). Wait — bounded — until the connection
    // has released the file, then compare the whole directory.
    let released = async {
        while directory_contents(dir.path())
            .keys()
            .any(|name| name != "smart-ssh.db")
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), released)
        .await
        .expect("the failed start must release the database within 10 s");
    let files_after = directory_contents(dir.path());
    assert_eq!(
        files_after.keys().collect::<Vec<_>>(),
        files_before.keys().collect::<Vec<_>>(),
        "the failed start added or removed files in the data directory"
    );
    for (name, before) in &files_before {
        let after = &files_after[name];
        let first_difference = before.iter().zip(after.iter()).position(|(a, b)| a != b);
        assert!(
            before == after,
            "the failed start modified {name}: {} -> {} bytes, first differing byte at {:?}",
            before.len(),
            after.len(),
            first_difference
        );
    }
    let snapshot_after = snapshot_database(&db_path, Some(&database_key())).await;
    assert_eq!(snapshot_before, snapshot_after);
}
