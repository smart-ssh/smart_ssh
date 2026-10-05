//! Spec 0101, T11 (A10/A11) — ohne die Variante *übersprungen*, die erst
//! mit A11.1 in Etappe 3 entsteht.
//!
//! **Multi-Thread-Runtime** wie in `crate::database_startup::tests`: Der
//! Datenbank-`CredentialStore` blockiert seinen Arbeitsthread
//! (`block_in_place`, s. `persistence_sqlite::credential_store`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use secrecy::{ExposeSecret, SecretString};

use persistence_sqlite::{AiProviderConfig, SqliteCredentialStore, SqliteProfileStore};
use ssh_manager_core::ai::{ProviderId, ProviderType};
use ssh_manager_core::crypto::DatabaseKey;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
    PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use super::*;

const TEST_ROOT_KEY: [u8; 32] = [9; 32];
const MARKER: &str = "Secret-0101";

/// Test-Schlüsselbund mit abschaltbaren Fehlern — er **zählt** `delete`,
/// weil mehrere Zusicherungen von T11 lauten „kein einziges `delete`".
#[derive(Default)]
struct TestKeyring {
    entries: Mutex<HashMap<String, String>>,
    /// Referenz, deren `get` scheitert.
    failing_get: Mutex<Option<String>>,
    /// Referenz, deren `delete` scheitert.
    failing_delete: Mutex<Option<String>>,
    deletes: AtomicUsize,
}

impl TestKeyring {
    fn with(entries: &[(&str, &str)]) -> Self {
        let store = Self::default();
        for (reference, value) in entries {
            store
                .entries
                .lock()
                .unwrap()
                .insert((*reference).to_string(), (*value).to_string());
        }
        store
    }

    fn deletes(&self) -> usize {
        self.deletes.load(Ordering::SeqCst)
    }

    fn has(&self, reference: &str) -> bool {
        self.entries.lock().unwrap().contains_key(reference)
    }

    fn count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

impl CredentialStore for TestKeyring {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        if self.failing_get.lock().unwrap().as_deref() == Some(r.as_str()) {
            return Err(CredentialError::Backend("LIBTEXT Geheim-0101".into()));
        }
        self.entries
            .lock()
            .unwrap()
            .get(r.as_str())
            .map(|v| SecretString::from(v.clone()))
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        self.entries
            .lock()
            .unwrap()
            .insert(r.as_str().to_string(), value.expose_secret().to_string());
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        if self.failing_delete.lock().unwrap().as_deref() == Some(r.as_str()) {
            return Err(CredentialError::Backend("LIBTEXT Geheim-0101".into()));
        }
        self.entries.lock().unwrap().remove(r.as_str());
        Ok(())
    }
}

/// Dialog-Doppel: zeichnet auf, was gefragt wurde, und antwortet nach Skript.
struct ScriptedPrompt {
    answers: Mutex<Vec<StartupChoice>>,
    asked: Mutex<Vec<StartupDialog>>,
}

