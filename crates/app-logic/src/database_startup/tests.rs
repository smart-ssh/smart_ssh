//! Spec 0101, Commit 5: T3 (Entscheidungstabelle A3/A4), T7 (A5, D4),
//! T8 (D1).
//!
//! **Multi-Thread-Runtime** wie in `persistence_sqlite::tests_encryption` —
//! der Startablauf öffnet und schließt mehrere Verbindungen, und die
//! Produktivumgebung (Tauris Async-Runtime) ist Multi-Thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use secrecy::SecretString;

use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use persistence_sqlite::{
    detect_database_file_state, DatabaseFileState, SqliteProfileStore, SQLITE_PLAINTEXT_HEADER,
};
use ssh_manager_core::crypto::{DatabaseKey, CHAT_CONTENT_ENCRYPTION_KEY_REF};
use ssh_manager_core::profiles::{CredentialError, CredentialRef, CredentialResult, ProfileStore};

use super::*;

/// Fester Wurzelschlüssel für die Tests — kein Geheimnis.
const TEST_ROOT_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

fn root_key_base64() -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(TEST_ROOT_KEY)
}

/// Wie sich der Test-Schlüsselbund beim Lesen verhält.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GetBehaviour {
    /// Ein gültiger Schlüssel liegt vor.
    Present,
    /// Kein Eintrag.
    Missing,
    /// Backend-Fehler.
    Failing,
    /// Eintrag da, aber kein gültiger 256-Bit-Schlüssel.
    Corrupt,
}

/// Test-`CredentialStore`, der **zählt** (T3: „mit Test-Store, der Aufrufe
/// zählt"). Nur so lässt sich die Zusicherung prüfen, dass ohne Nutzerwahl
/// genau in zwei Feldern der Tabelle ein `set` auf K passiert.
struct CountingCredentialStore {
    behaviour: Mutex<GetBehaviour>,
    entries: Mutex<HashMap<String, SecretString>>,
    gets: AtomicUsize,
    sets: AtomicUsize,
    deletes: AtomicUsize,
}

impl CountingCredentialStore {
    fn new(behaviour: GetBehaviour) -> Self {
        let entries = Mutex::new(HashMap::new());
        if behaviour == GetBehaviour::Present {
            entries.lock().unwrap().insert(
                CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
                SecretString::from(root_key_base64()),
            );
        } else if behaviour == GetBehaviour::Corrupt {
            entries.lock().unwrap().insert(
                CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
                SecretString::from("kein-base64!!!".to_string()),
            );
        }
        Self {
            behaviour: Mutex::new(behaviour),
            entries,
            gets: AtomicUsize::new(0),
            sets: AtomicUsize::new(0),
            deletes: AtomicUsize::new(0),
        }
    }

    fn set_behaviour(&self, behaviour: GetBehaviour) {
        if behaviour == GetBehaviour::Present {
            self.entries.lock().unwrap().insert(
                CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
                SecretString::from(root_key_base64()),
            );
        }
        *self.behaviour.lock().unwrap() = behaviour;
    }

    fn sets(&self) -> usize {
        self.sets.load(Ordering::SeqCst)
    }
}

impl CredentialStore for CountingCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        match *self.behaviour.lock().unwrap() {
            GetBehaviour::Failing => Err(CredentialError::Backend("Keychain gesperrt".into())),
            _ => self
                .entries
                .lock()
                .unwrap()
                .get(r.as_str())
                .cloned()
                .ok_or_else(|| CredentialError::NotFound(r.clone())),
        }
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        self.sets.fetch_add(1, Ordering::SeqCst);
        if *self.behaviour.lock().unwrap() == GetBehaviour::Failing {
            return Err(CredentialError::Backend("Keychain gesperrt".into()));
        }
        self.entries
            .lock()
            .unwrap()
            .insert(r.as_str().to_string(), value);
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.entries.lock().unwrap().remove(r.as_str());
        Ok(())
    }
}

/// Test-Dialog, der jede Frage aufzeichnet und vorgegebene Antworten
/// liefert.
struct ScriptedPrompt {
    answers: Mutex<Vec<StartupChoice>>,
    confirm_start_over: bool,
    confirm_new_key: bool,
    asked: Mutex<Vec<StartupDialog>>,
    confirm_texts: Mutex<Vec<String>>,
    notified: Mutex<Vec<String>>,
}

