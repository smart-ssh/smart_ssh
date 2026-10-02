//! Spec 0096, A3/A4: Rohdatei-Nachweis.
//!
//! Die bestehenden Store-Tests
//! (`test_direct_sql_access_to_content_column_never_reveals_plaintext` in
//! `chat_session_store`, `ledger_store`, `prompt_history_store`) prüfen je
//! **eine Spalte** per `SELECT` — sie sehen die Datenbank also durch dieselbe
//! SQLite-Brille, die auch beim Schreiben im Spiel war. Was sie nicht sehen
//! können: Seiten, die SQLite zusätzlich in der `-wal`-Datei hält, und alles,
//! was neben der geprüften Spalte in derselben Datei landet.
//!
//! Dieser Nachweis geht deshalb eine Ebene tiefer: er schreibt über die
//! echten Stores, schließt den Pool und durchsucht danach **jede Datei** des
//! Datenbank-Verzeichnisses byteweise — ohne SQLite, ohne SQL.
//!
//! **Grenze des Nachweises (bewusst):** gesucht werden die UTF-8-Bytes des
//! Geheimnisses. Läge dasselbe Geheimnis in einer anderen Kodierung
//! (UTF-16, Base64, komprimiert) in einer der Dateien, fände diese Suche es
//! nicht. Für die geprüften Schreibpfade ist UTF-8 die einzige Kodierung, die
//! entsteht (`ContentCipher::encrypt` nimmt `&str`, `serde_json` schreibt
//! UTF-8) — für neue Schreibpfade gilt das nicht automatisch.

use std::path::Path;
use std::sync::Arc;

use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::crypto::{
    ChaCha20Poly1305Cipher, CipherError, ContentCipher, EncryptedContent,
};
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::CommandOutput;

use crate::SqliteProfileStore;

/// Spec 0096, Abschnitt 7: das Geheimnis, nach dem gesucht wird. Bewusst
/// **ohne** die `password=`-Umgebung — gesucht wird der Wert selbst, denn er
/// ist es, der nicht im Klartext auf der Platte stehen darf.
const SECRET: &str = "Geheim-0096";

/// Länge des Nonce-Präfixes aus `EncryptedContent::to_blob`. Die Konstante
/// selbst ist in `ssh_manager_core::crypto` privat; ändert sie sich, bricht
/// hier der Typ des Array-Literals auf — sichtbar, nicht still.
const NONCE_LEN: usize = 12;

/// Cipher, der den Klartext unverändert durchreicht — **nur** für die
/// Gegenprobe A4. Er belegt, dass die Suchfunktion in T3 tatsächlich
/// zuschlagen würde, wenn Klartext in einer der Dateien stünde; ohne ihn
/// wäre T3 ein Test, der auch dann grün bliebe, wenn die Suche gar nichts
/// liest.
struct PassthroughCipher;

impl ContentCipher for PassthroughCipher {
    fn encrypt(&self, plaintext: &str) -> Result<EncryptedContent, CipherError> {
        Ok(EncryptedContent {
            ciphertext: plaintext.as_bytes().to_vec(),
            nonce: [0u8; NONCE_LEN],
        })
    }

    fn decrypt(&self, data: &EncryptedContent) -> Result<String, CipherError> {
        Ok(String::from_utf8_lossy(&data.ciphertext).into_owned())
    }
}

fn test_server(server_id: ServerId) -> Server {
    let now = chrono::Utc::now();
    Server {
        id: server_id,
        name: "Test-Server".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        // Ausdrücklich ohne Geheimnis: `servers.notes` ist Klartext (Spec
        // 0096 §1) und laut §3 auch ein Nicht-Ziel — stünde das Geheimnis
        // hier, würde der Nachweis aus dem falschen Grund rot.
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        created_at: now,
        updated_at: now,
    }
}

