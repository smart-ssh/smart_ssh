//! Issue #113: die einmalige Umstellung der feldweise verschlüsselten
//! Spalten auf Klartext (`crate::field_content_decryption`).
//!
//! Die Altzeilen entstehen hier so, wie die Stores sie bis Issue #113
//! geschrieben haben: `nonce || ciphertext` (ChaCha20-Poly1305 unter K) als
//! Blob in der Spalte. Gelesen wird danach über die echten Stores — „content
//! read through the app is identical to before".

use std::path::Path;

use chrono::Utc;
use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::crypto::legacy_field_content::encrypt_for_tests;
use ssh_manager_core::crypto::DatabaseKey;
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::test_support::FIELD_ENCRYPTED_COLUMNS;
use crate::{ColumnCounts, FieldContentDecryption, SqliteProfileStore};

/// K dieser Tests.
const K: [u8; 32] = [0x5a; 32];
/// Ein anderer K — Zeilen darunter sind mit [`K`] nicht lesbar, wie nach
/// „Neuen Schlüssel erzeugen" (Spec 0101, D4).
const OTHER_K: [u8; 32] = [0xa5; 32];

/// Der Inhalt, den die App vor und nach der Umstellung sehen muss.
struct Expected {
    server_id: ServerId,
    session_id: Uuid,
    messages: Vec<ChatMessage>,
    ledger: Vec<LedgerEntryContent>,
    prompts: Vec<String>,
    summary: (String, i64),
}

fn test_server() -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: "Test-Server".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: vec![],
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    }
}