impl ScriptedPrompt {
    fn new(answers: Vec<StartupChoice>) -> Self {
        Self {
            answers: Mutex::new(answers),
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl StartupPrompt for ScriptedPrompt {
    /// A11/A11.1: Der Umzugs-Dialog fragt nie nach einem Master-Passwort.
    fn ask_for_new_master_password(&self) -> Option<crate::database_startup::NewMasterPassword> {
        panic!("der Umzug fragt nie nach einem Master-Passwort");
    }
    /// A11.1 hängt nicht daran, ob eine Texteingabe möglich ist, sondern am
    /// Modus — `offers_skip` wird dem Umzug eigens übergeben.
    fn can_ask_for_a_password(&self) -> bool {
        false
    }

    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        self.asked.lock().unwrap().push(dialog);
        let mut answers = self.answers.lock().unwrap();
        if answers.is_empty() {
            StartupChoice::Quit
        } else {
            answers.remove(0)
        }
    }
    fn confirm_start_over(&self, _renamed_to: Option<&str>) -> bool {
        panic!("beim Secret-Umzug darf keine „Neu anfangen“-Bestätigung erscheinen");
    }
    fn confirm_generate_new_key(&self) -> bool {
        panic!("beim Secret-Umzug darf keine Schlüssel-Bestätigung erscheinen");
    }
    fn notify_started_over(&self, _renamed_to: &str) {
        panic!("beim Secret-Umzug darf nichts umbenannt werden");
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    store: SqliteProfileStore,
    database: SqliteCredentialStore,
    servers: Vec<ServerId>,
}

/// Zwei Server mit `PrivateKey`-Anmeldung, alle Slots belegt bis auf einen
/// (der fehlende wird je Test gewählt) — der Aufbau aus T11.
///
/// **Ohne Provider und ohne die übrigen Anmeldearten**, absichtlich: Die
/// Zählungen mehrerer Tests hängen an genau diesen sechs Slots. Den
/// Provider-Zweig und die anderen Anmeldearten deckt
/// `test_t11_the_provider_branch_and_the_other_auth_kinds_move_too` mit
/// eigenem Aufbau ab (spec-reviewer Runde 1: sonst bliebe die Suite grün,
/// wenn jemand den Provider-Zweig in `refs_in_database` vergisst).
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteProfileStore::connect_encrypted(
        &dir.path().join("smart-ssh.db"),
        &DatabaseKey::from_root_key(&TEST_ROOT_KEY),
    )
    .await
    .unwrap();
    let database = store.credential_store(tokio::runtime::Handle::current());

    let mut servers = Vec::new();
    for index in 0..2 {
        let id = ServerId::new();
        servers.push(id);
        let now = chrono::Utc::now();
        store
            .create_server(&Server {
                id,
                name: format!("T11-{index}"),
                host: "host-0101.example".to_string(),
                port: 22,
                username: "user-0101".to_string(),
                group_id: None,
                tags: Vec::new(),
                auth: AuthMethod::PrivateKey {
                    credential_ref: CredentialRef::new(format!("server:{}:private_key", id.0)),
                    passphrase_ref: Some(CredentialRef::new(format!("server:{}:passphrase", id.0))),
                },
                notes: String::new(),
                jump_host: None,
                post_ingest_policy: PostIngestPolicy::default(),
                ai_injection_check_enabled: false,
                sftp_server_path: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
    }

    Fixture {
        _dir: dir,
        store,
        database,
        servers,
    }
}

fn keyring_for(fixture: &Fixture, leave_out: Option<&str>) -> TestKeyring {
    let mut entries: Vec<(String, String)> = Vec::new();
    for id in &fixture.servers {
        for slot in ["private_key", "passphrase", "sudo_password"] {
            entries.push((
                format!("server:{}:{slot}", id.0),
                format!("{MARKER}-{slot}"),
            ));
        }
    }
    let keyring = TestKeyring::default();
    for (reference, value) in entries {
        if Some(reference.as_str()) == leave_out {
            continue;
        }
        keyring
            .set(&CredentialRef::new(reference), SecretString::from(value))
            .unwrap();
    }
    keyring
}

/// **T11, Provider-Zweig und die übrigen Anmeldearten** (spec-reviewer
/// Runde 1: „hätte jemand Provider in `refs_in_database` vergessen, bliebe
/// die Suite grün").
///
/// Der Hauptfall oben benutzt nur `PrivateKey`. Hier steht je eine
/// `Password`-, `Certificate`-, `IdentityFile`- und `Agent`-Anmeldung und
/// ein KI-Provider — also jeder Arm von `refs_of_server` **und** die
/// Provider-Schleife.
///
/// **Gegenbeweis geführt:** Ohne die Provider-Schleife in
/// `refs_in_database` bleibt das Provider-Secret im Schlüsselbund und
/// fehlt in der Datenbank; ohne einen der `match`-Arme fehlt der jeweilige
/// Slot. Beides scheitert hier sichtbar.
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_the_provider_branch_and_the_other_auth_kinds_move_too() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteProfileStore::connect_encrypted(
        &dir.path().join("smart-ssh.db"),
        &DatabaseKey::from_root_key(&TEST_ROOT_KEY),
    )
    .await
    .unwrap();
    let database = store.credential_store(tokio::runtime::Handle::current());

    // Je Anmeldeart ein Server, dazu die Referenzen, die dabei entstehen.
    let mut expected: Vec<String> = Vec::new();
    for (index, auth_for) in [0usize, 1, 2, 3].into_iter().zip(
        [
            (|id: ServerId| AuthMethod::Password {
                credential_ref: CredentialRef::new(format!("server:{}:password", id.0)),
            }) as fn(ServerId) -> AuthMethod,
            |id: ServerId| AuthMethod::Certificate {
                cert_ref: CredentialRef::new(format!("server:{}:certificate", id.0)),
                key_ref: CredentialRef::new(format!("server:{}:cert_key", id.0)),
            },
            |id: ServerId| AuthMethod::IdentityFile {
                path: "~/.ssh/id_ed25519".to_string(),
                passphrase_ref: Some(CredentialRef::new(format!(
                    "server:{}:identity_passphrase",
                    id.0
                ))),
            },
            |_id: ServerId| AuthMethod::Agent,
        ]
        .into_iter(),
    ) {
        let id = ServerId::new();
        let auth = auth_for(id);
        // Der feste Sudo-Slot gehört zu **jeder** Anmeldeart (A10).
        expected.push(format!("server:{}:sudo_password", id.0));
        match &auth {
            AuthMethod::Password { credential_ref } => {
                expected.push(credential_ref.as_str().to_string())
            }
            AuthMethod::Certificate { cert_ref, key_ref } => {
                expected.push(cert_ref.as_str().to_string());
                expected.push(key_ref.as_str().to_string());
            }
            AuthMethod::IdentityFile { passphrase_ref, .. } => {
                expected.push(passphrase_ref.as_ref().unwrap().as_str().to_string())
            }
            AuthMethod::Agent => {}
            AuthMethod::PrivateKey { .. } => unreachable!("in diesem Test nicht benutzt"),
        }
        let now = chrono::Utc::now();
        store
            .create_server(&Server {
                id,
                name: format!("T11-auth-{index}"),
                host: "host-0101.example".to_string(),
                port: 22,
                username: "user-0101".to_string(),
                group_id: None,
                tags: Vec::new(),
                auth,
                notes: String::new(),
                jump_host: None,
                post_ingest_policy: PostIngestPolicy::default(),
                ai_injection_check_enabled: false,
                sftp_server_path: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
    }

    // Der Provider-Zweig.
    let provider_id = ProviderId::new();
    let provider_ref = format!("ai-provider:{}", provider_id.0);
    expected.push(provider_ref.clone());
    let now = chrono::Utc::now();
    store
        .ai_provider_store()
        .create(&AiProviderConfig {
            id: provider_id,
            provider_type: ProviderType::Anthropic,
            display_name: "T11-Provider".to_string(),
            base_url: None,
            model: "claude-sonnet-5".to_string(),
            supports_native_tool_calling: true,
            credential_ref: CredentialRef::new(provider_ref.clone()),
            is_active: false,
            extra_headers: Vec::new(),
            attestation_url: None,
            max_tokens_override: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();

    let keyring = TestKeyring::default();
    for reference in &expected {
        keyring
            .set(
                &CredentialRef::new(reference.clone()),
                SecretString::from(format!("{MARKER}-{reference}")),
            )
            .unwrap();
    }
    assert_eq!(
        keyring.count(),
        expected.len(),
        "der Aufbau muss jeden erwarteten Slot belegen"
    );

    let prompt = ScriptedPrompt::new(Vec::new());
    migrate_secrets_into_database(&store, &keyring, &database, &prompt, false)
        .await
        .expect("der Umzug muss gelingen");

    for reference in &expected {
        assert_eq!(
            database
                .get(&CredentialRef::new(reference.clone()))
                .unwrap_or_else(|err| panic!("{reference} muss umgezogen sein, war aber {err:?}"))
                .expose_secret(),
            format!("{MARKER}-{reference}"),
            "{reference} muss unverändert umgezogen sein"
        );
    }
    assert_eq!(
        keyring.count(),
        0,
        "auch das Provider-Secret muss aus dem Schlüsselbund verschwinden"
    );
    let (state, pending) = store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_DONE);
    assert!(pending.is_empty());

    // Den Pool ausdrücklich schließen, nicht nur fallen lassen: Dieser Test
    // benutzt zusätzlich den `ai_provider_store`, und ein beim
    // Runtime-Abbau noch offener Pool beendete den Testprozess hier
    // reproduzierbar mit SIGSEGV — der Test war dabei schon grün, der
    // Prozess-Rückgabewert aber rot. Siehe Bericht.
    store.close().await;
}

/// T11, Hauptfall: alles umgezogen, der fehlende Slot bleibt `NotFound`,
/// alle Einträge gelöscht, Zustand *erledigt*.
///
/// **Gegenbeweis:** Ohne den Aufruf von `migrate_secrets_into_database`
/// bleibt der Datenbank-Store leer — der Test prüft genau das, was der
/// Schritt tut, und scheitert ohne ihn in der ersten Zusicherung.
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_every_secret_moves_and_every_entry_is_deleted() {
    let fixture = fixture().await;
    let missing = format!("server:{}:sudo_password", fixture.servers[1].0);
    let keyring = keyring_for(&fixture, Some(&missing));
    let before = keyring.count();
    assert_eq!(before, 5, "zwei Server, ein Slot fehlt");
    let prompt = ScriptedPrompt::new(Vec::new());

    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .expect("der Umzug muss gelingen");

    assert!(
        prompt.asked.lock().unwrap().is_empty(),
        "ohne Fehler darf kein Dialog erscheinen"
    );
    // Jeder vorhandene Wert steht jetzt in der Datenbank.
    for id in &fixture.servers {
        for slot in ["private_key", "passphrase", "sudo_password"] {
            let reference = CredentialRef::new(format!("server:{}:{slot}", id.0));
            if reference.as_str() == missing {
                assert!(
                    matches!(
                        fixture.database.get(&reference),
                        Err(CredentialError::NotFound(_))
                    ),
                    "ein fehlender Slot bleibt NotFound, er wird nicht erfunden"
                );
                continue;
            }
            assert_eq!(
                fixture.database.get(&reference).unwrap().expose_secret(),
                format!("{MARKER}-{slot}"),
                "{reference:?} muss umgezogen sein"
            );
        }
    }
    // Und im Schlüsselbund liegt nichts mehr (E7).
    assert_eq!(keyring.count(), 0, "alle Einträge müssen gelöscht sein");
    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_DONE);
    assert!(pending.is_empty());

    // Ein zweiter Start fasst den Schlüsselbund nicht mehr an.
    let deletes_after_first = keyring.deletes();
    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .unwrap();
    assert_eq!(
        keyring.deletes(),
        deletes_after_first,
        "im Zustand *erledigt* darf kein weiteres delete laufen"
    );
}

/// T11, Variante: ein `get` scheitert → **nichts** gelöscht, Zustand bleibt
/// *offen*, und es erscheint D1 **ohne** Einrichten (A11).
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_a_failing_read_deletes_nothing_and_keeps_the_state_open() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    let failing = format!("server:{}:private_key", fixture.servers[0].0);
    *keyring.failing_get.lock().unwrap() = Some(failing.clone());
    let before = keyring.count();
    let prompt = ScriptedPrompt::new(vec![StartupChoice::Quit]);

    let err =
        migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
            .await
            .expect_err("ein Lesefehler darf den Start nicht stillschweigend fortsetzen");
    assert!(matches!(err, StartupAbort::UserQuit));

    assert_eq!(
        prompt.asked.lock().unwrap().clone(),
        vec![StartupDialog::MigrationUnreadable {
            offers_skip_migration: false
        }],
        "A11: der Umzugs-Dialog, der das Einrichten per Konstruktion nicht \
         anbieten kann — und im Schlüsselbund-Modus auch kein Überspringen"
    );
    assert_eq!(keyring.deletes(), 0, "es darf nichts gelöscht werden");
    assert_eq!(keyring.count(), before);
    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_OPEN, "der Zustand bleibt *offen*");
    assert!(pending.is_empty());

    // „Erneut versuchen“ mit danach antwortendem Schlüsselbund startet.
    *keyring.failing_get.lock().unwrap() = None;
    let prompt = ScriptedPrompt::new(vec![StartupChoice::Retry]);
    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .expect("nach „Erneut versuchen“ muss der Umzug gelingen");
    assert_eq!(keyring.count(), 0);
}

/// T11, Variante: `delete` scheitert → die App **startet**, und der nächste
/// Start löscht erneut (A11, zweite Hälfte).
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_a_failing_delete_still_starts_and_is_retried() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    let stubborn = format!("server:{}:passphrase", fixture.servers[0].0);
    *keyring.failing_delete.lock().unwrap() = Some(stubborn.clone());
    let prompt = ScriptedPrompt::new(Vec::new());

    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .expect("ein Löschfehler darf den Start nicht aufhalten");

    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_MOVED, "noch nicht erledigt");
    assert_eq!(pending, vec![stubborn.clone()], "nur der eine bleibt übrig");
    assert!(keyring.has(&stubborn));
    assert_eq!(keyring.count(), 1, "alles andere ist weg");

    // Nächster Start: nur noch dieser eine Versuch, und diesmal gelingt er.
    *keyring.failing_delete.lock().unwrap() = None;
    let deletes_before = keyring.deletes();
    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .unwrap();
    assert_eq!(
        keyring.deletes() - deletes_before,
        1,
        "der nächste Start löscht genau die Liste, nicht alles erneut"
    );
    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_DONE);
    assert!(pending.is_empty());
    assert_eq!(keyring.count(), 0);
}

