//! Spec 0101, T10 (A9): Vertragstests des Datenbank-`CredentialStore`.
//!
//! Die zweite Hälfte von T10 (Verbindungstest mit Jump-Host →
//! `SECRET_STORE_FAILED`) steht in `app-logic`, weil erst dort die
//! Übersetzung in einen `CommandError` stattfindet.

use secrecy::ExposeSecret;
use secrecy::SecretString;

use ssh_manager_core::crypto::DatabaseKey;
use ssh_manager_core::profiles::{CredentialError, CredentialRef, CredentialStore};

use crate::{SqliteCredentialStore, SqliteProfileStore};

/// Fester Wurzelschlüssel für die Tests — kein Geheimnis.
const TEST_ROOT_KEY: [u8; 32] = [7; 32];

/// Der Marker aus Spec 0101 §7 für Secrets.
const MARKER: &str = "Secret-0101";

async fn store_in(dir: &std::path::Path) -> (SqliteProfileStore, SqliteCredentialStore) {
    let db_path = dir.join("smart-ssh.db");
    let profile_store = SqliteProfileStore::connect_encrypted(
        &db_path,
        &DatabaseKey::from_root_key(&TEST_ROOT_KEY),
    )
    .await
    .unwrap();
    let credentials = profile_store.credential_store(tokio::runtime::Handle::current());
    (profile_store, credentials)
}

fn r(name: &str) -> CredentialRef {
    CredentialRef::new(name.to_string())
}

/// T10: `get` nach `set`, `NotFound`, idempotentes `delete`, Überschreiben
/// — die vier Zusicherungen, die A9 mit „Semantik wie bisher" meint.
///
/// **Multi-Thread-Runtime**: Der Store blockiert den Arbeitsthread mit
/// `block_in_place`; auf einer `current_thread`-Runtime ginge das nicht
/// (s. `crate::credential_store`-Modulkommentar und den Test in
/// `app-shell`).
#[tokio::test(flavor = "multi_thread")]
async fn test_t10_contract_get_set_delete_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let (profile_store, credentials) = store_in(dir.path()).await;

    let password = r("server:1:password");

    // `NotFound` für einen Slot, der nie geschrieben wurde — und zwar
    // **die** Variante, nicht irgendein Backend-Fehler (Spec 0071 A14/I4).
    match credentials.get(&password) {
        Err(CredentialError::NotFound(got)) => assert_eq!(got.as_str(), password.as_str()),
        other => panic!("erwartet war NotFound, kam: {other:?}"),
    }

    // `get` nach `set`.
    credentials
        .set(&password, SecretString::from(MARKER.to_string()))
        .unwrap();
    assert_eq!(
        credentials.get(&password).unwrap().expose_secret(),
        MARKER,
        "der Wert muss unverändert zurückkommen"
    );

    // Überschreiben ersetzt den Wert (wie `set_password` im Schlüsselbund).
    credentials
        .set(&password, SecretString::from(format!("{MARKER}-neu")))
        .unwrap();
    assert_eq!(
        credentials.get(&password).unwrap().expose_secret(),
        format!("{MARKER}-neu")
    );

    // `delete` ist idempotent: zweimal löschen ist kein Fehler, und ein
    // nie geschriebener Slot ebensowenig.
    credentials.delete(&password).unwrap();
    credentials.delete(&password).unwrap();
    credentials.delete(&r("server:999:passphrase")).unwrap();
    assert!(matches!(
        credentials.get(&password),
        Err(CredentialError::NotFound(_))
    ));

    profile_store.close().await;
}

