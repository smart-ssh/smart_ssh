//! SQLite-gestützte Persistenz für das Session-Ledger (Spec 0057, §1 +
//! §5).
//!
//! **Eigener Store**, aus demselben Grund wie [`crate::SqliteChatSessionStore`]/
//! [`crate::SqlitePromptHistoryStore`] (s. dortige Modul-Kommentare):
//! fachlich unabhängig, teilt sich aber denselben `SqlitePool` (s.
//! [`SqliteLedgerStore::new`] und `crate::store::SqliteProfileStore::
//! ledger_store`).
//!
//! **Feld-Verschlüsselung** (Spec 0036-Muster, ausgeweitet auf den Ledger
//! per Spec 0057, §1.3): `content` wird vor dem Schreiben über den
//! mitgegebenen [`ContentCipher`] verschlüsselt und beim Lesen entschlüsselt
//! — transparent für jeden Aufrufer dieses Stores, exakt wie bei
//! [`crate::SqliteChatSessionStore`]. Redaction ist **nicht** Aufgabe
//! dieses Stores (Spec 0057, §1.2: "Pflicht", aber vom Aufrufer VOR
//! [`SqliteLedgerStore::append_entry`] zu erledigen — s. `app-shell::
//! orchestration::write_ledger_entry`, die einzige vorgesehene Aufrufstelle
//! in der App).
//!
//! **Append-only** (Spec 0057, §1.4): es gibt bewusst **keine**
//! `update_entry`/`delete_entry`-Methode — nur [`SqliteLedgerStore::
//! append_entry`] (Schreiben) und [`SqliteLedgerStore::load_entries`]
//! (Lesen). Einträge verschwinden ausschließlich über die
//! `ON DELETE CASCADE`-Regel der Migration, wenn die ganze Sitzung
//! gelöscht wird — kein eigener Code-Pfad dafür in diesem Modul nötig.

use std::sync::Arc;

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use uuid::Uuid;

use ssh_manager_core::audit::LedgerEntryContent;
use ssh_manager_core::crypto::{CipherError, ContentCipher, EncryptedContent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerStoreError {
    Backend(String),
    /// Wie `ChatSessionStoreError::CorruptContent` (s. dortiger
    /// Doc-Kommentar): die geladene `content`-Spalte eines Eintrags ließ
    /// sich nach der Entschlüsselung nicht als `LedgerEntryContent`
    /// deserialisieren.
    CorruptContent {
        entry_id: String,
        reason: String,
    },
    /// Wie `ChatSessionStoreError::Cipher`.
    Cipher(CipherError),
}

impl std::fmt::Display for LedgerStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerStoreError::Backend(msg) => write!(f, "Datenbankfehler: {msg}"),
            LedgerStoreError::CorruptContent { entry_id, reason } => write!(
                f,
                "Ledger-Eintrag '{entry_id}' konnte nicht gelesen werden: {reason}"
            ),
            LedgerStoreError::Cipher(err) => write!(f, "Verschlüsselungsfehler: {err}"),
        }
    }
}

impl std::error::Error for LedgerStoreError {}

impl From<CipherError> for LedgerStoreError {
    fn from(err: CipherError) -> Self {
        LedgerStoreError::Cipher(err)
    }
}

fn backend_err(e: sqlx::Error) -> LedgerStoreError {
    LedgerStoreError::Backend(e.to_string())
}

/// Spiegelt `LedgerEntryContent`s Varianten in der `entry_type`-Spalte
/// (dieselbe Konvention wie `chat_session_store::content_type_for`) — rein
/// für Filterung/Übersicht, die tatsächlichen Daten liegen ausschließlich
/// im verschlüsselten `content`-Blob.
fn entry_type_for(content: &LedgerEntryContent) -> &'static str {
    match content {
        LedgerEntryContent::CommandProposed { .. } => "command_proposed",
        LedgerEntryContent::Decision { .. } => "decision",
        LedgerEntryContent::CommandExecuted { .. } => "command_executed",
        LedgerEntryContent::AiMessage { .. } => "ai_message",
    }
}

fn source_to_text(source: ssh_manager_core::audit::LedgerSource) -> &'static str {
    use ssh_manager_core::audit::LedgerSource;
    match source {
        LedgerSource::User => "user",
        LedgerSource::Ai => "ai",
        LedgerSource::McpAgent => "mcp-agent",
    }
}