/// T11, Variante: Ein Server wird gelöscht, **während** das Löschen
/// aussteht — seine Einträge werden trotzdem gelöscht.
///
/// Das ist die Zusicherung, für die die Liste überhaupt festgehalten wird
/// (A10). **Gegenbeweis:** Würde nach dem Datenbankstand gelöscht, blieben
/// die drei Einträge des gelöschten Servers für immer im Schlüsselbund —
/// die letzte Zusicherung dieses Tests scheitert dann.
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_entries_of_a_server_deleted_while_pending_are_still_removed() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    let doomed = fixture.servers[0];
    // Jedes `delete` scheitert einmal, damit der Zustand auf *umgezogen*
    // stehen bleibt und die Liste erhalten ist.
    let stubborn = format!("server:{}:private_key", doomed.0);
    *keyring.failing_delete.lock().unwrap() = Some(stubborn.clone());
    let prompt = ScriptedPrompt::new(Vec::new());

    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .unwrap();
    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_MOVED);
    assert_eq!(pending, vec![stubborn.clone()]);

    // Jetzt verschwindet der Server aus der Datenbank.
    fixture.store.delete_server(&doomed).await.unwrap();
    assert!(fixture
        .store
        .list_servers()
        .await
        .unwrap()
        .iter()
        .all(|s| s.id != doomed));

    *keyring.failing_delete.lock().unwrap() = None;
    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .unwrap();

    assert!(
        !keyring.has(&stubborn),
        "der Eintrag des gelöschten Servers muss trotzdem verschwinden — \
         gelöscht wird nach der festgehaltenen Liste, nicht nach dem \
         Datenbankstand"
    );
    assert_eq!(keyring.count(), 0);
    let (state, _) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_DONE);
}