impl ScriptedPrompt {
    fn new(answers: Vec<StartupChoice>) -> Self {
        Self {
            answers: Mutex::new(answers),
            confirm_start_over: true,
            confirm_new_key: true,
            asked: Mutex::new(Vec::new()),
            confirm_texts: Mutex::new(Vec::new()),
            notified: Mutex::new(Vec::new()),
        }
    }

    fn without_second_confirmation(mut self) -> Self {
        self.confirm_start_over = false;
        self.confirm_new_key = false;
        self
    }

    fn asked(&self) -> Vec<StartupDialog> {
        self.asked.lock().unwrap().clone()
    }
}

impl StartupPrompt for ScriptedPrompt {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        self.asked.lock().unwrap().push(dialog);
        let mut answers = self.answers.lock().unwrap();
        if answers.is_empty() {
            StartupChoice::Quit
        } else {
            answers.remove(0)
        }
    }

    fn confirm_start_over(&self, renamed_to: &str) -> bool {
        self.confirm_texts
            .lock()
            .unwrap()
            .push(renamed_to.to_string());
        self.confirm_start_over
    }

    fn confirm_generate_new_key(&self) -> bool {
        self.confirm_new_key
    }

    fn notify_started_over(&self, renamed_to: &str) {
        self.notified.lock().unwrap().push(renamed_to.to_string());
    }
}

/// Ein Dialog-Doppel, das **jede** Frage mit einem Panic beantwortet — für
/// die Felder der Tabelle, in denen überhaupt kein Dialog erscheinen darf.
struct NoDialogExpected;

impl StartupPrompt for NoDialogExpected {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        panic!("in diesem Feld der Tabelle A3 darf kein Dialog erscheinen: {dialog:?}");
    }
    fn confirm_start_over(&self, _renamed_to: &str) -> bool {
        panic!("in diesem Feld darf keine Bestätigung erscheinen");
    }
    fn confirm_generate_new_key(&self) -> bool {
        panic!("in diesem Feld darf keine Bestätigung erscheinen");
    }
    fn notify_started_over(&self, _renamed_to: &str) {
        panic!("in diesem Feld darf nichts umbenannt werden");
    }
}

fn available() -> KeychainAvailability {
    KeychainAvailability::Available
}

fn unavailable(reason: KeychainUnavailableReason) -> KeychainAvailability {
    KeychainAvailability::Unavailable(reason)
}

// === T3: die Entscheidungstabelle, Feld für Feld =========================

