//! Spec 0101, T0: Fixture „vom heutigen Build“ — eine Datenbankdatei, mit
//! dem **heutigen**, noch nicht an SQLCipher gebundenen SQLite
//! geschrieben, mit allen 14 Migrationen und Beispielzeilen, die die
//! Marker aus Abschnitt 7 der Spec tragen. Grundlage für T4–T6 (Commit 4):
//! die Umwandlung einer bestehenden Installation in eine verschlüsselte
//! Datei muss auch an einer Datei gelingen, die nicht mit dem
//! SQLCipher-gebundenen Build entstanden ist (A7).
//!
//! **Warum eingecheckt, nicht zur Laufzeit erzeugt:** Ab dem nächsten
//! Commit dieser Spec baut dieselbe Crate dauerhaft gegen SQLCipher (A1,
//! T9) — danach gibt es keinen „heutigen, nicht-SQLCipher“-Build mehr,
//! gegen den sich diese Datei neu erzeugen ließe (die mitgebaute
//! SQLite-Version sinkt dabei zusätzlich von 3.51.3 auf 3.50.4, s. Spec
//! Abschnitt 1 „Messungen“ — ein später neu erzeugtes File wäre also auch
//! technisch nicht mehr dasselbe). Diese Fixture wird deshalb genau
//! einmal, **vor** diesem Abhängigkeitswechsel, erzeugt und als Binärdatei
//! unter `tests/fixtures/t0-pre-sqlcipher.sqlite3` eingecheckt.
//!
//! `Secret-0101`/`Token-0101` aus der Marker-Liste in Spec 0101 Abschnitt 7
//! fehlen hier bewusst: die Spalten/Tabellen, die diese Werte tragen,
//! führen erst die Commits 6–8 dieser Spec ein (Secrets-Tabelle,
//! MCP-Token-Spalte) — der heutige Build kennt sie nicht.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::Connection;

use ssh_manager_core::ai::{ChatMessage, MessageContent, ProviderId, ProviderType, Role};
use ssh_manager_core::crypto::{ChaCha20Poly1305Cipher, ContentCipher};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::{AiProviderConfig, PersistenceError, SqliteProfileStore};

const HOST_MARKER: &str = "host-0101.example";
const USER_MARKER: &str = "user-0101";
const HEADER_MARKER: &str = "Header-0101";
/// Eigener Marker für den feldweise verschlüsselten Chatinhalt — nicht Teil
/// der Spec-Marker-Liste (die zählt nur die Klartext-Marker für den
/// Rohdatei-Nachweis), sondern nur dazu da, nach Entschlüsselung mit
/// [`T0_TEST_KEY`] wiedererkannt zu werden.
const CHAT_MARKER: &str = "Chat-0101";

/// Fester Schlüssel, unter dem der Chatinhalt dieser Fixture feldweise
/// verschlüsselt ist (Spec 0101, T0: „fester Test-K“). Kein Geheimnis —
/// steht absichtlich im Klartext hier, damit T4–T6 (Commit 4) denselben
/// Wert zum Entschlüsseln verwenden können.
pub(crate) const T0_TEST_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// Pfad der eingecheckten Fixture — `pub(crate)`, weil Commit 4 (T4–T6)
/// denselben Pfad braucht, um die Umwandlung gegen dieselbe Datei zu
/// testen.
pub(crate) fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/t0-pre-sqlcipher.sqlite3")
}

fn test_cipher() -> Arc<dyn ContentCipher> {
    Arc::new(ChaCha20Poly1305Cipher::new(&T0_TEST_KEY))
}

