//! Spec 0101, Commit 4: A4, A6–A8 und die Tests T1 (Teil), T4–T6, T19, T20.
//!
//! **Eigene Datei, wie `tests_raw_file`:** Diese Tests arbeiten auf echten
//! Dateien in Temp-Verzeichnissen, lesen sie teils roh an SQLite vorbei und
//! wandeln die eingecheckte T0-Fixture um — sie teilen mit der
//! `SqliteProfileStore`-Testsuite in `tests` weder Aufbaumuster noch Helfer.
//!
//! **Alle Tests laufen auf einem Multi-Thread-Runtime** (`flavor =
//! "multi_thread"`): Die Umwandlung öffnet und schließt mehrere
//! Verbindungen hintereinander, und `sqlx`s SQLite-Treiber betreibt jede
//! Verbindung auf einem eigenen Hintergrund-Thread. Auf dem
//! Standard-`current_thread`-Runtime von `#[tokio::test]` läuft das zwar
//! auch, aber die Produktivumgebung (Tauris Async-Runtime) ist
//! Multi-Thread — der Test soll dieselbe Umgebung benutzen wie der
//! Startpfad.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::Connection;

use ssh_manager_core::ai::{MessageContent, ProviderId, ProviderType};
use ssh_manager_core::crypto::{ChaCha20Poly1305Cipher, ContentCipher, DatabaseKey};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::encryption::{
    convert_plaintext_database_inner, detect_database_file_state, intermediate_path,
    ConversionFailure, ConversionStep, DatabaseFileState, SQLITE_PLAINTEXT_HEADER,
};
use crate::tests_fixture_t0::{align_migration_checksums_to_current_build, fixture_path};
use crate::{AiProviderConfig, PersistenceError, SqliteProfileStore};

/// Die Marker aus Spec 0101 §7. `Secret-0101`/`Token-0101` kommen erst mit
/// den Commits 6–8 dazu (es gibt noch keine Tabelle dafür) — T1 ist
/// deshalb hier nur zum Teil erfüllt, wie die Umsetzungsreihenfolge es
/// vorsieht („T1 (Teil)").
const HOST_MARKER: &str = "host-0101.example";
const USER_MARKER: &str = "user-0101";
const HEADER_MARKER: &str = "Header-0101";

/// Derselbe feste Wurzelschlüssel wie in der T0-Fixture — kein Geheimnis.
const TEST_ROOT_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

fn test_key() -> DatabaseKey {
    DatabaseKey::from_root_key(&TEST_ROOT_KEY)
}

fn other_key() -> DatabaseKey {
    let mut root = TEST_ROOT_KEY;
    root[0] ^= 0xff;
    DatabaseKey::from_root_key(&root)
}

fn test_cipher() -> Arc<dyn ContentCipher> {
    Arc::new(ChaCha20Poly1305Cipher::new(&TEST_ROOT_KEY))
}