/// T3 (A3): Alle **zwölf** Felder der Tabelle als reine Entscheidung.
/// Diese Fassung prüft die Tabelle selbst; die Tests darunter prüfen, dass
/// der Ablauf sich auch so verhält.
///
/// Scheitert am Stand vor dieser Spec schon deshalb, weil es dort keine
/// Tabelle gab — `resolve_or_generate_key` erzeugte bei einer
/// verschlüsselten Datei einen neuen Schlüssel (Feld *sonst* ×
/// *NotFound*), statt D2 zu fragen.
#[test]
fn test_t3_every_field_of_the_decision_table() {
    use DatabaseFileState::*;
    use KeyState::*;

    let no_reason = Unreachable(None);
    let no_bus = Unreachable(Some(KeychainUnavailableReason::NoSessionBus));
    let no_provider = Unreachable(Some(KeychainUnavailableReason::NoSecretServiceProvider));
    let locked = Unreachable(Some(KeychainUnavailableReason::Locked));

    // Zeile „fehlt“
    assert_eq!(decide_startup(Missing, &Present), StartupPlan::CreateFresh);
    assert_eq!(
        decide_startup(Missing, &NotFound),
        StartupPlan::GenerateKeyThenCreateFresh
    );
    assert_eq!(
        decide_startup(Missing, &no_reason),
        StartupPlan::Dialog(StartupDialog::D1 {
            offers_password_setup: false
        })
    );
    assert_eq!(
        decide_startup(Missing, &Invalid),
        StartupPlan::Dialog(StartupDialog::D3)
    );

    // Zeile „Klartext“
    assert_eq!(decide_startup(Plaintext, &Present), StartupPlan::Convert);
    assert_eq!(
        decide_startup(Plaintext, &NotFound),
        StartupPlan::GenerateKeyThenConvert
    );
    assert_eq!(
        decide_startup(Plaintext, &no_reason),
        StartupPlan::Dialog(StartupDialog::D1 {
            offers_password_setup: false
        })
    );
    assert_eq!(
        decide_startup(Plaintext, &Invalid),
        StartupPlan::Dialog(StartupDialog::D4)
    );

    // Zeile „sonst“
    assert_eq!(decide_startup(Other, &Present), StartupPlan::OpenExisting);
    assert_eq!(
        decide_startup(Other, &NotFound),
        StartupPlan::Dialog(StartupDialog::D2)
    );
    assert_eq!(
        decide_startup(Other, &no_reason),
        StartupPlan::Dialog(StartupDialog::D1 {
            offers_password_setup: false
        })
    );
    assert_eq!(
        decide_startup(Other, &Invalid),
        StartupPlan::Dialog(StartupDialog::D3)
    );

    // D1 bietet das Einrichten nur bei den zwei Gründen, bei denen kein
    // erreichbarer K existieren kann — und bei Datei *sonst* nie.
    for reason in [&no_bus, &no_provider] {
        for file in [Missing, Plaintext] {
            assert_eq!(
                decide_startup(file, reason),
                StartupPlan::Dialog(StartupDialog::D1 {
                    offers_password_setup: true
                }),
                "{file:?} × {reason:?} muss das Einrichten anbieten"
            );
        }
        assert_eq!(
            decide_startup(Other, reason),
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup: false
            }),
            "bei Datei *sonst* darf D1 das Einrichten nie anbieten ({reason:?})"
        );
    }
    // Ein gesperrter Schlüsselbund kann den echten K enthalten — kein
    // Einrichten, nirgends.
    for file in [Missing, Plaintext, Other] {
        assert_eq!(
            decide_startup(file, &locked),
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup: false
            }),
            "{file:?} × Locked darf das Einrichten nicht anbieten"
        );
    }
}

/// T3: `read_key_state` verwechselt die Fehlerarten nicht — die
/// Angriffsrichtung „`Backend` als `NotFound`“.
#[test]
fn test_t3_key_state_never_confuses_backend_with_notfound() {
    let failing = CountingCredentialStore::new(GetBehaviour::Failing);
    assert_eq!(
        read_key_state(&failing, available()).0,
        KeyState::Unreachable(None)
    );

    let missing = CountingCredentialStore::new(GetBehaviour::Missing);
    assert_eq!(read_key_state(&missing, available()).0, KeyState::NotFound);

    let corrupt = CountingCredentialStore::new(GetBehaviour::Corrupt);
    assert_eq!(read_key_state(&corrupt, available()).0, KeyState::Invalid);

    let present = CountingCredentialStore::new(GetBehaviour::Present);
    let (state, key) = read_key_state(&present, available());
    assert_eq!(state, KeyState::Present);
    assert_eq!(key, Some(TEST_ROOT_KEY));
}

/// A3: Ist der Schlüsselbund beim Start als nicht verfügbar erkannt, wird
/// er **gar nicht** gefragt — und der Grund aus der Probe bleibt erhalten
/// (D1 hängt daran).
#[test]
fn test_a3_an_unavailable_keychain_is_not_queried_at_all() {
    let store = CountingCredentialStore::new(GetBehaviour::Present);
    let keychain = unavailable(KeychainUnavailableReason::NoSessionBus);

    let (state, key) = read_key_state(&store, keychain);

    assert_eq!(
        state,
        KeyState::Unreachable(Some(KeychainUnavailableReason::NoSessionBus))
    );
    assert_eq!(key, None);
    assert_eq!(
        store.gets.load(Ordering::SeqCst),
        0,
        "ein als nicht verfügbar erkannter Schlüsselbund darf nicht gefragt werden"
    );
    assert_eq!(store.sets(), 0);
}