async fn insert_message(store: &SqliteProfileStore, session_id: Uuid, seq: i64, blob: Vec<u8>) {
    sqlx::query(
        "INSERT INTO chat_messages \
         (id, session_id, role, content_type, content, sequence, created_at) \
         VALUES (?, ?, 'user', 'text', ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id.to_string())
    .bind(blob)
    .bind(seq)
    .bind(Utc::now().to_rfc3339())
    .execute(&store.pool)
    .await
    .unwrap();
}

async fn insert_ledger(store: &SqliteProfileStore, session_id: Uuid, seq: i64, blob: Vec<u8>) {
    sqlx::query(
        "INSERT INTO ledger_entries \
         (id, session_id, sequence, source, entry_type, content, created_at) \
         VALUES (?, ?, ?, 'ai', 'command_proposed', ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id.to_string())
    .bind(seq)
    .bind(blob)
    .bind(Utc::now().to_rfc3339())
    .execute(&store.pool)
    .await
    .unwrap();
}

async fn insert_prompt(
    store: &SqliteProfileStore,
    server_id: ServerId,
    minute: u32,
    blob: Vec<u8>,
) {
    sqlx::query(
        "INSERT INTO prompt_history (id, server_id, content, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(server_id.0.to_string())
    .bind(blob)
    .bind(format!("2026-01-01T00:{minute:02}:00+00:00"))
    .execute(&store.pool)
    .await
    .unwrap();
}

async fn set_summary(store: &SqliteProfileStore, session_id: Uuid, blob: Vec<u8>, rounds: i64) {
    sqlx::query(
        "UPDATE chat_sessions SET summary_text = ?, summary_rounds_covered = ? WHERE id = ?",
    )
    .bind(blob)
    .bind(rounds)
    .bind(session_id.to_string())
    .execute(&store.pool)
    .await
    .unwrap();
}

/// Legt einen Server und eine Sitzung an und schreibt Altzeilen unter `K` in
/// alle vier Spalten.
async fn plant_legacy_rows(store: &SqliteProfileStore) -> Expected {
    let server = test_server();
    let server_id = server.id;
    store.create_server(&server).await.unwrap();
    let session_id = store
        .chat_session_store()
        .create_session(&server_id, None)
        .await
        .unwrap();

    let messages = vec![
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Wie voll ist /var? äöü".to_string()),
        },
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Und /home?".to_string()),
        },
    ];
    for (seq, message) in messages.iter().enumerate() {
        let json = serde_json::to_string(&message.content).unwrap();
        insert_message(store, session_id, seq as i64, encrypt_for_tests(&K, &json)).await;
    }

    let ledger = vec![
        LedgerEntryContent::CommandProposed {
            command: "df -h /var".to_string(),
        },
        LedgerEntryContent::CommandProposed {
            command: "df -h /home".to_string(),
        },
    ];
    for (seq, entry) in ledger.iter().enumerate() {
        let json = serde_json::to_string(entry).unwrap();
        insert_ledger(store, session_id, seq as i64, encrypt_for_tests(&K, &json)).await;
    }

    let prompts = vec!["df -h".to_string(), "du -sh /var/log".to_string()];
    for (minute, prompt) in prompts.iter().enumerate() {
        insert_prompt(
            store,
            server_id,
            minute as u32,
            encrypt_for_tests(&K, prompt),
        )
        .await;
    }

    let summary = ("Plattenplatz geprüft, /var zu 80 % voll".to_string(), 2);
    set_summary(
        store,
        session_id,
        encrypt_for_tests(&K, &summary.0),
        summary.1,
    )
    .await;

    Expected {
        server_id,
        session_id,
        messages,
        ledger,
        prompts,
        summary,
    }
}

/// Liest alles über die echten Stores und vergleicht mit `expected`.
async fn assert_readable_through_the_app(store: &SqliteProfileStore, expected: &Expected) {
    let messages = store
        .chat_session_store()
        .load_session(expected.session_id)
        .await
        .expect("chat readable");
    assert_eq!(messages, expected.messages);
    let ledger: Vec<LedgerEntryContent> = store
        .ledger_store()
        .load_entries(expected.session_id)
        .await
        .expect("ledger readable")
        .into_iter()
        .map(|e| e.content)
        .collect();
    assert_eq!(ledger, expected.ledger);
    assert_eq!(
        store
            .prompt_history_store()
            .list(&expected.server_id)
            .await
            .expect("prompt history readable"),
        expected.prompts
    );
    assert_eq!(
        store
            .chat_session_store()
            .load_summary(expected.session_id)
            .await
            .expect("summary readable"),
        Some(expected.summary.clone())
    );
}

/// Wie viele Werte der vier Spalten noch Blobs sind.
async fn blob_counts(store: &SqliteProfileStore) -> Vec<(String, i64)> {
    let mut counts = Vec::new();
    for (table, column) in FIELD_ENCRYPTED_COLUMNS {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE typeof({column}) = 'blob'");
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(&store.pool)
            .await
            .unwrap();
        counts.push((format!("{table}.{column}"), n));
    }
    counts
}

async fn decryption_state(store: &SqliteProfileStore) -> String {
    sqlx::query_scalar("SELECT state FROM field_content_decryption_state WHERE id = 1")
        .fetch_one(&store.pool)
        .await
        .unwrap()
}

async fn open_encrypted(path: &Path) -> SqliteProfileStore {
    SqliteProfileStore::connect_encrypted(path, &DatabaseKey::from_root_key(&K))
        .await
        .expect("encrypted test database opens")
}

/// AC 1: Nach der Umstellung trägt keine der vier Spalten mehr ein
/// Chiffrat, und was die App liest, ist derselbe Inhalt wie vorher.
#[tokio::test(flavor = "multi_thread")]
async fn test_all_four_columns_are_decrypted_and_read_back_identically() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_encrypted(&dir.path().join("smart-ssh.db")).await;
    let expected = plant_legacy_rows(&store).await;
    assert!(
        blob_counts(&store).await.iter().all(|(_, n)| *n > 0),
        "every column must start with legacy rows: {:?}",
        blob_counts(&store).await
    );

    let outcome = store.decrypt_field_encrypted_content(&K).await.unwrap();

    let FieldContentDecryption::Completed(report) = outcome else {
        panic!("expected a completed run, got {outcome:?}");
    };
    let two = ColumnCounts {
        decrypted: 2,
        removed: 0,
    };
    assert_eq!(report.chat_messages, two);
    assert_eq!(report.ledger_entries, two);
    assert_eq!(report.prompt_history, two);
    assert_eq!(
        report.summaries,
        ColumnCounts {
            decrypted: 1,
            removed: 0
        }
    );
    assert!(
        blob_counts(&store).await.iter().all(|(_, n)| *n == 0),
        "no encrypted blob may remain: {:?}",
        blob_counts(&store).await
    );
    assert_eq!(decryption_state(&store).await, "done");
    assert_readable_through_the_app(&store, &expected).await;
    store.close().await;
}

/// Ein zweiter Lauf fasst nichts mehr an — auch keinen Blob, der danach
/// noch auftauchen würde (es gibt keinen Weg mehr, der einen schreibt).
#[tokio::test(flavor = "multi_thread")]
async fn test_a_second_run_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_encrypted(&dir.path().join("smart-ssh.db")).await;
    let expected = plant_legacy_rows(&store).await;
    store.decrypt_field_encrypted_content(&K).await.unwrap();

    assert_eq!(
        store.decrypt_field_encrypted_content(&K).await.unwrap(),
        FieldContentDecryption::AlreadyDone
    );
    assert_readable_through_the_app(&store, &expected).await;
    store.close().await;
}