/// Erzeugt die Fixture-Datei einmalig — bewusst **kein** Teil des
/// normalen Testlaufs (`#[ignore]`). Manuell ausgeführt vor Commit 2
/// dieser Spec:
///
/// ```text
/// cargo test -p persistence-sqlite --lib -- --ignored generate_fixture_once --nocapture
/// ```
///
/// Schreibt NIE über eine bereits vorhandene Fixture — dafür muss die
/// vorhandene Datei erst von Hand entfernt werden. Dieser Schutz
/// verhindert, dass ein künftiges pauschales `--ignored`-Sammelausführen
/// (z. B. ein CI-Job, der irgendwann alle ignorierten Tests laufen lässt)
/// die eingefrorene Datei stillschweigend mit dem Stand eines späteren,
/// SQLCipher-gebundenen Builds überschreibt. **Nach Commit 2 dieser Spec
/// darf diese Funktion nicht mehr ausgeführt werden** — das mitgebaute
/// SQLite wäre dann nicht mehr dasselbe (s. Moduldoku oben).
#[tokio::test]
#[ignore = "erzeugt die eingecheckte T0-Fixture von Hand, einmalig vor Commit 2 dieser Spec"]
async fn generate_fixture_once() {
    let path = fixture_path();
    assert!(
        !path.exists(),
        "Fixture {path:?} existiert bereits — von Hand löschen, wenn eine bewusste \
         Neuerzeugung gewollt ist (nur vor dem SQLCipher-Wechsel sinnvoll, s. Moduldoku)."
    );
    std::fs::create_dir_all(
        path.parent()
            .expect("Fixture-Pfad hat ein Elternverzeichnis"),
    )
    .expect("Fixture-Verzeichnis anlegbar");

    let store = SqliteProfileStore::connect(&path)
        .await
        .expect("frische DB mit 14 Migrationen sollte anlegbar sein");

    let server_id = ServerId::new();
    let now = chrono::Utc::now();
    store
        .create_server(&Server {
            id: server_id,
            name: "T0 Fixture Server".to_string(),
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
            display_name: "T0 Fixture Provider".to_string(),
            base_url: None,
            model: "claude-sonnet-5".to_string(),
            supports_native_tool_calling: true,
            credential_ref: CredentialRef::new("ai-provider:t0-fixture".to_string()),
            is_active: false,
            extra_headers: vec![("X-Test".to_string(), HEADER_MARKER.to_string())],
            attestation_url: None,
            max_tokens_override: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("Provider anlegen");

    let chat_store = store.chat_session_store(test_cipher());
    let session_id = chat_store
        .create_session(&server_id, None)
        .await
        .expect("Chat-Sitzung anlegen");
    chat_store
        .append_message(
            session_id,
            &ChatMessage {
                role: Role::User,
                content: MessageContent::Text(format!("Testnachricht {CHAT_MARKER}")),
            },
        )
        .await
        .expect("Chat-Nachricht schreiben");

    // Schließen überträgt den Inhalt des WAL in die Hauptdatei (s.
    // `tests_raw_file`-Moduldoku, derselbe gemessene Mechanismus) — die
    // eingecheckte Datei soll ein einzelnes, stabiles File sein, kein
    // Hauptdatei-plus-offenes-WAL-Paar.
    store.pool.close().await;

    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }

    println!("T0-Fixture erzeugt: {}", path.display());
}

/// Richtet die Prüfsummen in `_sqlx_migrations` der übergebenen **Kopie**
/// auf die des laufenden Builds aus (Spec 0101 §9 Klarstellung 2): Windows
/// checkt die Migrations-`.sql`-Dateien mit CRLF aus (`.gitattributes`),
/// wodurch die vom `sqlx::migrate!()`-Makro zur Compile-Zeit aus dem
/// Dateiinhalt berechnete Prüfsumme von der in der (unter LF geschriebenen)
/// Fixture gespeicherten abweicht — `sqlx` lehnt das Öffnen sonst mit
/// `Migrate(VersionMismatch(_))` ab, obwohl der SQL-Inhalt bis auf die
/// Zeilenenden identisch ist. Ändert nur die Prüfsumme der (bis zu 14)
/// bereits vorhandenen Einträge — `WHERE version = ?` trifft nie eine
/// Version, die die Kopie noch nicht kennt —, fügt keine neue Zeile hinzu
/// und führt keine Migration aus. Die eingecheckte Fixture-Datei selbst
/// bleibt unangetastet, weil immer nur die vom Aufrufer übergebene Kopie
/// geöffnet wird. `pub(crate)`, nicht nur für diesen Test: T4–T6 (Commit 4
/// dieser Spec) öffnen dieselbe Fixture und brauchen denselben Abgleich.
pub(crate) async fn align_migration_checksums_to_current_build(copy_path: &Path) {
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(copy_path);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("Kopie für Prüfsummen-Abgleich öffenbar");
    for migration in sqlx::migrate!().iter() {
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(migration.checksum.as_ref())
            .bind(migration.version)
            .execute(&mut conn)
            .await
            .expect("Prüfsumme in _sqlx_migrations aktualisierbar");
    }
    conn.close()
        .await
        .expect("Verbindung nach Abgleich schließbar");
}

/// T0 (Spec 0101, §7): Nachweis, dass die eingecheckte Fixture tatsächlich
/// das ist, was T4–T6 (Commit 4) brauchen. Geprüft an einer **Kopie** —
/// das Öffnen mit SQLite legt ggf. `-wal`/`-shm` neben die Datei an, und
/// `sqlx::migrate!()` würde (folgenlos, aber lesend/schreibend) gegen die
/// eingecheckte Datei selbst laufen; eine Kopie hält die Fixture im Repo
/// unverändert.
#[tokio::test]
async fn test_t0_fixture_has_14_migrations_all_markers_and_decryptable_chat_content() {
    let fixture = fixture_path();
    assert!(
        fixture.exists(),
        "T0-Fixture fehlt unter {fixture:?} — s. Moduldoku `generate_fixture_once`"
    );

    // Kopfbytes der EINGECHECKTEN Datei, nicht der Kopie: das belegt, dass
    // die Fixture selbst noch Klartext-SQLite ist (Spec 0101, §1: "heutiger
    // Build" = vor A1/SQLCipher) — unabhängig davon, was das Öffnen später
    // daraus macht.
    let header = std::fs::read(&fixture).expect("Fixture lesbar");
    assert!(
        header.starts_with(b"SQLite format 3\0"),
        "T0-Fixture muss eine unverschlüsselte SQLite-Datei sein (Kopfbytes)"
    );

    let tmp_dir = tempfile::tempdir().expect("Temp-Verzeichnis anlegbar");
    let copy_path = tmp_dir.path().join("t0-copy.sqlite3");
    std::fs::copy(&fixture, &copy_path).expect("Fixture kopierbar");
    // Spec 0101, §9 Klarstellung 2: Prüfsummen in der Kopie auf die des
    // laufenden Builds ausrichten, bevor `connect` migriert — sonst scheitert
    // das auf Windows (CRLF-Checkout der `.sql`-Dateien) mit
    // `Migrate(VersionMismatch(1))`, s.
    // `test_align_migration_checksums_recovers_opening_from_a_deliberate_mismatch`
    // unten für den Beleg.
    align_migration_checksums_to_current_build(&copy_path).await;

    let store = SqliteProfileStore::connect(&copy_path)
        .await
        .expect("T0-Fixture sollte mit dem aktuellen Build weiter öffenbar sein");

    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&store.pool)
        .await
        .expect("_sqlx_migrations lesbar");
    assert_eq!(applied, 14, "T0-Fixture muss alle 14 Migrationen tragen");

    let max_version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&store.pool)
        .await
        .expect("_sqlx_migrations lesbar");
    assert_eq!(max_version, 14);

    let servers = store.list_servers().await.expect("Server lesbar");
    assert_eq!(servers.len(), 1, "genau ein Beispiel-Server erwartet");
    assert_eq!(servers[0].host, HOST_MARKER);
    assert_eq!(servers[0].username, USER_MARKER);
    let server_id = servers[0].id;

    let providers = store
        .ai_provider_store()
        .list()
        .await
        .expect("Provider lesbar");
    assert_eq!(providers.len(), 1, "genau ein Beispiel-Provider erwartet");
    assert!(
        providers[0]
            .extra_headers
            .iter()
            .any(|(_, v)| v == HEADER_MARKER),
        "Provider-Header-Marker fehlt: {:?}",
        providers[0].extra_headers
    );

    let chat_store = store.chat_session_store(test_cipher());
    let sessions = chat_store
        .list_sessions_for_server(&server_id)
        .await
        .expect("Sitzungen lesbar");
    assert_eq!(sessions.len(), 1, "genau eine Beispiel-Sitzung erwartet");
    let messages = chat_store
        .load_session(sessions[0].id)
        .await
        .expect("Chat-Nachrichten unter T0_TEST_KEY entschlüsselbar");
    let found = messages.iter().any(|m| match &m.content {
        MessageContent::Text(text) => text.contains(CHAT_MARKER),
        _ => false,
    });
    assert!(
        found,
        "Chat-Marker nach Entschlüsselung mit T0_TEST_KEY nicht gefunden: {messages:?}"
    );

    store.pool.close().await;
}