/// Schreibt mit `cipher` über die **echten** Stores je einen Eintrag mit dem
/// Geheimnis in alle fünf von Spec 0096, A3 genannten Senken und gibt den
/// noch **offenen** Store zurück. Der Aufrufer entscheidet, ob er vor oder
/// nach `pool.close()` liest — beide Zeitpunkte werden geprüft (s. T3 und
/// `test_t3_..._while_the_database_is_still_open`).
async fn write_secret_through_all_stores(
    db_path: &Path,
    cipher: Arc<dyn ContentCipher>,
) -> SqliteProfileStore {
    let profile_store = SqliteProfileStore::connect_plaintext(db_path)
        .await
        .expect("frische DB mit angewendeten Migrationen sollte immer aufbaubar sein");

    let server_id = ServerId::new();
    profile_store
        .create_server(&test_server(server_id))
        .await
        .expect("servers-Zeile ist FK-Voraussetzung fuer chat_sessions/prompt_history");

    let chat_store = profile_store.chat_session_store(Arc::clone(&cipher));
    let session_id = chat_store
        .create_session(&server_id, None)
        .await
        .expect("chat_sessions-Zeile anlegen");

    // 1. Chat-Nachricht (Text) — was der Nutzer getippt hat.
    chat_store
        .append_message(
            session_id,
            &ssh_manager_core::ai::ChatMessage {
                role: ssh_manager_core::ai::Role::User,
                content: ssh_manager_core::ai::MessageContent::Text(format!(
                    "verbinde dich mit password={SECRET}"
                )),
            },
        )
        .await
        .expect("Chat-Nachricht schreiben");

    // 2. Kommando-Ergebnis (Kommando UND Ausgabe) — beide Felder tragen das
    //    Geheimnis, damit ein Nachweis, der nur eines davon abdeckt, auffällt.
    chat_store
        .append_message(
            session_id,
            &ssh_manager_core::ai::ChatMessage {
                role: ssh_manager_core::ai::Role::ActionResult,
                content: ssh_manager_core::ai::MessageContent::CommandResult {
                    command: format!("mysql --password={SECRET}"),
                    output: CommandOutput {
                        stdout: format!("verbunden mit password={SECRET}").into_bytes(),
                        stderr: format!("warnung: password={SECRET} auf der Kommandozeile")
                            .into_bytes(),
                        exit_code: Some(0),
                        truncated: false,
                    },
                    cancelled: false,
                },
            },
        )
        .await
        .expect("Kommando-Ergebnis schreiben");

    // 3. Zusammenfassung.
    chat_store
        .save_summary(
            session_id,
            &format!("Sitzung: Datenbank mit password={SECRET} geprueft"),
            1,
        )
        .await
        .expect("Zusammenfassung schreiben");

    // 4. Ausführungsprotokoll.
    profile_store
        .ledger_store(Arc::clone(&cipher))
        .append_entry(
            session_id,
            LedgerSource::User,
            &LedgerEntryContent::CommandExecuted {
                command: format!("mysql --password={SECRET}"),
                output: CommandOutput {
                    stdout: format!("ok, password={SECRET}").into_bytes(),
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
                cancelled: false,
            },
        )
        .await
        .expect("Ledger-Eintrag schreiben");

    // 5. Eingabe-Historie.
    profile_store
        .prompt_history_store(Arc::clone(&cipher))
        .record(&server_id, &format!("mysql --password={SECRET}"))
        .await
        .expect("Eingabe-Historie schreiben");

    profile_store
}

/// Durchsucht **jede** Datei des Verzeichnisses byteweise nach dem Geheimnis.
/// Liefert `(alle gelesenen Dateinamen, Dateinamen mit Treffer)`.
///
/// Bewusst `read_dir` statt einer festen Liste aus Datenbankdatei, `-wal`,
/// `-shm` und `-journal`: welche Begleitdateien SQLite anlegt, hängt vom
/// Journal-Modus und vom Zeitpunkt ab. Eine feste Liste würde eine künftig
/// neu hinzukommende Datei stillschweigend überspringen — genau die Lücke,
/// die dieser Nachweis schließen soll.
fn scan_directory_for_secret(dir: &Path) -> (Vec<String>, Vec<String>) {
    let needle = SECRET.as_bytes();
    let mut searched = Vec::new();
    let mut hits = Vec::new();

    for entry in std::fs::read_dir(dir).expect("Verzeichnis lesbar") {
        let entry = entry.expect("Verzeichniseintrag lesbar");
        if !entry.file_type().expect("Dateityp lesbar").is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let bytes = std::fs::read(entry.path()).expect("Datei lesbar");
        if bytes.windows(needle.len()).any(|window| window == needle) {
            hits.push(name.clone());
        }
        searched.push(name);
    }

    searched.sort();
    hits.sort();
    (searched, hits)
}

/// T3 (Spec 0096, A3): Nach einer Sitzung, in der das Geheimnis in Prompt,
/// Kommando, Ausgabe, Ausführungsprotokoll, Eingabe-Historie und
/// Zusammenfassung vorkam, enthält **keine** Datei des Datenbank-
/// Verzeichnisses das Geheimnis im Klartext.
#[tokio::test]
async fn test_t3_spec_0096_no_plaintext_secret_in_any_database_file() {
    let tmp_dir = tempfile::tempdir().expect("Temp-Verzeichnis anlegbar");
    let db_path = tmp_dir.path().join("smart-ssh.sqlite3");

    let store = write_secret_through_all_stores(
        &db_path,
        Arc::new(ChaCha20Poly1305Cipher::new(&[42u8; 32])),
    )
    .await;
    // Erst schließen, dann lesen: beim Schließen überträgt SQLite den Inhalt
    // des WAL in die Hauptdatei. Wer vorher liest, prüft einen
    // Zwischenstand und übersieht genau die Bytes, die erst dabei umziehen.
    store.pool.close().await;

    let (searched, hits) = scan_directory_for_secret(tmp_dir.path());
    // Mit `--nocapture` sichtbar: welche Dateien der Nachweis tatsächlich
    // gelesen hat. Ohne diese Angabe ließe sich ein grünes T3 nicht von
    // einem unterscheiden, das ein leeres Verzeichnis durchsucht hat.
    println!("T3 hat diese Dateien durchsucht: {searched:?}");

    assert!(
        searched.iter().any(|name| name == "smart-ssh.sqlite3"),
        "die Datenbankdatei selbst muss durchsucht worden sein, sonst belegt der Test nichts: \
         {searched:?}"
    );
    assert!(
        hits.is_empty(),
        "Klartext-Geheimnis in Datei(en) {hits:?} gefunden (durchsucht: {searched:?})"
    );
}

/// T4 (Spec 0096, A4): Derselbe Ablauf gegen eine eigene Temp-Datenbank mit
/// einem Cipher, der Klartext durchreicht — **dieselbe** Suchfunktion muss
/// das Geheimnis dort finden. Ohne diesen Gegenbeweis wäre T3 auch dann
/// grün, wenn `scan_directory_for_secret` gar nichts läse.
#[tokio::test]
async fn test_t4_spec_0096_the_scan_finds_the_secret_without_encryption() {
    let tmp_dir = tempfile::tempdir().expect("Temp-Verzeichnis anlegbar");
    let db_path = tmp_dir.path().join("smart-ssh.sqlite3");

    let store = write_secret_through_all_stores(&db_path, Arc::new(PassthroughCipher)).await;
    store.pool.close().await;

    let (searched, hits) = scan_directory_for_secret(tmp_dir.path());

    assert!(
        !hits.is_empty(),
        "die Gegenprobe muss das Geheimnis finden — sonst prueft T3 nichts (durchsucht: \
         {searched:?})"
    );
}

/// Ergänzung zu T3 (Spec 0096, A3, Angriffsrichtung „Rohdatei-Suche, die nur
/// die Hauptdatei liest (WAL)"): **gemessen** hinterlässt ein sauber
/// geschlossener Pool gar keine `-wal`-Datei mehr — T3 durchsucht deshalb in
/// der Praxis nur `smart-ssh.sqlite3`, und die WAL-Angriffsrichtung bliebe
/// dort unbelegt.
///
/// Dieser Fall schließt die Lücke und deckt zugleich den realistischeren
/// Zustand ab: die Datenbank einer **laufenden** App. Dort existiert das WAL,
/// und dort dürfen die frisch geschriebenen Einträge genauso wenig im
/// Klartext stehen.
#[tokio::test]
async fn test_spec_0096_no_plaintext_secret_while_the_database_is_still_open() {
    let tmp_dir = tempfile::tempdir().expect("Temp-Verzeichnis anlegbar");
    let db_path = tmp_dir.path().join("smart-ssh.sqlite3");

    let store = write_secret_through_all_stores(
        &db_path,
        Arc::new(ChaCha20Poly1305Cipher::new(&[42u8; 32])),
    )
    .await;

    // Bewusst VOR `pool.close()`: genau jetzt liegen die frischen Seiten im
    // WAL statt in der Hauptdatei.
    let (searched, hits) = scan_directory_for_secret(tmp_dir.path());
    println!("Der Nachweis bei offener Datenbank hat durchsucht: {searched:?}");

    assert!(
        searched.iter().any(|name| name.ends_with("-wal")),
        "bei offener Datenbank muss ein WAL existieren und mitdurchsucht werden — sonst belegt \
         dieser Fall die WAL-Angriffsrichtung nicht: {searched:?}"
    );
    assert!(
        hits.is_empty(),
        "Klartext-Geheimnis in Datei(en) {hits:?} gefunden (durchsucht: {searched:?})"
    );

    store.pool.close().await;
}