/// T10 / A9: „`Backend` ohne Secret im Text".
///
/// Die Störung wird echt erzeugt — der Pool wird geschlossen, jede weitere
/// Abfrage scheitert. Geprüft wird, dass weder der Wert noch die Referenz
/// in der Fehlermeldung steht, und dass es **`Backend`** ist und nicht
/// `NotFound` (ein geschlossener Pool darf nicht wie „kein Eintrag"
/// aussehen — sonst hielte ein Aufrufer ein Secret für gelöscht).
#[tokio::test(flavor = "multi_thread")]
async fn test_t10_a_store_failure_is_backend_and_carries_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let (profile_store, credentials) = store_in(dir.path()).await;

    let reference = r("ai-provider:Secret-0101-ref");
    credentials
        .set(&reference, SecretString::from(MARKER.to_string()))
        .unwrap();

    profile_store.close().await;

    for (what, err) in [
        ("get", credentials.get(&reference).err()),
        (
            "set",
            credentials
                .set(&reference, SecretString::from(MARKER.to_string()))
                .err(),
        ),
        ("delete", credentials.delete(&reference).err()),
    ] {
        let err = err.unwrap_or_else(|| panic!("{what} muss am geschlossenen Pool scheitern"));
        let payload = match &err {
            CredentialError::Backend(payload) => payload.clone(),
            CredentialError::NotFound(_) => {
                panic!("{what}: eine Störung des Speichers darf nicht wie „kein Eintrag\" aussehen")
            }
        };
        assert!(
            !payload.contains(MARKER),
            "{what}: der Secret-Wert darf nicht in der Meldung stehen: {payload}"
        );
        assert!(
            !payload.contains("Secret-0101-ref"),
            "{what}: auch die Referenz gehört nicht hinein: {payload}"
        );
    }
}

/// Teil 0, Frage 2: Derselbe Store, von **außerhalb** jeder Async-Runtime
/// benutzt — so, wie der Startablauf (`build_app_state`) ihn benutzt.
///
/// Ohne die Fallunterscheidung in `SqliteCredentialStore::block_on` würde
/// das panicken (`block_in_place` außerhalb einer Runtime bzw.
/// `Handle::current()` ohne Runtime).
#[test]
fn test_t0_question_2_the_store_works_outside_any_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("smart-ssh.db");

    let profile_store = runtime.block_on(async {
        SqliteProfileStore::connect_encrypted(&db_path, &DatabaseKey::from_root_key(&TEST_ROOT_KEY))
            .await
            .unwrap()
    });
    let credentials = profile_store.credential_store(runtime.handle().clone());

    // Dieser Teil läuft auf dem Testthread, also ohne Runtime-Kontext.
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "der Test prüft nichts, wenn hier doch eine Runtime aktiv ist"
    );
    let reference = r("server:2:sudo_password");
    credentials
        .set(&reference, SecretString::from(MARKER.to_string()))
        .unwrap();
    assert_eq!(credentials.get(&reference).unwrap().expose_secret(), MARKER);

    runtime.block_on(async { profile_store.close().await });
}

/// Teil 0, Frage 2: und aus einem `spawn_blocking`-Thread heraus — der
/// dritte gemessene Aufrufort. Dort meldet `Handle::try_current()` eine
/// laufende Runtime, der Thread gehört aber zum Blocking-Pool.
#[tokio::test(flavor = "multi_thread")]
async fn test_t0_question_2_the_store_works_from_a_blocking_thread() {
    let dir = tempfile::tempdir().unwrap();
    let (profile_store, credentials) = store_in(dir.path()).await;
    let credentials = std::sync::Arc::new(credentials);

    let in_blocking = credentials.clone();
    let value = tokio::task::spawn_blocking(move || {
        let reference = r("server:3:certificate");
        in_blocking
            .set(&reference, SecretString::from(MARKER.to_string()))
            .unwrap();
        in_blocking
            .get(&reference)
            .unwrap()
            .expose_secret()
            .to_string()
    })
    .await
    .unwrap();

    assert_eq!(value, MARKER);
    profile_store.close().await;
}

/// T1 (Teil, A1/A2): Ein über diesen Store geschriebenes Secret steht
/// **nirgends** im Klartext in der Datenbankdatei — sie ist verschlüsselt,
/// und der Store legt keine zweite, unverschlüsselte Spur an.
///
/// Rohdatei-Prüfung an SQLite vorbei, nach demselben Muster wie
/// `crate::tests_raw_file` (Spec 0096).
#[tokio::test(flavor = "multi_thread")]
async fn test_t1_a_stored_secret_never_appears_in_the_raw_file() {
    let dir = tempfile::tempdir().unwrap();
    let (profile_store, credentials) = store_in(dir.path()).await;

    credentials
        .set(
            &r("server:4:password"),
            SecretString::from(MARKER.to_string()),
        )
        .unwrap();
    profile_store.close().await;

    let marker = MARKER.as_bytes();
    let mut checked = 0_usize;
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        checked += 1;
        assert!(
            !bytes.windows(marker.len()).any(|w| w == marker),
            "{} enthält den Secret-Marker im Klartext",
            path.display()
        );
    }
    assert!(
        checked > 0,
        "es wurde keine einzige Datei geprüft — der Test läuft ins Leere"
    );
}