/// T3: In den beiden „K erzeugen“-Feldern entsteht ohne Nutzerwahl genau
/// **ein** `set` auf K — und in jedem anderen Feld **keines**.
#[tokio::test(flavor = "multi_thread")]
async fn test_t3_a_key_is_written_without_a_user_choice_only_in_the_two_generate_fields() {
    // --- Feld „fehlt × NotFound“: K erzeugen, neu anlegen.
    {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("smart-ssh.db");
        let store = CountingCredentialStore::new(GetBehaviour::Missing);
        let opened = open_or_prepare_database(&db_path, &store, available(), &NoDialogExpected)
            .await
            .expect("Datei fehlt, kein K: anlegen muss gelingen");
        assert_eq!(store.sets(), 1, "genau ein set auf K erwartet");
        assert_eq!(
            detect_database_file_state(&db_path).unwrap(),
            DatabaseFileState::Other,
            "die neue Datei muss verschlüsselt sein"
        );
        opened.store.close().await;
    }

    // --- Feld „Klartext × NotFound“: K erzeugen, umwandeln.
    {
        let dir = tempfile::tempdir().unwrap();
        let db_path = plaintext_database(dir.path()).await;
        let store = CountingCredentialStore::new(GetBehaviour::Missing);
        let opened = open_or_prepare_database(&db_path, &store, available(), &NoDialogExpected)
            .await
            .expect("Klartext, kein K: umwandeln muss gelingen");
        assert_eq!(store.sets(), 1, "genau ein set auf K erwartet");
        assert_eq!(
            detect_database_file_state(&db_path).unwrap(),
            DatabaseFileState::Other
        );
        opened.store.close().await;
    }

    // --- Feld „fehlt × Present“: kein set.
    {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("smart-ssh.db");
        let store = CountingCredentialStore::new(GetBehaviour::Present);
        let opened = open_or_prepare_database(&db_path, &store, available(), &NoDialogExpected)
            .await
            .expect("Datei fehlt, K da");
        assert_eq!(store.sets(), 0);
        opened.store.close().await;
    }

    // --- Feld „Klartext × Present“: umwandeln, kein set.
    {
        let dir = tempfile::tempdir().unwrap();
        let db_path = plaintext_database(dir.path()).await;
        let store = CountingCredentialStore::new(GetBehaviour::Present);
        let opened = open_or_prepare_database(&db_path, &store, available(), &NoDialogExpected)
            .await
            .expect("Klartext, K da");
        assert_eq!(store.sets(), 0);
        opened.store.close().await;
    }

    // --- Feld „sonst × Present“: öffnen, kein set.
    {
        let dir = tempfile::tempdir().unwrap();
        let db_path = encrypted_database(dir.path()).await;
        let store = CountingCredentialStore::new(GetBehaviour::Present);
        let opened = open_or_prepare_database(&db_path, &store, available(), &NoDialogExpected)
            .await
            .expect("verschlüsselt, K da");
        assert_eq!(store.sets(), 0);
        opened.store.close().await;
    }
}