/// Beleg zu `align_migration_checksums_to_current_build` (Spec 0101, §9
/// Klarstellung 2): verfälscht in zwei **Kopien** der Fixture gezielt die
/// Prüfsumme der ersten Migration (steht hier für eine CRLF-bedingte
/// Abweichung, wie sie unter Windows am echten Build entsteht) und zeigt,
/// dass das Öffnen ohne die Hilfsfunktion mit `Migrate(VersionMismatch(1))`
/// scheitert, mit ihr aber gelingt — der Gegenbeweis, dass die
/// Hilfsfunktion tatsächlich greift, nicht nur zufällig keinen Unterschied
/// macht.
#[tokio::test]
async fn test_align_migration_checksums_recovers_opening_from_a_deliberate_mismatch() {
    let fixture = fixture_path();
    assert!(
        fixture.exists(),
        "T0-Fixture fehlt unter {fixture:?} — s. Moduldoku `generate_fixture_once`"
    );
    let tmp_dir = tempfile::tempdir().expect("Temp-Verzeichnis anlegbar");

    async fn corrupt_first_migration_checksum(copy_path: &Path) {
        let options = sqlx::sqlite::SqliteConnectOptions::new().filename(copy_path);
        let mut conn = sqlx::SqliteConnection::connect_with(&options)
            .await
            .expect("Kopie zum Verfälschen öffenbar");
        sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 1")
            .execute(&mut conn)
            .await
            .expect("Prüfsumme verfälschbar");
        conn.close().await.expect("Verbindung schließbar");
    }

    // Ohne Hilfsfunktion: das Öffnen muss mit genau diesem Fehler scheitern.
    let without_fix = tmp_dir.path().join("t0-mismatch-without-fix.sqlite3");
    std::fs::copy(&fixture, &without_fix).expect("Fixture kopierbar");
    corrupt_first_migration_checksum(&without_fix).await;
    match SqliteProfileStore::connect(&without_fix).await {
        Err(PersistenceError::Migrate(sqlx::migrate::MigrateError::VersionMismatch(version))) => {
            assert_eq!(version, 1);
        }
        Err(other) => panic!(
            "erwartet: Migrate(VersionMismatch(1)) bei verfälschter Prüfsumme ohne Abgleich; \
             erhalten: {other}"
        ),
        Ok(_) => panic!(
            "erwartet: Öffnen scheitert an der verfälschten Prüfsumme ohne Abgleich, \
             ist aber geglückt"
        ),
    }

    // Mit Hilfsfunktion, auf derselben Verfälschung: das Öffnen muss gelingen.
    let with_fix = tmp_dir.path().join("t0-mismatch-with-fix.sqlite3");
    std::fs::copy(&fixture, &with_fix).expect("Fixture kopierbar");
    corrupt_first_migration_checksum(&with_fix).await;
    align_migration_checksums_to_current_build(&with_fix).await;
    let store = SqliteProfileStore::connect(&with_fix)
        .await
        .expect("nach dem Abgleich sollte die Kopie trotz vorheriger Verfälschung öffenbar sein");
    store.pool.close().await;
}