/// Sucht `needle` roh in allen Dateien des Verzeichnisses `dir` (nicht nur
/// in den Datenbankdateien) — damit auch eine versehentlich daneben
/// geschriebene Datei auffällt.
fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return hits;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if contains(&bytes, needle.as_bytes()) {
                hits.push(path);
            }
        }
    }
    hits
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Legt die Beispielzeilen mit den Markern an — identisch für die
/// verschlüsselte und die Klartext-Variante, damit T1s Gegenprobe
/// tatsächlich denselben Inhalt vergleicht.
async fn populate_markers(store: &SqliteProfileStore) {
    let now = chrono::Utc::now();
    store
        .create_server(&Server {
            id: ServerId::new(),
            name: "Marker Server".to_string(),
            host: HOST_MARKER.to_string(),
            port: 22,
            username: USER_MARKER.to_string(),
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
        .expect("Server anlegen");
    store
        .ai_provider_store()
        .create(&AiProviderConfig {
            id: ProviderId::new(),
            provider_type: ProviderType::Anthropic,
            display_name: "Marker Provider".to_string(),
            base_url: None,
            model: "claude-sonnet-5".to_string(),
            supports_native_tool_calling: true,
            credential_ref: CredentialRef::new("ai-provider:marker".to_string()),
            is_active: false,
            extra_headers: vec![("X-Test".to_string(), HEADER_MARKER.to_string())],
            attestation_url: None,
            max_tokens_override: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("Provider anlegen");
}

// --- A4/A2: Öffnen mit Schlüssel ------------------------------------------

/// T1 (Teil, Spec 0101 §7): Eine neue Installation mit
/// `connect_encrypted` — Server, Provider mit Header — enthält keinen der
/// Marker im Klartext, in **keiner** Datei des Datenverzeichnisses.
///
/// **Mit eingebauter Gegenprobe.** Derselbe Inhalt wird zusätzlich über
/// `connect_plaintext` in ein zweites Verzeichnis geschrieben; dort müssen
/// die Marker gefunden werden. Ohne diesen zweiten Teil könnte der Test
/// grün sein, weil die Suche nicht funktioniert — und nicht, weil
/// verschlüsselt wurde. Das ist der von §7 verlangte Beleg „Scheitert heute
/// (Hostname im Klartext)", als dauerhafter Bestandteil des Tests statt als
/// einmalige Handprobe.
#[tokio::test(flavor = "multi_thread")]
async fn test_t1_a_fresh_encrypted_database_contains_no_marker_in_plaintext() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    let key = test_key();

    let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .expect("frische verschlüsselte Datenbank anlegbar");
    populate_markers(&store).await;
    store.pool.close().await;

    let header = std::fs::read(&db_path).expect("Datenbankdatei lesbar");
    assert!(
        !header.starts_with(SQLITE_PLAINTEXT_HEADER),
        "die verschlüsselte Datei darf nicht den Klartext-SQLite-Header tragen"
    );

    for marker in [HOST_MARKER, USER_MARKER, HEADER_MARKER] {
        let hits = files_containing(dir.path(), marker);
        assert!(
            hits.is_empty(),
            "Marker {marker} steht im Klartext in: {hits:?}"
        );
    }

    // --- Gegenprobe am Klartext-Weg: dieselbe Suche MUSS dort fündig werden.
    let plain_dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let plain_path = plain_dir.path().join("smart-ssh.db");
    let plain = SqliteProfileStore::connect_plaintext(&plain_path)
        .await
        .expect("Klartext-Datenbank anlegbar");
    populate_markers(&plain).await;
    plain.pool.close().await;
    for marker in [HOST_MARKER, USER_MARKER, HEADER_MARKER] {
        assert!(
            !files_containing(plain_dir.path(), marker).is_empty(),
            "die Gegenprobe findet {marker} im Klartext-Fall nicht — dann prüft der \
             Test oben nichts"
        );
    }
}

/// A4: Eine Klartext-Datei, mit einem Schlüssel geöffnet, ergibt
/// [`PersistenceError::NotReadableWithKey`] — **nicht** `code 7 „out of
/// memory"` aus dem Migrationslauf und nicht den Bibliothekstext „file is
/// not a database".
#[tokio::test(flavor = "multi_thread")]
async fn test_a4_opening_a_plaintext_file_with_a_key_reports_the_key_case() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    SqliteProfileStore::connect_plaintext(&db_path)
        .await
        .expect("Klartext-Datenbank anlegbar")
        .pool
        .close()
        .await;

    match SqliteProfileStore::connect_encrypted(&db_path, &test_key()).await {
        Err(PersistenceError::NotReadableWithKey) => {}
        Err(other) => panic!("erwartet: NotReadableWithKey, erhalten: {other}"),
        Ok(_) => panic!("eine Klartext-Datei darf sich nicht mit einem Schlüssel öffnen lassen"),
    }

    assert_eq!(
        SqliteProfileStore::connect_encrypted(&db_path, &test_key())
            .await
            .err()
            .expect("Fehler erwartet")
            .classify(),
        crate::ConnectFailureKind::KeyMismatch,
        "der Startdialog braucht D2, nicht den Backup-Rat aus `Other`"
    );
}

/// A4/D2: Eine verschlüsselte Datei mit dem **falschen** Schlüssel ist
/// derselbe Fall — und der Fehlertext enthält kein Schlüsselmaterial.
#[tokio::test(flavor = "multi_thread")]
async fn test_a4_wrong_key_reports_the_key_case_without_key_material() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    let key = test_key();
    SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .expect("anlegbar")
        .pool
        .close()
        .await;

    let err = SqliteProfileStore::connect_encrypted(&db_path, &other_key())
        .await
        .err()
        .expect("falscher Schlüssel muss scheitern");
    assert!(matches!(err, PersistenceError::NotReadableWithKey));

    let rendered = format!("{err} {err:?}");
    for candidate in [&key, &other_key()] {
        let pragma = candidate.pragma_value();
        let hex = secrecy::ExposeSecret::expose_secret(&pragma)
            .trim_start_matches("x'")
            .trim_end_matches('\'')
            .to_string();
        assert!(
            !rendered.contains(&hex),
            "der Fehlertext enthält Schlüsselmaterial: {rendered}"
        );
    }
}

/// A2, mit Gegenbeweis am echten Fehler: Beim Bauen dieses Commits stand
/// der vollständige Datenbankschlüssel in einer `sqlx`-Fehlermeldung
/// (`near "x'7c83…'": syntax error`, weil der Pragma-Wert ohne
/// Anführungszeichen keine gültige Pragma-Syntax ist). Dieser Test
/// **erzeugt genau diesen Fehler wieder** — mit derselben, bewusst
/// un-zitierten Pragma-Fassung — und belegt zweierlei:
///
/// 1. Der Fehlertext enthält tatsächlich Schlüsselmaterial
///    (`contains_key_material` erkennt es; ohne diese Zusicherung könnte
///    die Redaktion ins Leere greifen).
/// 2. `redact_if_key_bearing` nimmt ihm daraufhin seine Variante, sodass
///    über `Display` nichts mehr durchkommt.
///
/// Die produktive Fassung (`encrypted_connect_options`) erzeugt diesen
/// Fehler nicht mehr — aber die Redaktion muss greifen, falls ein künftiger
/// `sqlx`- oder SQLCipher-Stand wieder eine Anweisung zitiert.
#[tokio::test(flavor = "multi_thread")]
async fn test_a2_a_connect_error_quoting_the_key_is_redacted() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    let key = test_key();

    // Die alte, fehlerhafte Fassung: Pragma-Wert ohne Anführungszeichen.
    let pragma = key.pragma_value();
    let leaking_options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .pragma(
            "key",
            secrecy::ExposeSecret::expose_secret(&pragma).to_string(),
        );
    let err = SqliteProfileStore::connect_with_probe(leaking_options, true)
        .await
        .err()
        .expect("die un-zitierte Pragma-Fassung muss scheitern");

    assert!(
        crate::encryption::contains_key_material(&err.to_string(), &key),
        "Voraussetzung dieses Tests: der rohe Fehlertext enthält den Schlüssel. \
         War: {err}"
    );

    let redacted = crate::encryption::redact_if_key_bearing(err, &key);
    assert!(matches!(redacted, PersistenceError::RedactedConnect));
    assert!(
        !crate::encryption::contains_key_material(&redacted.to_string(), &key),
        "nach der Redaktion darf kein Schlüsselmaterial mehr im Text stehen"
    );
    assert!(
        !crate::encryption::contains_key_material(&format!("{redacted:?}"), &key),
        "auch `Debug` darf nach der Redaktion kein Schlüsselmaterial zeigen"
    );
}

