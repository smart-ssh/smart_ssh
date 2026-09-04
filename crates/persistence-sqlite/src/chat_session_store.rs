//! SQLite-gestützte Persistenz für Chat-Sitzungen/-Nachrichten (Spec 0034,
//! Abschnitt 2/4).
//!
//! **Eigener Store statt Erweiterung von [`crate::SqliteProfileStore`]**,
//! aus demselben Grund wie [`crate::SqlitePromptHistoryStore`]/
//! [`crate::SqlitePolicyStore`] (s. dortige Modul-Kommentare): fachlich
//! unabhängig von Server-/Gruppen-Profilen, teilt sich aber denselben
//! `SqlitePool` (s. [`SqliteChatSessionStore::new`] und
//! `crate::store::SqliteProfileStore::chat_session_store`).
//!
//! **Teil 1** deckte die CRUD-Grundlagen ab: Sitzung anlegen, Nachricht
//! anhängen, Sitzung laden, als beendet/fortgesetzt markieren; Teil 2 die
//! übrigen Commands (`list_chat_sessions`/`rename_chat_session`/
//! `delete_chat_session`/Aufbewahrung).
//!
//! **Spec 0036 (Feld-Verschlüsselung)**: `content` wird vor dem Schreiben
//! über den mitgegebenen [`ContentCipher`] verschlüsselt und beim Lesen
//! entschlüsselt — transparent für jeden Aufrufer dieses Stores (Spec
//! 0036, Abschnitt 1: "aufrufender Code merkt davon nichts, arbeitet
//! weiterhin mit Klartext-`String`s"). Die Spalte selbst ist seit Migration
//! `0009` ein `BLOB` (`nonce || ciphertext`, s. `EncryptedContent::
//! to_blob`), vorher `TEXT` mit rohem JSON.

use std::sync::Arc;

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
use ssh_manager_core::crypto::{CipherError, ContentCipher, EncryptedContent};
use ssh_manager_core::shared::ServerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatSessionStoreError {
    Backend(String),
    /// Die geladene `content`-Spalte einer Nachricht ließ sich nicht als
    /// `MessageContent` deserialisieren — z. B. korrupte/fremde Daten.
    /// Eigene Variante statt in `Backend` verpackt, damit Aufrufer diesen
    /// Fall (Integritätsproblem, nicht bloß ein DB-Zugriffsfehler) gezielt
    /// von einem reinen Verbindungsfehler unterscheiden können (Spec 0034,
    /// Abschnitt 5, Punkt 1: "Integritätsprüfung, keine korrupten Daten").
    CorruptContent {
        message_id: String,
        reason: String,
    },
    /// Spec 0036: Ver-/Entschlüsselung ist fehlgeschlagen (korrupter Blob,
    /// falscher/fehlender Schlüssel, manipuliertes Chiffrat). Eigene
    /// Variante statt in `CorruptContent` verpackt — anders als dort ist
    /// die *Struktur* der Zeile (gültiges BLOB, korrekte Spalten) nicht
    /// das Problem, sondern der kryptografische Zugriff selbst.
    Cipher(CipherError),
}

impl std::fmt::Display for ChatSessionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatSessionStoreError::Backend(msg) => write!(f, "Datenbankfehler: {msg}"),
            ChatSessionStoreError::CorruptContent { message_id, reason } => write!(
                f,
                "Nachricht '{message_id}' konnte nicht gelesen werden: {reason}"
            ),
            ChatSessionStoreError::Cipher(err) => write!(f, "Verschlüsselungsfehler: {err}"),
        }
    }
}

impl std::error::Error for ChatSessionStoreError {}

impl From<CipherError> for ChatSessionStoreError {
    fn from(err: CipherError) -> Self {
        ChatSessionStoreError::Cipher(err)
    }
}

fn backend_err(e: sqlx::Error) -> ChatSessionStoreError {
    ChatSessionStoreError::Backend(e.to_string())
}