/// T11, Variante „Schlüsselbund-Modus mit scheiterndem `get`": **keine**
/// Option „Ohne Übernahme fortfahren". Die gehört zu A11.1 und damit zum
/// Passwort-Modus (Etappe 3).
///
/// Seit Etappe 3 gibt es `StartupChoice::ContinueWithoutMigration` — der
/// Test hält fest, dass sie im Schlüsselbund-Modus nicht angeboten wird und
/// der Zustand *übersprungen* dort auf keinem Weg entsteht.
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_the_keychain_mode_offers_no_skip_option() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    *keyring.failing_get.lock().unwrap() =
        Some(format!("server:{}:private_key", fixture.servers[0].0));
    let prompt = ScriptedPrompt::new(vec![StartupChoice::Quit]);

    let _ =
        migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
            .await;

    assert_eq!(
        prompt.asked.lock().unwrap().clone(),
        vec![StartupDialog::MigrationUnreadable {
            offers_skip_migration: false
        }]
    );
    let (state, _) = fixture.store.secret_migration_state().await.unwrap();
    assert_ne!(
        state, STATE_SKIPPED,
        "der Zustand *übersprungen* darf im Schlüsselbund-Modus nicht entstehen"
    );
}

/// T11, Variante *übersprungen* (A11.1): Im Passwort-Modus bietet der
/// Dialog „Ohne Übernahme fortfahren" an; danach wird **nie** ein
/// Schlüsselbund-Eintrag gelöscht, auch wenn der Schlüsselbund später
/// wieder antwortet.
#[tokio::test(flavor = "multi_thread")]
async fn test_t11_skipped_never_deletes_anything_even_once_the_keychain_works() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    *keyring.failing_get.lock().unwrap() =
        Some(format!("server:{}:private_key", fixture.servers[0].0));
    let before = keyring.count();
    let prompt = ScriptedPrompt::new(vec![StartupChoice::ContinueWithoutMigration]);

    migrate_secrets_into_database(
        &fixture.store,
        &keyring,
        &fixture.database,
        &prompt,
        // A11.1: nur im Passwort-Modus.
        true,
    )
    .await
    .expect("„Ohne Übernahme fortfahren“ lässt den Start weiterlaufen");

    assert_eq!(
        prompt.asked.lock().unwrap().clone(),
        vec![StartupDialog::MigrationUnreadable {
            offers_skip_migration: true
        }]
    );
    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_SKIPPED);
    assert!(pending.is_empty(), "es ist nichts zum Löschen vorgemerkt");
    assert_eq!(keyring.deletes(), 0);
    assert_eq!(keyring.count(), before, "kein Eintrag angefasst");

    // **Der eigentliche Punkt von A11.1:** Der Schlüsselbund antwortet
    // wieder — und trotzdem wird nichts gelöscht. Ohne den eigenen
    // `skipped`-Zweig (etwa mit einem `moved` und leerer Liste) würde
    // dieser zweite Start die Einträge nach dem Datenbankstand einsammeln
    // und löschen.
    *keyring.failing_get.lock().unwrap() = None;
    let prompt = ScriptedPrompt::new(vec![]);
    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, true)
        .await
        .expect("ein zweiter Start darf nicht scheitern");

    assert!(
        prompt.asked.lock().unwrap().is_empty(),
        "im Zustand *übersprungen* wird nicht erneut gefragt"
    );
    assert_eq!(
        keyring.deletes(),
        0,
        "A11.1: in *übersprungen* wird nie ein Eintrag gelöscht"
    );
    assert_eq!(keyring.count(), before);
    let (state, _) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_SKIPPED, "der Zustand bleibt *übersprungen*");
}