/// Eine frische Installation hat nichts umzustellen und steht danach auf
/// `done`.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_fresh_database_completes_with_nothing_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_encrypted(&dir.path().join("smart-ssh.db")).await;
    assert_eq!(decryption_state(&store).await, "open");

    let outcome = store.decrypt_field_encrypted_content(&K).await.unwrap();

    assert_eq!(
        outcome,
        FieldContentDecryption::Completed(Default::default())
    );
    assert_eq!(decryption_state(&store).await, "done");
    store.close().await;
}

/// Eine Prompt-Historie-Zeile aus der Zeit vor Spec 0040 (nie
/// verschlüsselter Text) bleibt, wie sie ist — sie war nie ein Chiffrat.
/// Deckt die Zeilen ab, die früher `migrate_legacy_plaintext_content`
/// verschlüsselt hätte.
#[tokio::test(flavor = "multi_thread")]
async fn test_pre_0040_plaintext_prompt_rows_are_kept_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_encrypted(&dir.path().join("smart-ssh.db")).await;
    let server = test_server();
    store.create_server(&server).await.unwrap();
    sqlx::query(
        "INSERT INTO prompt_history (id, server_id, content, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(server.id.0.to_string())
    .bind("alter Klartext-Prompt")
    .bind(Utc::now().to_rfc3339())
    .execute(&store.pool)
    .await
    .unwrap();

    let outcome = store.decrypt_field_encrypted_content(&K).await.unwrap();

    assert_eq!(
        outcome,
        FieldContentDecryption::Completed(Default::default())
    );
    assert_eq!(
        store.prompt_history_store().list(&server.id).await.unwrap(),
        vec!["alter Klartext-Prompt".to_string()]
    );
    store.close().await;
}

/// AC 5 (Entscheidung im Issue: entfernen und einmal hinweisen): Je Spalte
/// wird eine Zeile, die unter einem anderen K steht, entfernt — die
/// lesbaren daneben bleiben. Nachricht, Ledger-Eintrag und Historie
/// verschwinden als Zeile; bei der Zusammenfassung verschwindet nur sie,
/// die Sitzung bleibt. Eine zu kurze, kaputte Zeile zählt genauso.
#[tokio::test(flavor = "multi_thread")]
async fn test_rows_not_decryptable_with_the_current_key_are_removed_in_every_column() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_encrypted(&dir.path().join("smart-ssh.db")).await;
    let expected = plant_legacy_rows(&store).await;

    // Je Spalte eine Zeile unter einem anderen K, dazu eine kaputte
    // Nachricht, zu kurz für einen Nonce.
    let foreign_json = serde_json::to_string(&MessageContent::Text("fremd".into())).unwrap();
    insert_message(
        &store,
        expected.session_id,
        10,
        encrypt_for_tests(&OTHER_K, &foreign_json),
    )
    .await;
    insert_message(&store, expected.session_id, 11, vec![1, 2, 3]).await;
    let foreign_ledger = serde_json::to_string(&LedgerEntryContent::CommandProposed {
        command: "fremd".into(),
    })
    .unwrap();
    insert_ledger(
        &store,
        expected.session_id,
        10,
        encrypt_for_tests(&OTHER_K, &foreign_ledger),
    )
    .await;
    insert_prompt(
        &store,
        expected.server_id,
        30,
        encrypt_for_tests(&OTHER_K, "fremd"),
    )
    .await;
    // Zweite Sitzung, deren Zusammenfassung unter einem anderen K steht.
    let other_session = store
        .chat_session_store()
        .create_session(&expected.server_id, None)
        .await
        .unwrap();
    set_summary(
        &store,
        other_session,
        encrypt_for_tests(&OTHER_K, "fremd"),
        4,
    )
    .await;

    let outcome = store.decrypt_field_encrypted_content(&K).await.unwrap();

    let FieldContentDecryption::Completed(report) = outcome else {
        panic!("expected a completed run, got {outcome:?}");
    };
    assert_eq!(
        report.chat_messages,
        ColumnCounts {
            decrypted: 2,
            removed: 2
        }
    );
    assert_eq!(
        report.ledger_entries,
        ColumnCounts {
            decrypted: 2,
            removed: 1
        }
    );
    assert_eq!(
        report.prompt_history,
        ColumnCounts {
            decrypted: 2,
            removed: 1
        }
    );
    assert_eq!(
        report.summaries,
        ColumnCounts {
            decrypted: 1,
            removed: 1
        }
    );
    assert_eq!(report.removed_total(), 5);
    assert!(blob_counts(&store).await.iter().all(|(_, n)| *n == 0));

    // Die lesbaren Zeilen sind vollständig da, die unlesbaren weg.
    assert_readable_through_the_app(&store, &expected).await;
    let chat = store.chat_session_store();
    assert_eq!(chat.load_summary(other_session).await.unwrap(), None);
    let rounds: Option<i64> =
        sqlx::query_scalar("SELECT summary_rounds_covered FROM chat_sessions WHERE id = ?")
            .bind(other_session.to_string())
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(rounds, None, "the round count goes with the summary");
    let sessions = chat
        .list_sessions_for_server(&expected.server_id)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 2, "no session may be deleted: {sessions:?}");
    store.close().await;
}