/// A2: Ein Fehler **ohne** Schlüsselmaterial behält seine Variante — sonst
/// verlöre der Startdialog die Unterscheidung aus Spec 0059 (Fall 4,
/// `PermissionDenied`) und riete beim falschen Problem zum Backup.
#[test]
fn test_a2_redaction_keeps_a_key_free_error_intact() {
    let key = test_key();
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
    let err = PersistenceError::Connect(sqlx::Error::Io(io_err));

    let kept = crate::encryption::redact_if_key_bearing(err, &key);
    assert_eq!(kept.classify(), crate::ConnectFailureKind::PermissionDenied);
}

/// A8: `cipher_log_level` kommt tatsächlich auf der Verbindung an — und der
/// Schlüssel-Pragma davor, sonst wäre die Datei gar nicht lesbar.
/// Gemessen statt angenommen: `sqlx` führt die Pragmas in einer Reihenfolge
/// aus, die diese Crate nicht festlegt.
#[tokio::test(flavor = "multi_thread")]
async fn test_a8_cipher_log_level_is_none() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    let key = test_key();
    let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .expect("anlegbar");

    let level: String = sqlx::query_scalar("PRAGMA cipher_log_level")
        .fetch_one(&store.pool)
        .await
        .expect("cipher_log_level abfragbar");
    assert!(
        level.eq_ignore_ascii_case("NONE"),
        "cipher_log_level muss NONE sein, war: {level}"
    );
    store.pool.close().await;
}

// --- A3-Dateizustand (die Erkennung; die Tabelle selbst in Commit 5) -----