/// Fail-safe auf `Ai` bei einem unbekannten Wert — dieselbe Haltung wie
/// `chat_session_store::role_from_text` (in der Praxis unerreichbar, solange
/// die `CHECK`-Constraint der Migration greift).
fn source_from_text(raw: &str) -> ssh_manager_core::audit::LedgerSource {
    use ssh_manager_core::audit::LedgerSource;
    match raw {
        "user" => LedgerSource::User,
        "mcp-agent" => LedgerSource::McpAgent,
        _ => LedgerSource::Ai,
    }
}

/// S. Moduldoc — teilt sich den Pool mit [`crate::SqliteProfileStore`].
#[derive(Clone)]
pub struct SqliteLedgerStore {
    pool: SqlitePool,
    cipher: Arc<dyn ContentCipher>,
}

impl SqliteLedgerStore {
    pub fn new(pool: SqlitePool, cipher: Arc<dyn ContentCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Hängt einen neuen Eintrag an das Ledger einer Sitzung an (Spec 0057,
    /// §1.4: append-only — kein Update-Pfad existiert). `content` MUSS
    /// bereits redigiert sein (Spec 0057, §1.2 "Pflicht") — dieser Store
    /// prüft/redigiert nicht selbst, s. Moduldoc.
    ///
    /// `sequence`-Vergabe analog zu `SqliteChatSessionStore::append_message`
    /// ("ein höher als die bisher höchste Sequenznummer dieser Sitzung").
    pub async fn append_entry(
        &self,
        session_id: Uuid,
        source: ssh_manager_core::audit::LedgerSource,
        content: &LedgerEntryContent,
    ) -> Result<Uuid, LedgerStoreError> {
        let session_id_str = session_id.to_string();

        let next_sequence: i64 = sqlx::query(
            "SELECT COALESCE(MAX(sequence), -1) + 1 AS next FROM ledger_entries \
             WHERE session_id = ?",
        )
        .bind(&session_id_str)
        .fetch_one(&self.pool)
        .await
        .map_err(backend_err)?
        .get("next");

        let content_json = serde_json::to_string(content).map_err(|e| {
            LedgerStoreError::Backend(format!(
                "LedgerEntryContent-Serialisierung fehlgeschlagen: {e}"
            ))
        })?;
        let content_blob = self.cipher.encrypt(&content_json)?.to_blob();

        let entry_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO ledger_entries \
             (id, session_id, sequence, source, entry_type, content, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry_id.to_string())
        .bind(&session_id_str)
        .bind(next_sequence)
        .bind(source_to_text(source))
        .bind(entry_type_for(content))
        .bind(content_blob)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        Ok(entry_id)
    }

    /// Lädt das vollständige Ledger einer Sitzung, sortiert nach
    /// `sequence` (Spec 0057, §1: "Reihenfolge-garantiert"). Entschlüsselt
    /// `content` transparent, wie `SqliteChatSessionStore::load_session`.
    pub async fn load_entries(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<LedgerEntry>, LedgerStoreError> {
        let rows = sqlx::query(
            "SELECT id, source, content, created_at FROM ledger_entries \
             WHERE session_id = ? ORDER BY sequence ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend_err)?;

        rows.into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let source_raw: String = row.get("source");
                let content_blob: Vec<u8> = row.get("content");
                let created_at_raw: String = row.get("created_at");
                let encrypted = EncryptedContent::from_blob(&content_blob)?;
                let content_json = self.cipher.decrypt(&encrypted)?;
                let content: LedgerEntryContent =
                    serde_json::from_str(&content_json).map_err(|e| {
                        LedgerStoreError::CorruptContent {
                            entry_id: id.clone(),
                            reason: e.to_string(),
                        }
                    })?;
                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_raw)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| {
                        LedgerStoreError::Backend(format!("ungültiger Zeitstempel: {e}"))
                    })?;
                Ok(LedgerEntry {
                    id,
                    source: source_from_text(&source_raw),
                    content,
                    created_at,
                })
            })
            .collect()
    }
}