/// AC 4: Ein Lauf, der nach einem Teil der Zeilen scheitert, lässt eine
/// lesbare Datenbank im alten Stand zurück (Rollback, Zustand `open`), und
/// der nächste Lauf — nach Schließen und erneutem Öffnen, wie bei einem
/// Neustart — stellt alles vollständig um.
#[tokio::test(flavor = "multi_thread")]
async fn test_an_interrupted_run_keeps_the_old_state_and_completes_on_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart-ssh.db");
    let store = open_encrypted(&path).await;
    let expected = plant_legacy_rows(&store).await;
    let blobs_before = blob_counts(&store).await;

    // 3 von 7 Altzeilen bearbeitet: beide Nachrichten und ein Ledger-Eintrag.
    let interrupted = store
        .decrypt_field_encrypted_content_failing_after(&K, 3)
        .await;
    assert!(interrupted.is_err(), "the injected failure must surface");
    store.close().await;

    // Neustart: die Datei öffnet, alles steht noch im alten Stand.
    let store = open_encrypted(&path).await;
    assert_eq!(decryption_state(&store).await, "open");
    assert_eq!(
        blob_counts(&store).await,
        blobs_before,
        "an interrupted run must not leave a half-converted column"
    );
    let rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM chat_messages) + (SELECT COUNT(*) FROM ledger_entries) \
         + (SELECT COUNT(*) FROM prompt_history)",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(rows, 6, "no row may be lost by the interrupted run");

    // Der nächste Lauf stellt vollständig um.
    let outcome = store.decrypt_field_encrypted_content(&K).await.unwrap();
    let FieldContentDecryption::Completed(report) = outcome else {
        panic!("expected a completed run, got {outcome:?}");
    };
    assert_eq!(report.decrypted_total(), 7);
    assert_eq!(report.removed_total(), 0);
    assert!(blob_counts(&store).await.iter().all(|(_, n)| *n == 0));
    assert_readable_through_the_app(&store, &expected).await;
    store.close().await;
}

/// AC 3: Nach der Umstellung stehen die vier Spalten als Klartext in der
/// Datenbank — aber nicht auf der Platte: Die Datei ist verschlüsselt, kein
/// Inhalt aus einer der Spalten steht roh in einer Datei des Verzeichnisses.
#[tokio::test(flavor = "multi_thread")]
async fn test_decrypted_content_never_appears_in_any_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart-ssh.db");
    let store = open_encrypted(&path).await;
    let markers = [
        "Marker-113-chat",
        "Marker-113-ledger",
        "Marker-113-prompt",
        "Marker-113-summary",
    ];
    let server = test_server();
    store.create_server(&server).await.unwrap();
    let session_id = store
        .chat_session_store()
        .create_session(&server.id, None)
        .await
        .unwrap();
    let chat_json = serde_json::to_string(&MessageContent::Text(markers[0].into())).unwrap();
    insert_message(&store, session_id, 0, encrypt_for_tests(&K, &chat_json)).await;
    let ledger_json = serde_json::to_string(&LedgerEntryContent::CommandProposed {
        command: markers[1].into(),
    })
    .unwrap();
    insert_ledger(&store, session_id, 0, encrypt_for_tests(&K, &ledger_json)).await;
    insert_prompt(&store, server.id, 0, encrypt_for_tests(&K, markers[2])).await;
    set_summary(&store, session_id, encrypt_for_tests(&K, markers[3]), 1).await;

    store.decrypt_field_encrypted_content(&K).await.unwrap();
    // Gegenprobe innerhalb der Datenbank: die Werte sind jetzt Klartext.
    let chat_raw: String = sqlx::query_scalar("SELECT content FROM chat_messages")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert!(chat_raw.contains(markers[0]));
    // Und ein Neuschreiben über die Stores dazu.
    store
        .ledger_store()
        .append_entry(
            session_id,
            LedgerSource::User,
            &LedgerEntryContent::CommandProposed {
                command: markers[1].into(),
            },
        )
        .await
        .unwrap();
    store.close().await;

    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        let bytes = std::fs::read(entry.path()).unwrap();
        for marker in markers {
            assert!(
                !bytes
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "{marker} stands in plaintext in {:?}",
                entry.file_name()
            );
        }
    }
}
