//! SQLite-gestützte Speicherung der Chat-Prompt-Historie pro Server (Spec
//! 0015).
//!
//! **Eigener Store statt Erweiterung von [`crate::SqliteProfileStore`]**,
//! aus demselben Grund wie [`crate::SqlitePolicyStore`]/
//! [`crate::SqliteAiProviderStore`] (s. dortige Modul-Kommentare):
//! Prompt-Historie ist fachlich unabhängig von Server-/Gruppen-Profilen,
//! teilt sich aber denselben `SqlitePool` (s. [`SqlitePromptHistoryStore::new`]
//! und `crate::store::SqliteProfileStore::prompt_history_store`).
//!
//! Anders als `StoredRule`/`AiProviderConfig` braucht kein Aufrufer je die
//! `id`/`created_at`-Spalten einzeln — Spec 0015 Abschnitt 4 verlangt nur
//! `Vec<String>` (reine Prompt-Texte, chronologisch). Es gibt deshalb keinen
//! eigenen `PromptHistoryEntry`-Domänentyp, nur die beiden Operationen
//! [`SqlitePromptHistoryStore::record`]/[`SqlitePromptHistoryStore::list`].

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use uuid::Uuid;

use ssh_manager_core::shared::ServerId;

/// Spec 0015, Abschnitt 3: "maximal die letzten 200 Einträge".
const MAX_ENTRIES_PER_SERVER: i64 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptHistoryStoreError {
    Backend(String),
}

impl std::fmt::Display for PromptHistoryStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptHistoryStoreError::Backend(msg) => write!(f, "Datenbankfehler: {msg}"),
        }
    }
}

impl std::error::Error for PromptHistoryStoreError {}

fn backend_err(e: sqlx::Error) -> PromptHistoryStoreError {
    PromptHistoryStoreError::Backend(e.to_string())
}

/// S. Moduldoc — teilt sich den Pool mit [`crate::SqliteProfileStore`],
/// deshalb `Clone` billig (nur ein `SqlitePool::clone()`, kein zweiter
/// Verbindungsaufbau), analog zu [`crate::SqlitePolicyStore`].
#[derive(Clone)]
pub struct SqlitePromptHistoryStore {
    pool: SqlitePool,
}