/// Ein aus dem Ledger geladener, bereits entschlüsselter Eintrag (Spec
/// 0057, §1) — die "Zeile", nicht der reine [`LedgerEntryContent`]: ergänzt
/// um die DB-eigenen Felder (`id`, `source`, `created_at`), die
/// `LedgerEntryContent` selbst bewusst nicht trägt (s. dortiger
/// Doc-Kommentar in `ssh_manager_core::audit`).
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub id: String,
    pub source: ssh_manager_core::audit::LedgerSource,
    pub content: LedgerEntryContent,
    pub created_at: chrono::DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;

    use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerSource};
    use ssh_manager_core::ssh::CommandOutput;

    use super::*;
    use crate::SqliteProfileStore;

    fn test_cipher() -> Arc<dyn ContentCipher> {
        Arc::new(ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(
            &[42u8; 32],
        ))
    }

    async fn in_memory_ledger_store() -> (SqliteProfileStore, SqliteLedgerStore) {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let profile_store = SqliteProfileStore::connect_with(options)
            .await
            .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein");
        let ledger_store = profile_store.ledger_store(test_cipher());
        (profile_store, ledger_store)
    }

    /// `ledger_entries.session_id` referenziert `chat_sessions(id)`, die
    /// wiederum `servers(id)` referenziert — für die Tests deshalb ein
    /// echter Server + eine echte `chat_sessions`-Zeile, analog zu
    /// `chat_session_store::tests::create_test_server`.
    async fn create_test_session(profile_store: &SqliteProfileStore) -> Uuid {
        use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
        use ssh_manager_core::shared::ServerId;

        let now = Utc::now();
        let server = Server {
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
            created_at: now,
            updated_at: now,
        };
        let server_id = server.id;
        profile_store.create_server(&server).await.unwrap();

        let chat_store = profile_store.chat_session_store(Arc::new(
            ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(&[7u8; 32]),
        ));
        chat_store.create_session(&server_id, None).await.unwrap()
    }

    fn sample_output() -> CommandOutput {
        CommandOutput {
            stdout: b"PID TTY\n1 ?\n".to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
            truncated: false,
        }
    }

    /// Spec 0057, §7: "alles erfasst" — alle vier Ereignistypen, in
    /// Reihenfolge.
    #[tokio::test]
    async fn test_append_entry_persists_all_four_entry_types_in_order() {
        let (profile_store, ledger_store) = in_memory_ledger_store().await;
        let session_id = create_test_session(&profile_store).await;

        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::CommandProposed {
                    command: "ps aux".to_string(),
                },
            )
            .await
            .unwrap();
        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::Decision {
                    outcome: LedgerDecisionOutcome::AutoApproved,
                    reason: None,
                    code: None,
                    matched_rule: None,
                    matched_rule_origin: None,
                },
            )
            .await
            .unwrap();
        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::CommandExecuted {
                    command: "ps aux".to_string(),
                    output: sample_output(),
                    cancelled: false,
                },
            )
            .await
            .unwrap();
        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::AiMessage {
                    text: "Hier sind die laufenden Prozesse.".to_string(),
                },
            )
            .await
            .unwrap();

        let loaded = ledger_store.load_entries(session_id).await.unwrap();
        assert_eq!(loaded.len(), 4);
        assert!(matches!(
            loaded[0].content,
            LedgerEntryContent::CommandProposed { .. }
        ));
        assert!(matches!(
            loaded[1].content,
            LedgerEntryContent::Decision { .. }
        ));
        assert!(matches!(
            loaded[2].content,
            LedgerEntryContent::CommandExecuted { .. }
        ));
        assert!(matches!(
            loaded[3].content,
            LedgerEntryContent::AiMessage { .. }
        ));
    }

    #[tokio::test]
    async fn test_append_entry_round_trips_source_and_decision_details() {
        let (profile_store, ledger_store) = in_memory_ledger_store().await;
        let session_id = create_test_session(&profile_store).await;

        ledger_store
            .append_entry(
                session_id,
                LedgerSource::User,
                &LedgerEntryContent::Decision {
                    outcome: LedgerDecisionOutcome::Confirmed,
                    reason: Some("Bestätigung nötig".to_string()),
                    code: Some("FILTER_NO_RULE_MATCHED".to_string()),
                    matched_rule: Some(ssh_manager_core::filter::RuleId("allow-ls".to_string())),
                    matched_rule_origin: Some(ssh_manager_core::filter::RuleOrigin::User),
                },
            )
            .await
            .unwrap();

        let loaded = ledger_store.load_entries(session_id).await.unwrap();
        assert_eq!(loaded[0].source, LedgerSource::User);
        match &loaded[0].content {
            LedgerEntryContent::Decision {
                outcome,
                reason,
                code,
                matched_rule,
                matched_rule_origin,
            } => {
                assert_eq!(*outcome, LedgerDecisionOutcome::Confirmed);
                assert_eq!(reason.as_deref(), Some("Bestätigung nötig"));
                assert_eq!(code.as_deref(), Some("FILTER_NO_RULE_MATCHED"));
                assert_eq!(
                    matched_rule.as_ref().map(|r| r.0.as_str()),
                    Some("allow-ls")
                );
                assert_eq!(
                    *matched_rule_origin,
                    Some(ssh_manager_core::filter::RuleOrigin::User)
                );
            }
            other => panic!("erwartete Decision-Variante, bekam {other:?}"),
        }
    }

    /// Spec 0057, §1: "Reihenfolge-garantiert (append-only, monotone
    /// Sequenz)" — mehrere Einträge behalten ihre Einfügereihenfolge, auch
    /// über mehrere unabhängige `append_entry`-Aufrufe hinweg.
    #[tokio::test]
    async fn test_entries_persist_in_monotone_sequence_order() {
        let (profile_store, ledger_store) = in_memory_ledger_store().await;
        let session_id = create_test_session(&profile_store).await;

        for i in 0..5 {
            ledger_store
                .append_entry(
                    session_id,
                    LedgerSource::Ai,
                    &LedgerEntryContent::AiMessage {
                        text: format!("Nachricht {i}"),
                    },
                )
                .await
                .unwrap();
        }

        let loaded = ledger_store.load_entries(session_id).await.unwrap();
        let texts: Vec<&str> = loaded
            .iter()
            .map(|e| match &e.content {
                LedgerEntryContent::AiMessage { text } => text.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            texts,
            vec![
                "Nachricht 0",
                "Nachricht 1",
                "Nachricht 2",
                "Nachricht 3",
                "Nachricht 4"
            ]
        );
    }

    /// Spec 0057, §1.4/§6: Cascade-Delete wie andere Session-Daten.
    #[tokio::test]
    async fn test_deleting_session_cascades_to_its_ledger_entries() {
        let (profile_store, ledger_store) = in_memory_ledger_store().await;
        let session_id = create_test_session(&profile_store).await;
        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::AiMessage {
                    text: "wird verwaist".to_string(),
                },
            )
            .await
            .unwrap();

        sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&profile_store.pool)
            .await
            .unwrap();

        let count: i64 =
            sqlx::query("SELECT COUNT(*) AS c FROM ledger_entries WHERE session_id = ?")
                .bind(session_id.to_string())
                .fetch_one(&profile_store.pool)
                .await
                .unwrap()
                .get("c");
        assert_eq!(count, 0);
    }

    /// Spec 0057, §7: "verschlüsselt" — ein direkter SQL-Zugriff auf
    /// `content` (am `ContentCipher` vorbei) liefert keinen lesbaren
    /// Klartext, analog zu
    /// `chat_session_store::tests::test_direct_sql_access_to_content_column_never_reveals_plaintext`.
    #[tokio::test]
    async fn test_direct_sql_access_to_content_column_never_reveals_plaintext() {
        let (profile_store, ledger_store) = in_memory_ledger_store().await;
        let session_id = create_test_session(&profile_store).await;
        let secret_command = "mysql --password=hunter2geheim --host=db.internal.example";

        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::CommandProposed {
                    command: secret_command.to_string(),
                },
            )
            .await
            .unwrap();

        let raw_blob: Vec<u8> =
            sqlx::query("SELECT content FROM ledger_entries WHERE session_id = ?")
                .bind(session_id.to_string())
                .fetch_one(&profile_store.pool)
                .await
                .unwrap()
                .get("content");

        let raw_as_lossy_string = String::from_utf8_lossy(&raw_blob);
        assert!(
            !raw_as_lossy_string.contains("hunter2geheim"),
            "der rohe BLOB darf den Klartext nicht enthalten: {raw_as_lossy_string}"
        );
        assert!(
            !raw_as_lossy_string.contains("CommandProposed"),
            "der rohe BLOB darf nicht einmal als lesbares JSON erkennbar sein: {raw_as_lossy_string}"
        );
        assert!(
            raw_blob.len() > 12,
            "Blob muss mindestens den 12-Byte-Nonce enthalten"
        );
    }

    #[tokio::test]
    async fn test_load_entries_with_wrong_key_yields_clean_error_not_panic() {
        let (profile_store, ledger_store) = in_memory_ledger_store().await;
        let session_id = create_test_session(&profile_store).await;
        ledger_store
            .append_entry(
                session_id,
                LedgerSource::Ai,
                &LedgerEntryContent::AiMessage {
                    text: "geheim".to_string(),
                },
            )
            .await
            .unwrap();

        let wrong_key_cipher: Arc<dyn ContentCipher> = Arc::new(
            ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(&[99u8; 32]),
        );
        let store_with_wrong_key =
            SqliteLedgerStore::new(profile_store.pool.clone(), wrong_key_cipher);

        let result = store_with_wrong_key.load_entries(session_id).await;

        assert!(
            matches!(
                result,
                Err(LedgerStoreError::Cipher(CipherError::DecryptionFailed))
            ),
            "erwartete einen klaren Cipher-Fehler, kein Panic: {result:?}"
        );
    }
}