/// A11.1, Gegenprobe zur Schranke: Die Wahl „Ohne Übernahme fortfahren"
/// wird **auch dann** abgewiesen, wenn sie aus einer Oberfläche kommt, die
/// sie nicht angeboten bekam.
///
/// Die Prüfung steht deshalb zweimal im Code — beim Anbieten und beim
/// Auswerten. Im Schlüsselbund-Modus würde *übersprungen* Secrets dauerhaft
/// unerreichbar machen, obwohl sie im erreichbaren Schlüsselbund liegen.
#[tokio::test(flavor = "multi_thread")]
async fn test_a11_1_skip_is_refused_when_it_was_not_offered() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    *keyring.failing_get.lock().unwrap() =
        Some(format!("server:{}:private_key", fixture.servers[0].0));
    let prompt = ScriptedPrompt::new(vec![StartupChoice::ContinueWithoutMigration]);

    let err =
        migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
            .await
            .expect_err("eine nicht angebotene Wahl darf nicht wirken");
    assert!(matches!(err, StartupAbort::UserQuit));

    let (state, _) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_OPEN, "der Zustand bleibt *offen*");
    assert_eq!(keyring.deletes(), 0);
}

/// **Der Umzug darf K nie anfassen** (spec-reviewer Runde 1, Spec 0101
/// Angriffsrichtung „Abgebrochener Moduswechsel löscht die einzige Kopie
/// von K").
///
/// Die Referenzen kommen wörtlich aus der Datenbank. Steht im
/// `auth_method`-JSON eines Servers — das bis zur Umwandlung im Klartext
/// auf der Platte liegt — die Referenz des Wurzelschlüssels, dann würde ein
/// Umzug ohne Schema-Prüfung K in die Secrets-Tabelle kopieren und danach
/// aus dem Schlüsselbund löschen. Beim nächsten Start wäre die Datei
/// verschlüsselt und der Schlüssel weg.
///
/// **Gegenbeweis geführt:** Ohne `is_migratable` ist K nach diesem Test aus
/// dem Test-Schlüsselbund verschwunden und die letzte Zusicherung scheitert.
#[tokio::test(flavor = "multi_thread")]
async fn test_a10_a_reference_pointing_at_the_root_key_is_never_migrated() {
    let fixture = fixture().await;
    let id = fixture.servers[0];
    let mut server = fixture.store.get_server(&id).await.unwrap();
    server.auth = AuthMethod::Password {
        credential_ref: CredentialRef::new(
            ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
        ),
    };
    fixture.store.update_server(&server).await.unwrap();

    let keyring = TestKeyring::with(&[(
        ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF,
        "Wurzelschlüssel-0101",
    )]);
    let prompt = ScriptedPrompt::new(Vec::new());

    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .expect("eine fremde Referenz darf den Start nicht aufhalten");

    assert!(
        matches!(
            fixture.database.get(&CredentialRef::new(
                ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string()
            )),
            Err(CredentialError::NotFound(_))
        ),
        "K darf nicht in die Secrets-Tabelle kopiert werden"
    );
    assert_eq!(keyring.deletes(), 0, "an K wird kein delete versucht");
    assert!(
        keyring.has(ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF),
        "K muss im Schlüsselbund liegen bleiben — sonst ist die Datenbank verloren"
    );
}