#[test]
fn test_a3_file_state_missing_plaintext_and_other() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let missing = dir.path().join("gibt-es-nicht.db");
    assert_eq!(
        detect_database_file_state(&missing).unwrap(),
        DatabaseFileState::Missing
    );

    let empty = dir.path().join("leer.db");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(
        detect_database_file_state(&empty).unwrap(),
        DatabaseFileState::Missing,
        "0 Byte ohne nichtleere -wal ist *fehlt* (A3, wörtlich)"
    );

    // 0 Byte **mit** nichtleerer -wal ist ausdrücklich nicht *fehlt*.
    std::fs::write(dir.path().join("leer.db-wal"), b"irgendwas").unwrap();
    assert_eq!(
        detect_database_file_state(&empty).unwrap(),
        DatabaseFileState::Other
    );

    let plaintext = dir.path().join("klartext.db");
    std::fs::write(&plaintext, SQLITE_PLAINTEXT_HEADER).unwrap();
    assert_eq!(
        detect_database_file_state(&plaintext).unwrap(),
        DatabaseFileState::Plaintext
    );

    let encrypted = dir.path().join("sonst.db");
    std::fs::write(&encrypted, b"\x17\x42 kein SQLite-Header \x00\x01\x02").unwrap();
    assert_eq!(
        detect_database_file_state(&encrypted).unwrap(),
        DatabaseFileState::Other
    );

    let too_short = dir.path().join("kurz.db");
    std::fs::write(&too_short, b"SQLite").unwrap();
    assert_eq!(
        detect_database_file_state(&too_short).unwrap(),
        DatabaseFileState::Other,
        "kürzer als 16 Byte ist vorhanden, aber kein Klartext-Header"
    );
}

/// A3: Die Erkennung darf die Datei nicht verändern — T8 baut darauf auf
/// („Datenbank nicht geöffnet, Inhalt und mtime gleich").
#[test]
fn test_a3_detecting_the_file_state_changes_neither_content_nor_mtime() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    std::fs::write(&db_path, b"\x17\x42 verschluesselt sieht so aus \x00").unwrap();
    let before = std::fs::read(&db_path).unwrap();
    let mtime_before = std::fs::metadata(&db_path).unwrap().modified().unwrap();

    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Other
    );

    assert_eq!(std::fs::read(&db_path).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&db_path).unwrap().modified().unwrap(),
        mtime_before
    );
}

/// A3: Ein Lesefehler, der nicht „nicht vorhanden" ist, wird nicht zu
/// *fehlt* verharmlost — sonst liefe die Tabelle auf „neu anlegen" und
/// wollte eine vorhandene Datenbank überschreiben.
#[cfg(unix)]
#[test]
fn test_a3_an_unreadable_file_is_not_reported_as_missing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("smart-ssh.db");
    std::fs::write(&db_path, b"\x17\x42 etwas, das nicht Klartext ist \x00").unwrap();
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = detect_database_file_state(&db_path);
    // Als `root` ist die Datei trotz 0o000 lesbar — dann ist der Zustand
    // *sonst*, nie *fehlt*. Beides ist richtig, `Missing` wäre es nicht.
    match result {
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied),
        Ok(state) => assert_eq!(state, DatabaseFileState::Other),
    }

    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

// --- A6/A7: Umwandlung ----------------------------------------------------

/// Kopiert die eingecheckte T0-Fixture in ein Temp-Verzeichnis und richtet
/// ihre Migrations-Prüfsummen auf den laufenden Build aus (Spec 0101 §9
/// Klarstellung 2 — gilt für T0 **und** T4–T6).
async fn fixture_copy(dir: &Path) -> PathBuf {
    let fixture = fixture_path();
    assert!(
        fixture.exists(),
        "T0-Fixture fehlt unter {fixture:?} — s. `tests_fixture_t0`"
    );
    let copy = dir.join("smart-ssh.db");
    std::fs::copy(&fixture, &copy).expect("Fixture kopierbar");
    align_migration_checksums_to_current_build(&copy).await;
    copy
}

/// Zeilenzahlen je Tabelle und `user_version` einer **unverschlüsselten**
/// Datei, von außen gemessen — damit T4 nicht auf die Prüfung vertraut, die
/// in der Umwandlung selbst steckt.
async fn plaintext_snapshot(path: &Path) -> (i64, Vec<(String, i64)>) {
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("Klartext-Datei öffenbar");
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    let mut rows = Vec::new();
    for name in names {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{name}\""
        )))
        .fetch_one(&mut conn)
        .await
        .unwrap();
        rows.push((name, count));
    }
    conn.close().await.unwrap();
    (user_version, rows)
}

