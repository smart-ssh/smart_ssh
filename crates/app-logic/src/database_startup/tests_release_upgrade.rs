//! Issue #15: der Startablauf von Ende zu Ende, an einer echten
//! Datenbankdatei.
//!
//! - **Wiederholter Start:** Eine Datenbank des aktuellen Release (die
//!   neueste Fixture in `persistence-sqlite/tests/fixtures/releases/`)
//!   durchläuft den ganzen Start — Umwandlung nach SQLCipher (Spec 0101,
//!   A6), die übrigen Migrationen, den Secret-Umzug (A10) — und startet
//!   dann ein zweites Mal. Der zweite Start muss Schema und Daten genau so
//!   lassen, wie der erste sie hinterlassen hat.
//! - **Downgrade:** Eine Datenbank mit einer Migration, die dieser Build
//!   nicht kennt (was ein neueres Release hinterlässt), geht durch
//!   denselben Startablauf. Er muss mit `SchemaTooNew` und dem Dialogtext
//!   aus Spec 0059 enden, und das Datenverzeichnis bleibt Byte für Byte,
//!   wie es war.
//!
//! „Startablauf“ heißt hier, was `app_shell::open_and_assemble` vor dem
//! Zusammenbau des App-Zustands ausführt: [`open_or_prepare_database`] und
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

/// Die Secrets, auf die Server und Provider der Release-Fixture verweisen —
/// in 0.5.2 lagen sie im Schlüsselbund des Betriebssystems (IDs s.
/// `generate_v0.5.2.rs`).
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

/// Der Schlüsselbund eines Nutzers des aktuellen Release: K plus Secrets.
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

/// Fragesteller für Starts, die nichts fragen dürfen: Jeder Dialog wird
/// aufgezeichnet und mit „Beenden“ beantwortet — ein unerwarteter Dialog
/// beendet den Start also, statt etwas zu verändern.
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

/// Ein Start, wie die App ihn im Schlüsselbund-Modus fährt, bis zum
/// Zusammenbau des App-Zustands. Schließt die Datenbank danach, wie das
/// Beenden der App.
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

/// Kopiert die Fixture des aktuellen Release als Datenbank der App nach `dir`.
async fn release_database(dir: &std::path::Path) -> std::path::PathBuf {
    let fixture = current_release_fixture();
    let db_path = dir.join("smart-ssh.db");
    std::fs::copy(fixture.path(), &db_path).expect("fixture can be copied");
    let file_key = match fixture.encryption {
        FixtureEncryption::Plaintext => None,
        FixtureEncryption::Sqlcipher => Some(database_key()),
    };
    // Windows checkt die Migrationen mit CRLF aus (s. Helfer).
    align_migration_checksums(&db_path, file_key.as_ref()).await;
    db_path
}

/// Issue #15, wiederholter Start: Der erste Start wandelt die
/// Klartext-Datei des Release um, migriert sie und zieht die Secrets um;
/// der zweite Start auf demselben Datenverzeichnis ändert nichts — nicht
/// das Schema, keine Zeile (auch nicht `_sqlx_migrations` mit ihren
/// Zeitstempeln und den Umzugszustand), nicht den Schlüsselbund.
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

    // --- Erster Start: Umwandlung, Migrationen, Secret-Umzug.
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

    // --- Zweiter Start auf demselben Datenverzeichnis.
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

    // Und die Daten sind noch die des Release — samt Secrets, jetzt aus der
    // Datenbank gelesen.
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

/// Issue #15, wiederholter Start einer frischen Installation: keine
/// Datenbank, K im Schlüsselbund. Der erste Start legt die verschlüsselte
/// Datei an, der zweite öffnet sie und ändert nichts.
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

/// Issue #15, Downgrade: Eine Datenbank, die ein neueres Release
/// geschrieben hat — mit einer echten `sqlx`-Migration, die dieser Build
/// nicht kennt —, geht durch den normalen Startablauf. Erwartet:
/// `SchemaTooNew` mit beiden Versionsnummern, der Dialogtext aus Spec 0059,
/// kein Dialog, kein Schreiben in den Schlüsselbund, und jede Datei im
/// Datenverzeichnis ist danach bytegleich.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_database_from_a_newer_release_stops_the_start_and_stays_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = release_database(dir.path()).await;
    let keychain = release_keychain();
    let prompt = NoDialogExpected::default();

    // Der Nutzer aktualisiert auf diesen Build (erster Start) …
    start_app(&db_path, &keychain, &prompt)
        .await
        .unwrap_or_else(|abort| panic!("upgrade start failed: {abort:?}"));
    // … dann bringt ein neueres Release seine Migration mit …
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
    // … und der Nutzer geht zurück auf diesen Build.
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
    // Die Datenbankdatei selbst, direkt nach dem Start — bevor irgendetwas
    // anderes sie anfassen konnte.
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

    // Der Dialogtext, den die App für genau diesen Fehler zeigt.
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

    // Keine Datei hinzugekommen oder liegen geblieben, kein Byte verändert.
    // Das gescheiterte Öffnen lässt seinen Pool fallen, ohne das Schließen
    // abzuwarten; `-wal`/`-shm` dieser Verbindung können deshalb kurz nach
    // dem Start noch existieren (beobachtet). Begrenzt warten, bis die
    // Verbindung die Datei freigegeben hat, dann das ganze Verzeichnis
    // vergleichen.
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