/// T3: In **jedem** Dialogfall bleibt die Datei byte-gleich, solange nichts
/// gewählt ist — und es gibt nie einen Migrationsfehler („nie Code 7“).
#[tokio::test(flavor = "multi_thread")]
async fn test_t3_in_every_dialog_case_the_file_stays_byte_identical_until_a_choice_is_made() {
    struct Case {
        name: &'static str,
        behaviour: GetBehaviour,
        keychain: KeychainAvailability,
        expected_dialog: StartupDialog,
    }

    let cases = [
        Case {
            name: "sonst × NotFound → D2",
            behaviour: GetBehaviour::Missing,
            keychain: available(),
            expected_dialog: StartupDialog::D2,
        },
        Case {
            name: "sonst × nicht erreichbar → D1 ohne Einrichten",
            behaviour: GetBehaviour::Failing,
            keychain: available(),
            expected_dialog: StartupDialog::D1 {
                offers_password_setup: false,
            },
        },
        Case {
            name: "sonst × ungültig → D3",
            behaviour: GetBehaviour::Corrupt,
            keychain: available(),
            expected_dialog: StartupDialog::D3,
        },
    ];

    for case in cases {
        let dir = tempfile::tempdir().unwrap();
        let db_path = encrypted_database(dir.path()).await;
        let before = std::fs::read(&db_path).unwrap();
        let mtime_before = std::fs::metadata(&db_path).unwrap().modified().unwrap();

        let store = CountingCredentialStore::new(case.behaviour);
        // Keine Antwort im Skript → der Dialog wird mit „Beenden“
        // beantwortet, es wird also nichts gewählt.
        let prompt = ScriptedPrompt::new(vec![]);
        let result = open_or_prepare_database(&db_path, &store, case.keychain, &prompt).await;

        assert!(
            matches!(result, Err(StartupAbort::UserQuit)),
            "{}: erwartet UserQuit",
            case.name
        );
        assert_eq!(
            prompt.asked(),
            vec![case.expected_dialog],
            "{}: falscher Dialog",
            case.name
        );
        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            before,
            "{}: die Datei muss byte-gleich bleiben",
            case.name
        );
        assert_eq!(
            std::fs::metadata(&db_path).unwrap().modified().unwrap(),
            mtime_before,
            "{}: die mtime muss gleich bleiben",
            case.name
        );
        assert_eq!(store.sets(), 0, "{}: kein set auf K", case.name);
        assert!(
            prompt.confirm_texts.lock().unwrap().is_empty(),
            "{}: ohne Wahl darf keine zweite Bestätigung kommen",
            case.name
        );
    }
}

/// T3/D4: Klartext-Datei und unbrauchbarer Schlüssel → D4, und ohne
/// zweite Bestätigung passiert **nichts**: kein neuer Schlüssel, keine
/// Umwandlung, die Datei byte-gleich.
#[tokio::test(flavor = "multi_thread")]
async fn test_t3_d4_without_the_second_confirmation_nothing_happens() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = plaintext_database(dir.path()).await;
    let before = std::fs::read(&db_path).unwrap();

    let store = CountingCredentialStore::new(GetBehaviour::Corrupt);
    let prompt =
        ScriptedPrompt::new(vec![StartupChoice::GenerateNewKey]).without_second_confirmation();

    let result = open_or_prepare_database(&db_path, &store, available(), &prompt).await;

    assert!(matches!(result, Err(StartupAbort::UserQuit)));
    assert_eq!(prompt.asked(), vec![StartupDialog::D4]);
    assert_eq!(store.sets(), 0, "ohne Bestätigung kein neuer Schlüssel");
    assert_eq!(std::fs::read(&db_path).unwrap(), before);
    assert!(std::fs::read(&db_path)
        .unwrap()
        .starts_with(SQLITE_PLAINTEXT_HEADER));
}

// === T7: „Neu anfangen“ (A5) und D4 ======================================

/// T7 (A5): „Neu anfangen“ aus D2 — Dateien umbenannt und byte-gleich, der
/// Dialogtext nennt den Namen, danach eine neue leere Datenbank.
#[tokio::test(flavor = "multi_thread")]
async fn test_t7_start_over_from_d2_renames_byte_identically_and_starts_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("smart-ssh.db");
    // Eine verschlüsselte Datei mit einem **anderen** Schlüssel — der
    // vorhandene K öffnet sie nicht (D2).
    let mut foreign_root = TEST_ROOT_KEY;
    foreign_root[0] ^= 0xff;
    let foreign_key = DatabaseKey::from_root_key(&foreign_root);
    let store = SqliteProfileStore::connect_encrypted(&db_path, &foreign_key)
        .await
        .unwrap();
    store.close().await;
    let before = std::fs::read(&db_path).unwrap();

    let credentials = CountingCredentialStore::new(GetBehaviour::Present);
    let prompt = ScriptedPrompt::new(vec![StartupChoice::StartOver]);

    let opened = open_or_prepare_database(&db_path, &credentials, available(), &prompt)
        .await
        .expect("nach „Neu anfangen“ muss eine frische Datenbank entstehen");

    assert_eq!(prompt.asked(), vec![StartupDialog::D2]);
    let renamed_to = prompt.confirm_texts.lock().unwrap().clone();
    assert_eq!(
        renamed_to.len(),
        1,
        "genau eine zweite Bestätigung erwartet"
    );
    let renamed_to = renamed_to[0].clone();
    assert!(
        renamed_to.starts_with("smart-ssh.db.unreadable-"),
        "der Dialogtext muss den neuen Namen nennen, war: {renamed_to}"
    );
    assert_eq!(
        *prompt.notified.lock().unwrap(),
        vec![renamed_to.clone()],
        "nach dem Umbenennen muss der Name noch einmal genannt werden"
    );

    // Die alte Datei liegt unter dem neuen Namen und ist byte-gleich.
    let renamed_path = dir.path().join(&renamed_to);
    assert!(renamed_path.exists(), "{renamed_path:?} fehlt");
    assert_eq!(
        std::fs::read(&renamed_path).unwrap(),
        before,
        "die umbenannte Datei muss byte-gleich sein — gelöscht wird nichts"
    );

    // Die neue Datenbank ist leer und mit dem vorhandenen K lesbar.
    assert!(opened.store.list_servers().await.unwrap().is_empty());
    assert_eq!(opened.root_key, TEST_ROOT_KEY);
    assert_eq!(
        credentials.sets(),
        0,
        "der vorhandene K bleibt — kein neuer"
    );
    opened.store.close().await;
}