/// T4 (A6, A7): Die eingecheckte Fixture — geschrieben vom Build **vor**
/// SQLCipher (SQLite 3.51.3) — wird umgewandelt. Danach: gleiche
/// Zeilenzahlen je Tabelle, gleiche `user_version`, `journal_mode = wal`,
/// alle 14 Migrationen, kein Marker im Klartext, kein Klartext-Original,
/// und der feldweise verschlüsselte Chatinhalt bleibt mit dem Test-K
/// lesbar.
#[tokio::test(flavor = "multi_thread")]
async fn test_t4_converting_the_pre_sqlcipher_fixture_keeps_everything() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = fixture_copy(dir.path()).await;
    let key = test_key();

    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Plaintext
    );
    let (expected_user_version, expected_rows) = plaintext_snapshot(&db_path).await;
    assert!(
        expected_rows.iter().any(|(name, _)| name == "servers"),
        "die Fixture muss die Tabelle `servers` tragen: {expected_rows:?}"
    );

    crate::convert_plaintext_database(&db_path, &key)
        .await
        .expect("Umwandlung der Fixture muss gelingen (A7)");

    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Other,
        "nach der Umwandlung darf die Datei keinen Klartext-Header mehr tragen"
    );
    assert!(
        !intermediate_path(&db_path).exists(),
        "die Zwischendatei muss nach dem Umbenennen verschwunden sein"
    );

    let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .expect("umgewandelte Datei mit dem Schlüssel öffenbar");

    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(user_version, expected_user_version);

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert!(
        journal_mode.eq_ignore_ascii_case("wal"),
        "journal_mode war {journal_mode}"
    );

    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(applied, 14, "alle 14 Migrationen müssen erhalten sein");

    for (name, expected_count) in &expected_rows {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{name}\""
        )))
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            count, *expected_count,
            "Zeilenzahl von {name} weicht nach der Umwandlung ab"
        );
    }

    let servers = store.list_servers().await.expect("Server lesbar");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].host, HOST_MARKER);
    assert_eq!(servers[0].username, USER_MARKER);

    // Der feldweise verschlüsselte Chatinhalt bleibt unter dem Test-K
    // lesbar — die Umwandlung darf die Blobs nicht anfassen (E11).
    let chat = store.chat_session_store(test_cipher());
    let sessions = chat
        .list_sessions_for_server(&servers[0].id)
        .await
        .expect("Sitzungen lesbar");
    assert_eq!(sessions.len(), 1);
    let messages = chat
        .load_session(sessions[0].id)
        .await
        .expect("Chatinhalt mit dem Test-K entschlüsselbar");
    assert!(
        messages.iter().any(|m| matches!(
            &m.content,
            MessageContent::Text(text) if text.contains("Chat-0101")
        )),
        "Chat-Marker nach der Umwandlung nicht mehr entschlüsselbar: {messages:?}"
    );

    store.pool.close().await;

    for marker in [HOST_MARKER, USER_MARKER, HEADER_MARKER] {
        let hits = files_containing(dir.path(), marker);
        assert!(
            hits.is_empty(),
            "Marker {marker} steht nach der Umwandlung noch im Klartext in: {hits:?}"
        );
    }
}