/// **Auch die festgehaltene Löschliste wird geprüft, nicht nur das
/// Aufsammeln** — der Nachweis für die zweite Schranke (spec-reviewer
/// Runde 2: der Test oben deckt nur die erste ab, weil die Filterung beim
/// Aufsammeln den Löschweg nie erreicht).
///
/// Die Liste in der Datenbank ist selbst eine Datenquelle. Zwei Wege führen
/// zu einer Liste, die K nennt, obwohl das Aufsammeln ihn heute aussperrt:
/// Jemand verändert die Zeile von Hand — oder die Installation hat die
/// Liste noch mit dem ungefixten Stand (`05b277c`) geschrieben und wird
/// jetzt aktualisiert. Genau dieser Upgrade-Fall ist der Grund, dass die
/// Prüfung an **beiden** Stellen steht.
///
/// **Gegenbeweis geführt:** Ohne die Schranke in `delete_moved_entries`
/// wird K aus dem Test-Schlüsselbund gelöscht — `deletes()` ist dann 1 und
/// `has(K)` falsch, beide letzten Zusicherungen scheitern. Die Schranke
/// beim Aufsammeln hilft hier nicht: Der Zustand *moved* kehrt vor ihr
/// zurück.
#[tokio::test(flavor = "multi_thread")]
async fn test_a10_a_tampered_pending_list_never_deletes_the_root_key() {
    let fixture = fixture().await;
    let keyring = TestKeyring::with(&[(
        ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF,
        "Wurzelschlüssel-0101",
    )]);

    fixture
        .store
        .set_secret_migration_state(
            STATE_MOVED,
            &[ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string()],
        )
        .await
        .unwrap();

    let prompt = ScriptedPrompt::new(Vec::new());
    migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
        .await
        .expect("eine fremde Referenz in der Liste darf den Start nicht aufhalten");

    assert_eq!(
        keyring.deletes(),
        0,
        "an K wird kein delete versucht, auch nicht aus der festgehaltenen Liste"
    );
    assert!(
        keyring.has(ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF),
        "K muss im Schlüsselbund liegen bleiben — sonst ist die Datenbank verloren"
    );
    let (state, pending) = fixture.store.secret_migration_state().await.unwrap();
    assert_eq!(
        (state.as_str(), pending.as_slice()),
        (STATE_DONE, &[] as &[String]),
        "die übersprungene Referenz darf die Liste nicht dauerhaft offen halten"
    );
}

