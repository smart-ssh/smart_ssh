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
//! **Teil 1 (dieser Schritt)** deckt nur die CRUD-Grundlagen ab: Sitzung
//! anlegen, Nachricht anhängen, Sitzung laden, als beendet/fortgesetzt
//! markieren. `list_chat_sessions`/`rename_chat_session`/
//! `delete_chat_session` (Spec 0034, Abschnitt 8) sind bewusst **nicht**
//! Teil dieses Schritts — folgen in Teil 2, zusammen mit der
//! Kernschleifen-Anbindung, die sie tatsächlich braucht.

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
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
}

impl std::fmt::Display for ChatSessionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatSessionStoreError::Backend(msg) => write!(f, "Datenbankfehler: {msg}"),
            ChatSessionStoreError::CorruptContent { message_id, reason } => write!(
                f,
                "Nachricht '{message_id}' konnte nicht gelesen werden: {reason}"
            ),
        }
    }
}

impl std::error::Error for ChatSessionStoreError {}

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
#[derive(Clone)]
pub struct SqliteChatSessionStore {
    pool: SqlitePool,
}

impl SqliteChatSessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
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
    /// `MessageContent`s `Serialize`-Ableitung) — bereits redigierter
    /// Inhalt wird hier vorausgesetzt, nicht selbst geprüft (Spec 0034,
    /// Abschnitt 3: das ist Aufgabe des Aufrufers/`OutputRedactor`, bevor
    /// diese Funktion überhaupt aufgerufen wird).
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
        .bind(content_json)
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
    /// genau diese Sortierung ab).
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
                let content_raw: String = row.get("content");
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
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;
    use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
    use ssh_manager_core::ssh::CommandOutput;

    use crate::SqliteProfileStore;

    async fn in_memory_chat_session_store() -> (SqliteProfileStore, SqliteChatSessionStore) {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let profile_store = SqliteProfileStore::connect_with(options)
            .await
            .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein");
        let chat_store = profile_store.chat_session_store();
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
}