/// T5 (A6, Abbruch): Fehlerinjektion nach Schritt 2 und nach Schritt 3.
/// Danach muss das Original inhaltsgleich sein, es darf keine
/// Zwischendatei liegen, und der nächste Start muss die Umwandlung
/// nachholen können.
#[tokio::test(flavor = "multi_thread")]
async fn test_t5_an_aborted_conversion_leaves_the_original_usable() {
    for abort_after in [
        ConversionStep::AfterEncryptedCopy,
        ConversionStep::AfterVerification,
    ] {
        let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
        let db_path = fixture_copy(dir.path()).await;
        let key = test_key();
        let (expected_user_version, expected_rows) = plaintext_snapshot(&db_path).await;

        let err = convert_plaintext_database_inner(&db_path, &key, &mut |step| {
            if step == abort_after {
                Err(ConversionFailure::Verification(
                    "Fehlerinjektion (T5)".to_string(),
                ))
            } else {
                Ok(())
            }
        })
        .await
        .err()
        .unwrap_or_else(|| panic!("{abort_after:?}: die Umwandlung musste scheitern"));
        assert!(matches!(err, ConversionFailure::Verification(_)));

        assert!(
            !intermediate_path(&db_path).exists(),
            "{abort_after:?}: die Zwischendatei muss entfernt sein"
        );
        assert_eq!(
            detect_database_file_state(&db_path).unwrap(),
            DatabaseFileState::Plaintext,
            "{abort_after:?}: das Original muss unverändert Klartext sein"
        );
        assert_eq!(
            plaintext_snapshot(&db_path).await,
            (expected_user_version, expected_rows.clone()),
            "{abort_after:?}: das Original muss inhaltsgleich sein"
        );

        // Der nächste Start wandelt um — ohne Spur des Abbruchs.
        crate::convert_plaintext_database(&db_path, &key)
            .await
            .unwrap_or_else(|e| panic!("{abort_after:?}: zweiter Versuch muss gelingen: {e}"));
        let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
            .await
            .expect("nach dem zweiten Versuch öffenbar");
        for (name, expected_count) in &expected_rows {
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*) FROM \"{name}\""
            )))
            .fetch_one(&store.pool)
            .await
            .unwrap();
            assert_eq!(count, *expected_count, "{abort_after:?}: {name}");
        }
        store.pool.close().await;
    }
}

/// T6 (A6, WAL): Eine Zeile, die nur im WAL des Originals steht, muss nach
/// der Umwandlung vorhanden sein — und neben der neuen Datei darf keine
/// alte `-wal` liegen bleiben (sie gehörte zu einer anderen Datenbank).
#[tokio::test(flavor = "multi_thread")]
async fn test_t6_a_row_only_in_the_original_wal_survives_the_conversion() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = fixture_copy(dir.path()).await;
    let key = test_key();

    // Eine zweite Zeile schreiben und die Verbindung hart fallen lassen,
    // ohne `pool.close()` — dann bleibt der Inhalt im WAL stehen (dieselbe
    // gemessene Mechanik wie in `tests_raw_file`).
    let wal_only_host = "host-0101-wal.example";
    {
        let store = SqliteProfileStore::connect_plaintext(&db_path)
            .await
            .expect("Fixture-Kopie öffenbar");
        let now = chrono::Utc::now();
        store
            .create_server(&Server {
                id: ServerId::new(),
                name: "Nur im WAL".to_string(),
                host: wal_only_host.to_string(),
                port: 22,
                username: USER_MARKER.to_string(),
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
            .expect("zweiten Server anlegen");
        std::mem::forget(store);
    }
    let wal = {
        let mut name = db_path.as_os_str().to_os_string();
        name.push("-wal");
        PathBuf::from(name)
    };
    assert!(
        std::fs::metadata(&wal)
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "Voraussetzung von T6: es muss ein nichtleeres -wal neben dem Original liegen"
    );

    crate::convert_plaintext_database(&db_path, &key)
        .await
        .expect("Umwandlung mit nichtleerem WAL muss gelingen");

    assert!(
        !wal.exists(),
        "die alte -wal darf nicht neben der neuen Datei liegen bleiben"
    );
    for suffix in ["-shm", "-journal"] {
        let mut name = db_path.as_os_str().to_os_string();
        name.push(suffix);
        assert!(
            !PathBuf::from(&name).exists(),
            "alte {suffix}-Datei liegt noch neben der neuen Datei"
        );
    }

    let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .expect("umgewandelte Datei öffenbar");
    let servers = store.list_servers().await.expect("Server lesbar");
    assert!(
        servers.iter().any(|s| s.host == wal_only_host),
        "die nur im WAL stehende Zeile fehlt nach der Umwandlung: {:?}",
        servers.iter().map(|s| &s.host).collect::<Vec<_>>()
    );
    store.pool.close().await;

    assert!(
        files_containing(dir.path(), wal_only_host).is_empty(),
        "der WAL-Marker steht noch im Klartext"
    );
}

/// T19 (Fremde Zwischendatei): Was an der Stelle der Zwischendatei liegt,
/// wird verworfen — nie als fertige Umwandlung übernommen.
#[tokio::test(flavor = "multi_thread")]
async fn test_t19_a_foreign_intermediate_file_is_discarded_not_adopted() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = fixture_copy(dir.path()).await;
    let key = test_key();
    let (_, expected_rows) = plaintext_snapshot(&db_path).await;

    let tmp = intermediate_path(&db_path);
    std::fs::write(&tmp, b"fremder Inhalt, der nie eine Datenbank war").unwrap();
    // Dazu noch eine fremde `-wal` der Zwischendatei, die sonst auf die
    // neue Datei angewandt werden könnte.
    let mut tmp_wal = tmp.as_os_str().to_os_string();
    tmp_wal.push("-wal");
    std::fs::write(PathBuf::from(&tmp_wal), b"fremdes WAL").unwrap();

    crate::convert_plaintext_database(&db_path, &key)
        .await
        .expect("die fremde Zwischendatei darf die Umwandlung nicht aufhalten");

    assert!(!tmp.exists(), "die Zwischendatei muss verschwunden sein");
    let store = SqliteProfileStore::connect_encrypted(&db_path, &key)
        .await
        .expect("das Ergebnis muss die umgewandelte Datenbank sein, nicht die fremde Datei");
    for (name, expected_count) in &expected_rows {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{name}\""
        )))
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(count, *expected_count, "{name}");
    }
    store.pool.close().await;

    assert!(
        files_containing(dir.path(), "fremder Inhalt").is_empty(),
        "Reste der fremden Zwischendatei liegen noch im Verzeichnis"
    );
}