/// T7 (A5): „Neu anfangen“ aus D3 (unbrauchbarer Schlüssel) — die Datei
/// wandert unter den neuen Namen, **und** der unbrauchbare
/// Schlüsselbund-Eintrag wird ersetzt. Ohne diesen zweiten Teil käme beim
/// nächsten Start wieder D3.
#[tokio::test(flavor = "multi_thread")]
async fn test_t7_start_over_from_d3_also_replaces_the_unusable_key() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = encrypted_database(dir.path()).await;
    let before = std::fs::read(&db_path).unwrap();

    let credentials = CountingCredentialStore::new(GetBehaviour::Corrupt);
    let prompt = ScriptedPrompt::new(vec![StartupChoice::StartOver]);

    let opened = open_or_prepare_database(&db_path, &credentials, available(), &prompt)
        .await
        .expect("nach „Neu anfangen“ aus D3 muss eine frische Datenbank entstehen");

    assert_eq!(prompt.asked(), vec![StartupDialog::D3]);
    assert_eq!(credentials.sets(), 1, "der unbrauchbare K wird ersetzt");
    assert_ne!(
        opened.root_key, TEST_ROOT_KEY,
        "der neue K darf nicht der Testschlüssel sein"
    );

    let renamed_to = prompt.notified.lock().unwrap()[0].clone();
    assert_eq!(std::fs::read(dir.path().join(&renamed_to)).unwrap(), before);
    assert!(opened.store.list_servers().await.unwrap().is_empty());

    // Der Zustand ist danach auflösbar: ein zweiter Start öffnet dieselbe
    // Datei ohne Dialog.
    opened.store.close().await;
    let second = open_or_prepare_database(&db_path, &credentials, available(), &NoDialogExpected)
        .await
        .expect("der zweite Start darf keinen Dialog mehr brauchen");
    second.store.close().await;
}

/// T7 (A5): Ohne zweite Bestätigung passiert **nichts** — keine Datei
/// umbenannt, kein Schlüssel geschrieben.
#[tokio::test(flavor = "multi_thread")]
async fn test_t7_without_the_second_confirmation_no_file_is_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = encrypted_database(dir.path()).await;
    let before = std::fs::read(&db_path).unwrap();
    let files_before = file_names(dir.path());

    let credentials = CountingCredentialStore::new(GetBehaviour::Corrupt);
    let prompt = ScriptedPrompt::new(vec![StartupChoice::StartOver]).without_second_confirmation();

    let result = open_or_prepare_database(&db_path, &credentials, available(), &prompt).await;

    assert!(matches!(result, Err(StartupAbort::UserQuit)));
    assert_eq!(std::fs::read(&db_path).unwrap(), before);
    assert_eq!(file_names(dir.path()), files_before);
    assert_eq!(credentials.sets(), 0);
    assert!(
        prompt.notified.lock().unwrap().is_empty(),
        "ohne Bestätigung darf nichts gemeldet werden"
    );
}