impl SqlitePromptHistoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Spec 0015, Abschnitt 3: speichert `content` für `server_id`. Ist der
    /// bislang jüngste Eintrag für denselben Server exakt identisch mit
    /// `content`, wird **kein** zweiter Eintrag angelegt — stattdessen wird
    /// dessen `created_at` auf jetzt aktualisiert, sodass er bei
    /// aufeinanderfolgend wiederholten Prompts weiterhin als "jüngster"
    /// gilt, ohne die Historie mit Duplikaten aufzublähen. Danach wird die
    /// 200-Einträge-Grenze durchgesetzt: überzählige, älteste Einträge für
    /// diesen Server werden gelöscht.
    pub async fn record(
        &self,
        server_id: &ServerId,
        content: &str,
    ) -> Result<(), PromptHistoryStoreError> {
        let server_id_str = server_id.0.to_string();
        let now = Utc::now().to_rfc3339();

        let latest = sqlx::query(
            "SELECT id, content FROM prompt_history WHERE server_id = ? \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&server_id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend_err)?;

        if let Some(row) = &latest {
            let existing_content: String = row.get("content");
            if existing_content == content {
                let existing_id: String = row.get("id");
                sqlx::query("UPDATE prompt_history SET created_at = ? WHERE id = ?")
                    .bind(&now)
                    .bind(&existing_id)
                    .execute(&self.pool)
                    .await
                    .map_err(backend_err)?;
                return Ok(());
            }
        }

        sqlx::query(
            "INSERT INTO prompt_history (id, server_id, content, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&server_id_str)
        .bind(content)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        sqlx::query(
            "DELETE FROM prompt_history WHERE server_id = ? AND id NOT IN ( \
                 SELECT id FROM prompt_history WHERE server_id = ? \
                 ORDER BY created_at DESC LIMIT ?)",
        )
        .bind(&server_id_str)
        .bind(&server_id_str)
        .bind(MAX_ENTRIES_PER_SERVER)
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        Ok(())
    }

    /// Spec 0015, Abschnitt 4: chronologisch aufsteigend (älteste zuerst) —
    /// das Frontend kehrt für die Navigation selbst um bzw. greift von
    /// hinten zu.
    pub async fn list(&self, server_id: &ServerId) -> Result<Vec<String>, PromptHistoryStoreError> {
        let rows = sqlx::query(
            "SELECT content FROM prompt_history WHERE server_id = ? ORDER BY created_at ASC",
        )
        .bind(server_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend_err)?;

        Ok(rows.into_iter().map(|row| row.get("content")).collect())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;
    use chrono::Utc;

    use crate::SqliteProfileStore;
    use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};

    async fn in_memory_prompt_history_store() -> (SqliteProfileStore, SqlitePromptHistoryStore) {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let profile_store = SqliteProfileStore::connect_with(options)
            .await
            .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein");
        let history_store = profile_store.prompt_history_store();
        (profile_store, history_store)
    }

    /// `prompt_history.server_id` referenziert `servers(id)` — für die
    /// Tests wird deshalb ein echter Server angelegt, statt nur eine
    /// beliebige UUID zu verwenden (sonst würde die Fremdschlüssel-
    /// Einschränkung, s. Migration, jeden `record()`-Aufruf ablehnen).
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

    #[tokio::test]
    async fn test_record_then_list_roundtrip() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_id = create_test_server(&profile_store).await;

        history_store.record(&server_id, "ls -la").await.unwrap();

        assert_eq!(
            history_store.list(&server_id).await.unwrap(),
            vec!["ls -la".to_string()]
        );
    }

    #[tokio::test]
    async fn test_list_is_chronologically_ascending() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_id = create_test_server(&profile_store).await;

        history_store.record(&server_id, "erstens").await.unwrap();
        history_store.record(&server_id, "zweitens").await.unwrap();
        history_store.record(&server_id, "drittens").await.unwrap();

        assert_eq!(
            history_store.list(&server_id).await.unwrap(),
            vec![
                "erstens".to_string(),
                "zweitens".to_string(),
                "drittens".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_consecutive_identical_prompts_do_not_duplicate() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_id = create_test_server(&profile_store).await;

        history_store.record(&server_id, "status").await.unwrap();
        history_store.record(&server_id, "status").await.unwrap();
        history_store.record(&server_id, "status").await.unwrap();

        assert_eq!(
            history_store.list(&server_id).await.unwrap(),
            vec!["status".to_string()]
        );
    }

    /// Nicht-aufeinanderfolgend identische Prompts (dazwischen ein anderer
    /// Prompt) sind laut Spec **kein** Dedupe-Fall — beide Vorkommen bleiben
    /// als eigene Einträge erhalten.
    #[tokio::test]
    async fn test_non_consecutive_identical_prompts_are_both_kept() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_id = create_test_server(&profile_store).await;

        history_store.record(&server_id, "status").await.unwrap();
        history_store
            .record(&server_id, "andere Sache")
            .await
            .unwrap();
        history_store.record(&server_id, "status").await.unwrap();

        assert_eq!(
            history_store.list(&server_id).await.unwrap(),
            vec![
                "status".to_string(),
                "andere Sache".to_string(),
                "status".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_more_than_200_entries_drops_oldest() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_id = create_test_server(&profile_store).await;

        for i in 0..201 {
            history_store
                .record(&server_id, &format!("prompt-{i}"))
                .await
                .unwrap();
        }

        let listed = history_store.list(&server_id).await.unwrap();
        assert_eq!(listed.len(), 200);
        assert_eq!(listed.first(), Some(&"prompt-1".to_string()));
        assert_eq!(listed.last(), Some(&"prompt-200".to_string()));
    }

    #[tokio::test]
    async fn test_list_only_returns_entries_of_requested_server() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_a = create_test_server(&profile_store).await;
        let server_b = create_test_server(&profile_store).await;

        history_store.record(&server_a, "für a").await.unwrap();
        history_store.record(&server_b, "für b").await.unwrap();

        assert_eq!(
            history_store.list(&server_a).await.unwrap(),
            vec!["für a".to_string()]
        );
        assert_eq!(
            history_store.list(&server_b).await.unwrap(),
            vec!["für b".to_string()]
        );
    }

    #[tokio::test]
    async fn test_deleting_server_cascades_to_its_prompt_history() {
        let (profile_store, history_store) = in_memory_prompt_history_store().await;
        let server_id = create_test_server(&profile_store).await;
        history_store
            .record(&server_id, "wird verwaist")
            .await
            .unwrap();

        profile_store.delete_server(&server_id).await.unwrap();

        assert!(history_store.list(&server_id).await.unwrap().is_empty());
    }
}