/// T20 (Symlink): Ist `smart-ssh.db` eine symbolische Verknüpfung auf eine
/// Klartext-Datei, wird **nicht** umgewandelt — Ziel und Verknüpfung
/// bleiben unverändert, und es entsteht keine verschlüsselte Datei.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_t20_a_symlinked_database_is_not_converted() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let target = fixture_copy(dir.path()).await;
    let renamed_target = dir.path().join("echte-datenbank.db");
    std::fs::rename(&target, &renamed_target).unwrap();
    let link = dir.path().join("smart-ssh.db");
    std::os::unix::fs::symlink(&renamed_target, &link).unwrap();

    let target_before = std::fs::read(&renamed_target).unwrap();
    let key = test_key();

    let err = crate::convert_plaintext_database(&link, &key)
        .await
        .expect_err("ein Symlink darf nicht umgewandelt werden");
    assert!(
        matches!(err, ConversionFailure::Symlink),
        "erwartet: Symlink, erhalten: {err}"
    );

    assert_eq!(
        std::fs::read(&renamed_target).unwrap(),
        target_before,
        "die Zieldatei muss unverändert bleiben"
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "die Verknüpfung muss eine Verknüpfung bleiben"
    );
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        renamed_target,
        "die Verknüpfung muss auf dasselbe Ziel zeigen"
    );
    assert!(
        !intermediate_path(&link).exists(),
        "es darf keine Zwischendatei entstanden sein"
    );
    assert_eq!(
        detect_database_file_state(&renamed_target).unwrap(),
        DatabaseFileState::Plaintext,
        "das Ziel darf nicht verschlüsselt worden sein"
    );

    // Und der Startfehler sagt es als eigener Fall, nicht als „beschädigt".
    assert_eq!(
        PersistenceError::Conversion(ConversionFailure::Symlink).classify(),
        crate::ConnectFailureKind::SymlinkedDatabase
    );
}

/// A6, Schritt 4: Nach der Umwandlung liegt kein Klartext-Original mehr —
/// und auf Unix trägt die neue Datei 0600.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_a6_converted_file_has_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = fixture_copy(dir.path()).await;
    crate::convert_plaintext_database(&db_path, &test_key())
        .await
        .expect("Umwandlung");

    let mode = std::fs::metadata(&db_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "Rechte waren {mode:o}");
}

/// A6, Schritt 1/4: Der Pfad auf eine Datei, die es nicht gibt, darf nicht
/// still eine leere „umgewandelte" Datenbank hinterlassen — sonst wäre eine
/// verlorene Datei nicht von einer erfolgreich umgewandelten zu
/// unterscheiden.
#[tokio::test(flavor = "multi_thread")]
async fn test_a6_converting_a_missing_file_fails_without_creating_anything() {
    let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
    let db_path = dir.path().join("gibt-es-nicht.db");

    let err = crate::convert_plaintext_database(&db_path, &test_key())
        .await
        .expect_err("ohne Original darf nichts umgewandelt werden");
    assert!(matches!(err, ConversionFailure::Io(_)), "erhalten: {err}");
    assert!(!db_path.exists());
    assert!(!intermediate_path(&db_path).exists());
}