fn role_to_text(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::ActionResult => "action_result",
    }
}

/// Fail-safe auf `ActionResult` bei einem unbekannten Wert — dieselbe
/// Fail-safe-Haltung wie `mapping::post_ingest_policy_from_text`. In der
/// Praxis unerreichbar, solange die `CHECK`-Constraint aus der Migration
/// greift; nur als letzte Verteidigungslinie gegen eine künftig erweiterte
/// Spalte, die dieser Code-Stand noch nicht kennt.
fn role_from_text(raw: &str) -> Role {
    match raw {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => Role::ActionResult,
    }
}

fn content_type_for(content: &MessageContent) -> &'static str {
    match content {
        MessageContent::Text(_) => "text",
        MessageContent::CommandResult { .. } => "command_result",
        MessageContent::ActionRejected { .. } => "action_rejected",
    }
}

/// S. Moduldoc — teilt sich den Pool mit [`crate::SqliteProfileStore`].
/// `cipher`: `Arc`, nicht `Box`, da `Self` bereits `Clone` ist (billiger
/// `SqlitePool::clone()`, s. o.) — ein `Box<dyn ContentCipher>` wäre nicht
/// `Clone`.
#[derive(Clone)]
pub struct SqliteChatSessionStore {
    pool: SqlitePool,
    cipher: Arc<dyn ContentCipher>,
}

impl SqliteChatSessionStore {
    pub fn new(pool: SqlitePool, cipher: Arc<dyn ContentCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Spec 0034, Abschnitt 4: "Eine Sitzung beginnt bei `connect()`" —
    /// `started_at` wird hier gesetzt, `ended_at` bleibt `NULL` (aktiv),
    /// `title` bleibt `NULL` (Abschnitt 7: bis automatisch generiert).
    pub async fn create_session(
        &self,
        server_id: &ServerId,
        ai_provider_id: Option<Uuid>,
    ) -> Result<Uuid, ChatSessionStoreError> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO chat_sessions (id, server_id, title, started_at, ended_at, ai_provider_id) \
             VALUES (?, ?, NULL, ?, NULL, ?)",
        )
        .bind(id.to_string())
        .bind(server_id.0.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(ai_provider_id.map(|id| id.to_string()))
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        Ok(id)
    }

    /// Spec 0034, Abschnitt 4: "Jede Nachricht ... wird fortlaufend
    /// geschrieben, sobald sie entsteht — kein Sammeln bis zum
    /// Verbindungsende." `sequence` wird hier serverseitig als
    /// "ein höher als die bisher höchste Sequenznummer dieser Sitzung"
    /// bestimmt (0, falls die Sitzung noch keine Nachricht hat) — der
    /// Aufrufer muss selbst keine fortlaufende Zählung pflegen.
    ///
    /// `content` wird als JSON serialisiert (s. Moduldoc auf
    /// `MessageContent`s `Serialize`-Ableitung), dann über [`ContentCipher`]
    /// verschlüsselt (Spec 0036, Abschnitt 3: `nonce || ciphertext` als
    /// zusammenhängender Blob, s. `EncryptedContent::to_blob`) — bereits
    /// redigierter Inhalt wird hier vorausgesetzt, nicht selbst geprüft
    /// (Spec 0034, Abschnitt 3: das ist Aufgabe des Aufrufers/
    /// `OutputRedactor`, bevor diese Funktion überhaupt aufgerufen wird).
    pub async fn append_message(
        &self,
        session_id: Uuid,
        message: &ChatMessage,
    ) -> Result<Uuid, ChatSessionStoreError> {
        let session_id_str = session_id.to_string();

        let next_sequence: i64 = sqlx::query(
            "SELECT COALESCE(MAX(sequence), -1) + 1 AS next FROM chat_messages WHERE session_id = ?",
        )
        .bind(&session_id_str)
        .fetch_one(&self.pool)
        .await
        .map_err(backend_err)?
        .get("next");

        let content_json = serde_json::to_string(&message.content).map_err(|e| {
            ChatSessionStoreError::Backend(format!(
                "MessageContent-Serialisierung fehlgeschlagen: {e}"
            ))
        })?;
        let content_blob = self.cipher.encrypt(&content_json)?.to_blob();

        let message_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO chat_messages \
             (id, session_id, role, content_type, content, sequence, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id.to_string())
        .bind(&session_id_str)
        .bind(role_to_text(message.role))
        .bind(content_type_for(&message.content))
        .bind(content_blob)
        .bind(next_sequence)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        Ok(message_id)
    }