/// A10: Eine frische Installation hat nichts umzuziehen und fasst den
/// Schlüsselbund **nicht** an — kein `get`, kein `delete`.
#[tokio::test(flavor = "multi_thread")]
async fn test_a10_a_fresh_installation_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteProfileStore::connect_encrypted(
        &dir.path().join("smart-ssh.db"),
        &DatabaseKey::from_root_key(&TEST_ROOT_KEY),
    )
    .await
    .unwrap();
    let database = store.credential_store(tokio::runtime::Handle::current());
    let keyring = TestKeyring::with(&[("server:fremd:password", "nicht meins")]);
    let prompt = ScriptedPrompt::new(Vec::new());

    migrate_secrets_into_database(&store, &keyring, &database, &prompt, false)
        .await
        .unwrap();

    assert_eq!(keyring.deletes(), 0);
    assert!(
        keyring.has("server:fremd:password"),
        "ein Eintrag, auf den keine Referenz zeigt, wird nicht angefasst"
    );
    let (state, _) = store.secret_migration_state().await.unwrap();
    assert_eq!(state, STATE_DONE);
    store.close().await;
}

/// **Ein gescheiterter Umzug rät nicht zum Backup** (spec-reviewer Runde 1:
/// jeder fatale Umzugsfehler benutzte `ConnectFailureKind::Other`, dessen
/// Text zum Backup rät — an einer Stelle, an der die Datenbank gerade
/// erfolgreich geöffnet wurde).
///
/// Der Rat ist hier nicht nur nutzlos, sondern im Fall „Zustand nicht
/// schreibbar, Einträge schon gelöscht" aktiv schädlich: Ein eingespieltes
/// Backup löste den Umzug erneut aus, während die Secrets im Schlüsselbund
/// schon weg sind.
///
/// **Gegenbeweis geführt:** Gegen den Stand vor dieser Änderung liefert
/// derselbe Ablauf `ConnectFailureKind::Other`, und die Zusicherung auf
/// `SecretMigrationFailed` scheitert; der zugehörige Text enthält dann
/// „Backup" bzw. „backup".
#[tokio::test(flavor = "multi_thread")]
async fn test_a11_a_failed_migration_gets_its_own_kind_without_backup_advice() {
    let fixture = fixture().await;
    let keyring = keyring_for(&fixture, None);
    let prompt = ScriptedPrompt::new(Vec::new());
    // Die Datenbank war offen und fällt mitten im Umzug weg — genau die
    // Lage, in der „Datei beschädigt, spiel ein Backup ein" falsch ist.
    fixture.store.close().await;

    let err =
        migrate_secrets_into_database(&fixture.store, &keyring, &fixture.database, &prompt, false)
            .await
            .expect_err(
                "ein nicht erreichbarer Store darf den Start nicht stillschweigend fortsetzen",
            );

    match err {
        StartupAbort::Fatal { kind, .. } => assert_eq!(
            kind,
            ConnectFailureKind::SecretMigrationFailed,
            "der Umzug braucht seinen eigenen Fall — `Other` rät zum Backup"
        ),
        other => panic!("erwartet war ein fataler Startfehler, kam: {other:?}"),
    }
    assert_eq!(
        keyring.deletes(),
        0,
        "ein gescheiterter Umzug löscht keinen Schlüsselbund-Eintrag"
    );

    for language in [
        crate::startup_error_messages::Language::De,
        crate::startup_error_messages::Language::En,
    ] {
        let text = crate::startup_error_messages::db_connect_failure_text(
            &ConnectFailureKind::SecretMigrationFailed,
            std::path::Path::new("/tmp/smart-ssh.db"),
            std::path::Path::new("/tmp/logs"),
            language,
        );
        let lower = text.message.to_lowercase();
        assert!(
            !lower.contains("backup"),
            "der Text zum gescheiterten Umzug darf kein Backup empfehlen ({language:?}): {}",
            text.message
        );
    }
}