/// T7 (D4): Klartext-Datei, unbrauchbarer Schlüssel, bestätigt → die Datei
/// wird mit einem neuen K umgewandelt, **die Zeilen bleiben erhalten**, und
/// die Datei wird nicht umbenannt.
#[tokio::test(flavor = "multi_thread")]
async fn test_t7_d4_converts_the_plaintext_file_with_a_new_key_and_keeps_the_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = plaintext_database(dir.path()).await;
    let files_before = file_names(dir.path());

    let credentials = CountingCredentialStore::new(GetBehaviour::Corrupt);
    let prompt = ScriptedPrompt::new(vec![StartupChoice::GenerateNewKey]);

    let opened = open_or_prepare_database(&db_path, &credentials, available(), &prompt)
        .await
        .expect("D4 mit Bestätigung muss umwandeln");

    assert_eq!(prompt.asked(), vec![StartupDialog::D4]);
    assert_eq!(credentials.sets(), 1, "genau ein neuer K");
    assert!(
        prompt.notified.lock().unwrap().is_empty(),
        "D4 benennt nichts um"
    );
    // Nichts wurde zur Seite gelegt. (Die offene Datenbank hat jetzt
    // zusätzlich `-wal`/`-shm` — deshalb nicht die Dateiliste vergleichen,
    // sondern gezielt: es gibt keinen `.unreadable-`-Namen.)
    assert!(
        file_names(dir.path())
            .iter()
            .all(|n| !n.contains(".unreadable-")),
        "D4 darf keine Datei zur Seite legen, war: {:?} (vorher {files_before:?})",
        file_names(dir.path())
    );

    let servers = opened.store.list_servers().await.unwrap();
    assert_eq!(servers.len(), 1, "die Zeile muss erhalten bleiben");
    assert_eq!(servers[0].host, "host-0101.example");
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Other
    );
    opened.store.close().await;
}

// === T8: D1 ==============================================================

/// T8 (D1): Ist K nicht erreichbar, wird die Datenbank **nicht geöffnet**
/// (Inhalt und mtime gleich) — und „Erneut versuchen“ mit einem danach
/// funktionierenden Store startet.
#[tokio::test(flavor = "multi_thread")]
async fn test_t8_an_unreachable_key_does_not_open_the_database_and_retry_works() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = encrypted_database(dir.path()).await;
    let before = std::fs::read(&db_path).unwrap();
    let mtime_before = std::fs::metadata(&db_path).unwrap().modified().unwrap();

    let credentials = CountingCredentialStore::new(GetBehaviour::Failing);

    // --- Erster Teil: „Beenden“ — nichts angefasst.
    {
        let prompt = ScriptedPrompt::new(vec![StartupChoice::Quit]);
        let result = open_or_prepare_database(&db_path, &credentials, available(), &prompt).await;
        assert!(matches!(result, Err(StartupAbort::UserQuit)));
        assert_eq!(
            prompt.asked(),
            vec![StartupDialog::D1 {
                offers_password_setup: false
            }]
        );
        assert_eq!(std::fs::read(&db_path).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&db_path).unwrap().modified().unwrap(),
            mtime_before,
            "die Datenbank darf nicht geöffnet worden sein"
        );
        assert_eq!(credentials.sets(), 0);
    }

    // --- Zweiter Teil: „Erneut versuchen“, und beim zweiten Durchlauf
    // antwortet der Store. Der Start muss ohne Neustart durchlaufen.
    {
        struct RetryThenWork<'a> {
            store: &'a CountingCredentialStore,
            asked: Mutex<Vec<StartupDialog>>,
        }
        impl StartupPrompt for RetryThenWork<'_> {
            fn ask(&self, dialog: StartupDialog) -> StartupChoice {
                self.asked.lock().unwrap().push(dialog);
                // Genau das, was ein Nutzer tut: den Schlüsselbund
                // entsperren und dann „Erneut versuchen“ drücken.
                self.store.set_behaviour(GetBehaviour::Present);
                StartupChoice::Retry
            }
            fn confirm_start_over(&self, _renamed_to: &str) -> bool {
                panic!("D1 führt nicht zu „Neu anfangen“")
            }
            fn confirm_generate_new_key(&self) -> bool {
                panic!("D1 führt nicht zu einem neuen Schlüssel")
            }
            fn notify_started_over(&self, _renamed_to: &str) {
                panic!("D1 benennt nichts um")
            }
        }

        let prompt = RetryThenWork {
            store: &credentials,
            asked: Mutex::new(Vec::new()),
        };
        let opened = open_or_prepare_database(&db_path, &credentials, available(), &prompt)
            .await
            .expect("„Erneut versuchen“ mit funktionierendem Store muss starten");
        assert_eq!(
            prompt.asked.lock().unwrap().len(),
            1,
            "genau ein D1 erwartet"
        );
        assert_eq!(opened.root_key, TEST_ROOT_KEY);
        assert_eq!(credentials.sets(), 0, "kein neuer K bei einem Retry");
        opened.store.close().await;
    }
}