    /// Spec 0034, Abschnitt 8 (`resume_chat_session`): "lädt ... die
    /// gespeicherte Historie" — alle Nachrichten einer Sitzung, sortiert
    /// nach `sequence` (Migrations-Index `idx_chat_messages_session` deckt
    /// genau diese Sortierung ab). Entschlüsselt `content` transparent (Spec
    /// 0036) — der Aufrufer bekommt wie vor Spec 0036 nur `ChatMessage`s
    /// mit Klartext-`String`-Inhalten zu sehen.
    pub async fn load_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ChatMessage>, ChatSessionStoreError> {
        let rows = sqlx::query(
            "SELECT id, role, content FROM chat_messages \
             WHERE session_id = ? ORDER BY sequence ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend_err)?;

        rows.into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let role_raw: String = row.get("role");
                let content_blob: Vec<u8> = row.get("content");
                let encrypted = EncryptedContent::from_blob(&content_blob)?;
                let content_raw = self.cipher.decrypt(&encrypted)?;
                let content: MessageContent = serde_json::from_str(&content_raw).map_err(|e| {
                    ChatSessionStoreError::CorruptContent {
                        message_id: id.clone(),
                        reason: e.to_string(),
                    }
                })?;
                Ok(ChatMessage {
                    role: role_from_text(&role_raw),
                    content,
                })
            })
            .collect()
    }

    /// Spec 0034, Abschnitt 4: "endet bei `disconnect()`".
    pub async fn mark_ended(&self, session_id: Uuid) -> Result<(), ChatSessionStoreError> {
        sqlx::query("UPDATE chat_sessions SET ended_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;
        Ok(())
    }

    /// Spec 0034, Abschnitt 4: "Beim Fortsetzen ... wird dieselbe
    /// `chat_sessions`-Zeile weiterverwendet (`ended_at` wird auf `NULL`
    /// zurückgesetzt, `sequence` läuft weiter hoch)" — Letzteres ergibt
    /// sich automatisch aus [`Self::append_message`]s
    /// `MAX(sequence) + 1`-Logik, ohne einen eigenen Zähler
    /// zurücksetzen/fortführen zu müssen.
    pub async fn mark_resumed(&self, session_id: Uuid) -> Result<(), ChatSessionStoreError> {
        sqlx::query("UPDATE chat_sessions SET ended_at = NULL WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;
        Ok(())
    }

    /// Spec 0034, Abschnitt 8: `list_chat_sessions(server_id)`. Neueste
    /// zuerst (Abschnitt 6: "Liste vergangener Sitzungen darunter, neueste
    /// zuerst"), inklusive Nachrichtenanzahl (per Korrelations-Subquery,
    /// kein zweiter Roundtrip pro Sitzung).
    pub async fn list_sessions_for_server(
        &self,
        server_id: &ServerId,
    ) -> Result<Vec<ChatSessionSummary>, ChatSessionStoreError> {
        let rows = sqlx::query(
            "SELECT id, title, started_at, ended_at, \
                    (SELECT COUNT(*) FROM chat_messages WHERE session_id = chat_sessions.id) AS message_count \
             FROM chat_sessions WHERE server_id = ? ORDER BY started_at DESC",
        )
        .bind(server_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend_err)?;

        rows.into_iter()
            .map(|row| {
                let id_raw: String = row.get("id");
                let id = Uuid::parse_str(&id_raw).map_err(|e| {
                    ChatSessionStoreError::Backend(format!("ungültige UUID in Spalte id: {e}"))
                })?;
                let started_at_raw: String = row.get("started_at");
                let started_at = parse_timestamp(&started_at_raw)?;
                let ended_at = row
                    .get::<Option<String>, _>("ended_at")
                    .map(|raw| parse_timestamp(&raw))
                    .transpose()?;
                Ok(ChatSessionSummary {
                    id,
                    title: row.get("title"),
                    started_at,
                    ended_at,
                    message_count: row.get("message_count"),
                })
            })
            .collect()
    }

    /// Spec 0034, Abschnitt 8: `rename_chat_session(session_id, new_title)`
    /// — "überschreibt den automatischen Titel dauerhaft", also
    /// unbedingt, anders als [`Self::set_title_if_absent`].
    pub async fn rename_session(
        &self,
        session_id: Uuid,
        new_title: &str,
    ) -> Result<(), ChatSessionStoreError> {
        sqlx::query("UPDATE chat_sessions SET title = ? WHERE id = ?")
            .bind(new_title)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;
        Ok(())
    }

    /// Spec 0034, Abschnitt 7: automatische Titel-Generierung — "Ein einmal
    /// automatisch gesetzter Titel wird nicht ... erneut überschrieben".
    /// Das `WHERE title IS NULL` macht das atomar in der DB statt als
    /// Read-then-write im Aufrufer (sonst ein theoretisches Race, falls je
    /// zwei gleichzeitige Trigger für dieselbe Sitzung liefen). Gibt
    /// zurück, ob tatsächlich gesetzt wurde (`false`, wenn bereits ein
    /// Titel bestand) — nicht, dass ein Aufrufer das aktuell auswertet,
    /// aber ehrlicher als stillschweigend zu verschlucken, ob der
    /// Aufruf etwas bewirkt hat.
    pub async fn set_title_if_absent(
        &self,
        session_id: Uuid,
        title: &str,
    ) -> Result<bool, ChatSessionStoreError> {
        let result =
            sqlx::query("UPDATE chat_sessions SET title = ? WHERE id = ? AND title IS NULL")
                .bind(title)
                .bind(session_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(backend_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Spec 0034, Abschnitt 8: `delete_chat_session(session_id)` —
    /// zugehörige Nachrichten verschwinden automatisch über
    /// `ON DELETE CASCADE` (Migration).
    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), ChatSessionStoreError> {
        sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;
        Ok(())
    }

    /// Spec 0034, Abschnitt 5: Aufbewahrungs-Job — löscht beendete
    /// Sitzungen, deren `ended_at` vor `cutoff` liegt. Noch aktive
    /// Sitzungen (`ended_at IS NULL`) werden nie gelöscht, unabhängig vom
    /// Cutoff — eine laufende Sitzung hat kein "Alter" im Sinne dieser
    /// Einstellung. Gibt die Anzahl gelöschter Sitzungen zurück (fürs
    /// Log, s. Aufrufer).
    pub async fn delete_ended_sessions_before(
        &self,
        cutoff: chrono::DateTime<Utc>,
    ) -> Result<u64, ChatSessionStoreError> {
        let result =
            sqlx::query("DELETE FROM chat_sessions WHERE ended_at IS NOT NULL AND ended_at < ?")
                .bind(cutoff.to_rfc3339())
                .execute(&self.pool)
                .await
                .map_err(backend_err)?;
        Ok(result.rows_affected())
    }
}

/// Spec 0034, Abschnitt 8 (`list_chat_sessions`): Titel, Zeitpunkt,
/// Nachrichtenanzahl je vergangener Sitzung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSessionSummary {
    pub id: Uuid,
    pub title: Option<String>,
    pub started_at: chrono::DateTime<Utc>,
    pub ended_at: Option<chrono::DateTime<Utc>>,
    pub message_count: i64,
}

fn parse_timestamp(raw: &str) -> Result<chrono::DateTime<Utc>, ChatSessionStoreError> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| ChatSessionStoreError::Backend(format!("ungültiger Zeitstempel: {e}")))
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;
    use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
    use ssh_manager_core::ssh::CommandOutput;

    use crate::SqliteProfileStore;

    fn test_cipher() -> Arc<dyn ContentCipher> {
        Arc::new(ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(
            &[42u8; 32],
        ))
    }

    async fn in_memory_chat_session_store() -> (SqliteProfileStore, SqliteChatSessionStore) {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let profile_store = SqliteProfileStore::connect_with(options)
            .await
            .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein");
        let chat_store = profile_store.chat_session_store(test_cipher());
        (profile_store, chat_store)
    }

    /// `chat_sessions.server_id` referenziert `servers(id)` — für die Tests
    /// deshalb ein echter Server statt einer beliebigen UUID (sonst würde
    /// die Fremdschlüssel-Einschränkung jeden `create_session()`-Aufruf
    /// ablehnen), analog zu `prompt_history_store::tests::create_test_server`.
    async fn create_test_server(profile_store: &SqliteProfileStore) -> ServerId {
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
            created_at: now,
            updated_at: now,
        };
        let id = server.id;
        profile_store.create_server(&server).await.unwrap();
        id
    }

    fn text_message(role: Role, text: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: MessageContent::Text(text.to_string()),
        }
    }

    #[tokio::test]
    async fn test_messages_persist_in_correct_order() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        chat_store
            .append_message(session_id, &text_message(Role::User, "erstens"))
            .await
            .unwrap();
        chat_store
            .append_message(session_id, &text_message(Role::Assistant, "zweitens"))
            .await
            .unwrap();
        chat_store
            .append_message(session_id, &text_message(Role::User, "drittens"))
            .await
            .unwrap();

        let loaded = chat_store.load_session(session_id).await.unwrap();
        let texts: Vec<&str> = loaded
            .iter()
            .map(|m| match &m.content {
                MessageContent::Text(t) => t.as_str(),
                _ => panic!("erwartete Text-Nachricht"),
            })
            .collect();
        assert_eq!(texts, vec!["erstens", "zweitens", "drittens"]);
    }

    /// Spec 0034, Abschnitt 5, Punkt 1: "sich die gespeicherte Historie
    /// laden lässt" — hier verifiziert als exakter Round-trip über alle
    /// `MessageContent`-Varianten hinweg, nicht nur `Text`.
    #[tokio::test]
    async fn test_resume_loads_exact_persisted_history() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        let messages = vec![
            text_message(Role::User, "zeig mir die laufenden Prozesse"),
            ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::CommandResult {
                    command: "ps aux".to_string(),
                    output: CommandOutput {
                        stdout: b"PID TTY\n1 ?\n".to_vec(),
                        stderr: Vec::new(),
                        exit_code: Some(0),
                    },
                    cancelled: false,
                },
            },
            ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::ActionRejected {
                    command: "rm -rf /".to_string(),
                    reason: ssh_manager_core::ai::RejectionReason::User,
                },
            },
        ];
        for message in &messages {
            chat_store
                .append_message(session_id, message)
                .await
                .unwrap();
        }

        let loaded = chat_store.load_session(session_id).await.unwrap();
        assert_eq!(loaded, messages);
    }

    /// Spec 0034, Abschnitt 4, letzter Punkt: "`sequence` läuft weiter
    /// hoch" beim Fortsetzen — kein Zurücksetzen, keine Lücke/Duplikat.
    #[tokio::test]
    async fn test_mark_resumed_then_appended_messages_continue_sequence() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        chat_store
            .append_message(session_id, &text_message(Role::User, "vor dem Trennen"))
            .await
            .unwrap();
        chat_store.mark_ended(session_id).await.unwrap();
        chat_store.mark_resumed(session_id).await.unwrap();
        chat_store
            .append_message(session_id, &text_message(Role::User, "nach dem Fortsetzen"))
            .await
            .unwrap();

        let loaded = chat_store.load_session(session_id).await.unwrap();
        assert_eq!(loaded.len(), 2);
        let texts: Vec<&str> = loaded
            .iter()
            .map(|m| match &m.content {
                MessageContent::Text(t) => t.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(texts, vec!["vor dem Trennen", "nach dem Fortsetzen"]);
    }

    #[tokio::test]
    async fn test_mark_ended_sets_ended_at() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        chat_store.mark_ended(session_id).await.unwrap();

        let ended_at: Option<String> =
            sqlx::query("SELECT ended_at FROM chat_sessions WHERE id = ?")
                .bind(session_id.to_string())
                .fetch_one(&profile_store.pool)
                .await
                .unwrap()
                .get("ended_at");
        assert!(ended_at.is_some());
    }

    #[tokio::test]
    async fn test_mark_resumed_resets_ended_at_to_null() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();
        chat_store.mark_ended(session_id).await.unwrap();

        chat_store.mark_resumed(session_id).await.unwrap();

        let ended_at: Option<String> =
            sqlx::query("SELECT ended_at FROM chat_sessions WHERE id = ?")
                .bind(session_id.to_string())
                .fetch_one(&profile_store.pool)
                .await
                .unwrap()
                .get("ended_at");
        assert!(ended_at.is_none());
    }

    /// Spec 0034, Abschnitt 3: "Der `content`-Wert entspricht exakt dem,
    /// was tatsächlich durch den `OutputRedactor` gelaufen ist" — dieser
    /// Test verifiziert nur, dass der Store selbst nichts zusätzlich
    /// verändert (reiner Passthrough); die eigentliche Redaction ist
    /// Aufgabe der Kernschleife (Teil 2), nicht dieses Stores.
    #[tokio::test]
    async fn test_store_persists_content_unmodified() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        let already_redacted = "API-Key: [REDIGIERT]";
        chat_store
            .append_message(session_id, &text_message(Role::User, already_redacted))
            .await
            .unwrap();

        let loaded = chat_store.load_session(session_id).await.unwrap();
        assert_eq!(
            loaded[0].content,
            MessageContent::Text(already_redacted.to_string())
        );
    }

    #[tokio::test]
    async fn test_deleting_session_cascades_to_its_messages() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();
        chat_store
            .append_message(session_id, &text_message(Role::User, "wird verwaist"))
            .await
            .unwrap();

        sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&profile_store.pool)
            .await
            .unwrap();

        let count: i64 =
            sqlx::query("SELECT COUNT(*) AS c FROM chat_messages WHERE session_id = ?")
                .bind(session_id.to_string())
                .fetch_one(&profile_store.pool)
                .await
                .unwrap()
                .get("c");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_deleting_server_cascades_to_its_chat_sessions() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        profile_store.delete_server(&server_id).await.unwrap();

        let count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM chat_sessions WHERE id = ?")
            .bind(session_id.to_string())
            .fetch_one(&profile_store.pool)
            .await
            .unwrap()
            .get("c");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_list_sessions_for_server_orders_newest_first_with_message_counts() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let first = chat_store.create_session(&server_id, None).await.unwrap();
        // Reihenfolge über `started_at` erzwingen (sonst könnten beide
        // Sitzungen denselben Zeitstempel bekommen, wenn die Testmaschine
        // sehr schnell ist) — direktes SQL-Update statt `sleep`, um den
        // Test schnell und deterministisch zu halten.
        sqlx::query("UPDATE chat_sessions SET started_at = ? WHERE id = ?")
            .bind("2020-01-01T00:00:00Z")
            .bind(first.to_string())
            .execute(&profile_store.pool)
            .await
            .unwrap();
        chat_store
            .append_message(first, &text_message(Role::User, "eins"))
            .await
            .unwrap();

        let second = chat_store.create_session(&server_id, None).await.unwrap();
        sqlx::query("UPDATE chat_sessions SET started_at = ? WHERE id = ?")
            .bind("2020-06-01T00:00:00Z")
            .bind(second.to_string())
            .execute(&profile_store.pool)
            .await
            .unwrap();
        chat_store
            .append_message(second, &text_message(Role::User, "eins"))
            .await
            .unwrap();
        chat_store
            .append_message(second, &text_message(Role::Assistant, "zwei"))
            .await
            .unwrap();

        let listed = chat_store
            .list_sessions_for_server(&server_id)
            .await
            .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, second, "neueste zuerst");
        assert_eq!(listed[0].message_count, 2);
        assert_eq!(listed[1].id, first);
        assert_eq!(listed[1].message_count, 1);
    }

    #[tokio::test]
    async fn test_rename_session_always_overwrites() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();
        chat_store
            .set_title_if_absent(session_id, "Automatischer Titel")
            .await
            .unwrap();

        chat_store
            .rename_session(session_id, "Manuell umbenannt")
            .await
            .unwrap();

        let listed = chat_store
            .list_sessions_for_server(&server_id)
            .await
            .unwrap();
        assert_eq!(listed[0].title.as_deref(), Some("Manuell umbenannt"));
    }

    /// Spec 0034, Abschnitt 7: "Ein einmal automatisch gesetzter Titel wird
    /// nicht ... erneut überschrieben."
    #[tokio::test]
    async fn test_set_title_if_absent_does_not_overwrite_existing_title() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        let first_set = chat_store
            .set_title_if_absent(session_id, "Erster Titel")
            .await
            .unwrap();
        let second_set = chat_store
            .set_title_if_absent(session_id, "Zweiter Titel")
            .await
            .unwrap();

        assert!(first_set);
        assert!(!second_set);
        let listed = chat_store
            .list_sessions_for_server(&server_id)
            .await
            .unwrap();
        assert_eq!(listed[0].title.as_deref(), Some("Erster Titel"));
    }

    #[tokio::test]
    async fn test_delete_session_removes_it_from_listing() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();

        chat_store.delete_session(session_id).await.unwrap();

        assert!(chat_store
            .list_sessions_for_server(&server_id)
            .await
            .unwrap()
            .is_empty());
    }

    /// Spec 0034, Abschnitt 5: nur BEENDETE Sitzungen älter als der
    /// Cutoff werden gelöscht — eine noch aktive (`ended_at IS NULL`)
    /// Sitzung nie, unabhängig davon, wie alt `started_at` ist.
    #[tokio::test]
    async fn test_delete_ended_sessions_before_cutoff_spares_active_and_recent_sessions() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;

        let old_ended = chat_store.create_session(&server_id, None).await.unwrap();
        chat_store.mark_ended(old_ended).await.unwrap();
        sqlx::query("UPDATE chat_sessions SET ended_at = ? WHERE id = ?")
            .bind("2020-01-01T00:00:00Z")
            .bind(old_ended.to_string())
            .execute(&profile_store.pool)
            .await
            .unwrap();

        let still_active = chat_store.create_session(&server_id, None).await.unwrap();
        sqlx::query("UPDATE chat_sessions SET started_at = ? WHERE id = ?")
            .bind("2020-01-01T00:00:00Z")
            .bind(still_active.to_string())
            .execute(&profile_store.pool)
            .await
            .unwrap();

        let recently_ended = chat_store.create_session(&server_id, None).await.unwrap();
        chat_store.mark_ended(recently_ended).await.unwrap();

        let cutoff = chrono::DateTime::parse_from_rfc3339("2021-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let deleted_count = chat_store
            .delete_ended_sessions_before(cutoff)
            .await
            .unwrap();

        assert_eq!(deleted_count, 1);
        let remaining_ids: Vec<Uuid> = chat_store
            .list_sessions_for_server(&server_id)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert!(!remaining_ids.contains(&old_ended));
        assert!(remaining_ids.contains(&still_active));
        assert!(remaining_ids.contains(&recently_ended));
    }

    // --- Spec 0036: Feld-Verschlüsselung ------------------------------------

    /// Aufgabenstellung: "ein direkter SQL-Zugriff auf `chat_messages.
    /// content` (am `ContentCipher` vorbei, wie es ein Angreifer mit
    /// Dateizugriff tun würde) liefert nachweislich keinen lesbaren
    /// Klartext." Liest die rohe BLOB-Spalte über eine eigene Query
    /// (nicht `load_session`, das entschlüsselt) und prüft, dass weder der
    /// Nachrichtentext noch sein JSON-Feldname (`"Text"`, aus `serde`s
    /// externally-tagged Repräsentation von `MessageContent`) irgendwo
    /// darin vorkommt — ein unverschlüsseltes JSON-Encoding hätte beides
    /// im Klartext enthalten.
    #[tokio::test]
    async fn test_direct_sql_access_to_content_column_never_reveals_plaintext() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();
        let secret_text = "streng geheime Diagnoseinformationen, server42.internal";

        chat_store
            .append_message(session_id, &text_message(Role::User, secret_text))
            .await
            .unwrap();

        let raw_blob: Vec<u8> =
            sqlx::query("SELECT content FROM chat_messages WHERE session_id = ?")
                .bind(session_id.to_string())
                .fetch_one(&profile_store.pool)
                .await
                .unwrap()
                .get("content");

        let raw_as_lossy_string = String::from_utf8_lossy(&raw_blob);
        assert!(
            !raw_as_lossy_string.contains(secret_text),
            "der rohe BLOB darf den Klartext nicht enthalten: {raw_as_lossy_string}"
        );
        assert!(
            !raw_as_lossy_string.contains("Text"),
            "der rohe BLOB darf nicht einmal als lesbares JSON erkennbar sein: {raw_as_lossy_string}"
        );
        // Der Blob muss trotzdem strukturell wie erwartet aussehen (Nonce +
        // etwas Chiffrat, s. `EncryptedContent::to_blob`), kein leerer/
        // kaputter Wert.
        assert!(
            raw_blob.len() > 12,
            "Blob muss mindestens den 12-Byte-Nonce enthalten"
        );
    }

    /// Aufgabenstellung: "fehlender/korrupter Schlüssel führt zu einem
    /// klaren Fehler, nicht zu einem Panic" — hier über einen Store mit
    /// einem ANDEREN Schlüssel als dem, mit dem die Zeile ursprünglich
    /// geschrieben wurde (derselbe Effekt wie ein tatsächlich korrupter/
    /// verlorener Schlüssel: Entschlüsselung kann nicht mehr gelingen).
    #[tokio::test]
    async fn test_load_session_with_wrong_key_yields_clean_error_not_panic() {
        let (profile_store, chat_store) = in_memory_chat_session_store().await;
        let server_id = create_test_server(&profile_store).await;
        let session_id = chat_store.create_session(&server_id, None).await.unwrap();
        chat_store
            .append_message(session_id, &text_message(Role::User, "geheim"))
            .await
            .unwrap();

        let wrong_key_cipher: Arc<dyn ContentCipher> = Arc::new(
            ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(&[99u8; 32]),
        );
        let store_with_wrong_key =
            SqliteChatSessionStore::new(profile_store.pool.clone(), wrong_key_cipher);

        let result = store_with_wrong_key.load_session(session_id).await;

        assert!(
            matches!(
                result,
                Err(ChatSessionStoreError::Cipher(CipherError::DecryptionFailed))
            ),
            "erwartete einen klaren Cipher-Fehler, kein Panic: {result:?}"
        );
    }
}