// === A5: der Umbenennungsplan ===========================================

/// A5: Der Plan benennt Datei, `-wal` und `-shm` um, löscht nichts und
/// überschreibt keinen bereits vorhandenen Zielnamen.
#[test]
fn test_a5_the_rename_plan_covers_all_three_files_and_overwrites_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("smart-ssh.db");
    std::fs::write(&db_path, b"haupt").unwrap();
    std::fs::write(dir.path().join("smart-ssh.db-wal"), b"wal").unwrap();
    std::fs::write(dir.path().join("smart-ssh.db-shm"), b"shm").unwrap();

    let plan = plan_start_over_renames(&db_path);
    let main_name = plan.main_target_name();
    plan.execute().unwrap();

    assert!(!db_path.exists(), "das Original muss weg sein");
    assert_eq!(
        std::fs::read(dir.path().join(&main_name)).unwrap(),
        b"haupt"
    );
    assert_eq!(
        std::fs::read(
            dir.path()
                .join(format!("smart-ssh.db-wal{}", suffix_of(&main_name)))
        )
        .unwrap(),
        b"wal"
    );
    assert_eq!(
        std::fs::read(
            dir.path()
                .join(format!("smart-ssh.db-shm{}", suffix_of(&main_name)))
        )
        .unwrap(),
        b"shm"
    );

    // Zweiter Durchlauf in derselben Sekunde: der Zielname darf die erste
    // Sicherung nicht überschreiben.
    std::fs::write(&db_path, b"zweiter").unwrap();
    let second = plan_start_over_renames(&db_path);
    let second_name = second.main_target_name();
    assert_ne!(second_name, main_name);
    second.execute().unwrap();
    assert_eq!(
        std::fs::read(dir.path().join(&main_name)).unwrap(),
        b"haupt"
    );
    assert_eq!(
        std::fs::read(dir.path().join(&second_name)).unwrap(),
        b"zweiter"
    );
}

fn suffix_of(main_name: &str) -> String {
    main_name
        .strip_prefix("smart-ssh.db")
        .expect("Name beginnt mit dem Dateinamen")
        .to_string()
}

// === Hilfen ==============================================================

fn file_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// Eine unverschlüsselte Datenbank mit einer Zeile — der Zustand einer
/// bestehenden Installation vor dieser Spec.
async fn plaintext_database(dir: &std::path::Path) -> std::path::PathBuf {
    let db_path = dir.join("smart-ssh.db");
    let store = SqliteProfileStore::connect_plaintext(&db_path)
        .await
        .unwrap();
    insert_marker_server(&store).await;
    store.close().await;
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Plaintext
    );
    db_path
}

/// Eine mit [`TEST_ROOT_KEY`] verschlüsselte Datenbank.
async fn encrypted_database(dir: &std::path::Path) -> std::path::PathBuf {
    let db_path = dir.join("smart-ssh.db");
    let key = DatabaseKey::from_root_key(&TEST_ROOT_KEY);
    let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .unwrap();
    store.close().await;
    db_path
}

async fn insert_marker_server(store: &SqliteProfileStore) {
    use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, Server};
    use ssh_manager_core::shared::ServerId;
    let now = chrono::Utc::now();
    store
        .create_server(&Server {
            id: ServerId::new(),
            name: "T3".to_string(),
            host: "host-0101.example".to_string(),
            port: 22,
            username: "user-0101".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
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
