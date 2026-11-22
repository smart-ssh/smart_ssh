//! Recorded sessions of the activity record (Spec 0057, issue #1443).
//!
//! A recorded session (`activity_sessions`) exists independently of a chat:
//! deleting a chat or a server never deletes it. It holds a snapshot of the
//! server (name, host, port, user, group path, tags), so the record stays
//! readable after the server is gone.

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use ssh_manager_core::shared::ServerId;

use crate::ledger_store::LedgerStoreError;

/// Kind of a recorded session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySessionKind {
    Chat,
    Mcp,
    Terminal,
}

impl ActivitySessionKind {
    fn as_text(self) -> &'static str {
        match self {
            ActivitySessionKind::Chat => "chat",
            ActivitySessionKind::Mcp => "mcp",
            ActivitySessionKind::Terminal => "terminal",
        }
    }
}

/// A recorded session as read from the database.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivitySession {
    pub id: Uuid,
    pub kind: String,
    pub mcp_client: Option<String>,
    pub server_id: String,
    pub server_name: String,
    pub server_host: String,
    pub server_port: u16,
    pub server_user: String,
    /// Group ids, root group first.
    pub group_ids: Vec<String>,
    pub group_names: Vec<String>,
    pub tags: Vec<String>,
    pub time_zone: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub failed_writes: i64,
}

/// Source of the device's IANA time zone name; `None` when unknown.
pub type TimeZoneSource = std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Default source: the platform's zone; a failed or empty lookup is `None`.
pub fn device_time_zone_source() -> TimeZoneSource {
    std::sync::Arc::new(|| {
        iana_time_zone::get_timezone()
            .ok()
            .filter(|z| !z.trim().is_empty())
    })
}

/// Creates the recorded session for `server_id` inside `tx`, with the
/// snapshot taken from the current server, group path and tags. Returns
/// `false` (no row written) when the server does not exist.
pub(crate) async fn insert_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    kind: ActivitySessionKind,
    mcp_client: Option<&str>,
    server_id: &ServerId,
    time_zone: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "WITH RECURSIVE path(gid, name, parent_id, depth, seen) AS ( \
             SELECT g.id, g.name, g.parent_id, 0, '|' || g.id || '|' FROM servers s \
                 JOIN groups g ON g.id = s.group_id WHERE s.id = ?1 \
             UNION ALL \
             SELECT g.id, g.name, g.parent_id, p.depth + 1, p.seen || g.id || '|' FROM path p \
                 JOIN groups g ON g.id = p.parent_id \
                 WHERE p.depth < 64 AND instr(p.seen, '|' || g.id || '|') = 0 \
         ) \
         INSERT INTO activity_sessions ( \
             id, kind, mcp_client, server_id, server_name, server_host, server_port, \
             server_user, group_ids, group_names, tags, time_zone, started_at, \
             last_activity_at, failed_writes) \
         SELECT ?2, ?3, ?4, s.id, s.name, s.host, s.port, s.username, \
             COALESCE((SELECT json_group_array(gid) FROM \
                 (SELECT gid FROM path ORDER BY depth DESC)), '[]'), \
             COALESCE((SELECT json_group_array(name) FROM \
                 (SELECT name FROM path ORDER BY depth DESC)), '[]'), \
             COALESCE((SELECT json_group_array(tag) FROM \
                 (SELECT tag FROM server_tags WHERE server_id = s.id ORDER BY tag)), '[]'), \
             ?5, ?6, NULL, 0 \
         FROM servers s WHERE s.id = ?1",
    )
    .bind(server_id.0.to_string())
    .bind(id.to_string())
    .bind(kind.as_text())
    .bind(mcp_client)
    .bind(time_zone)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Creates a chat session and its recorded session in one transaction and
/// links them (Spec 0057). The recorded session reuses the chat session id.
/// A missing server writes no recorded session; the `chat_sessions` insert
/// then fails on its foreign key and the transaction is rolled back.
pub(crate) async fn create_chat_session(
    pool: &SqlitePool,
    id: Uuid,
    server_id: &ServerId,
    ai_provider_id: Option<Uuid>,
    time_zone: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let _server_exists = insert_snapshot(
        &mut tx,
        id,
        ActivitySessionKind::Chat,
        None,
        server_id,
        time_zone,
    )
    .await?;
    sqlx::query(
        "INSERT INTO chat_sessions \
         (id, server_id, title, started_at, ended_at, ai_provider_id, activity_session_id) \
         VALUES (?1, ?2, NULL, ?3, NULL, ?4, ?1)",
    )
    .bind(id.to_string())
    .bind(server_id.0.to_string())
    .bind(Utc::now().to_rfc3339())
    .bind(ai_provider_id.map(|id| id.to_string()))
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Sets (`Some`) or clears (`None`) the end of a chat session and of its
/// recorded session in one transaction (issue #1447).
pub(crate) async fn set_chat_session_ended(
    pool: &SqlitePool,
    session_id: Uuid,
    ended_at: Option<chrono::DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    let ended = ended_at.map(|t| t.to_rfc3339());
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE chat_sessions SET ended_at = ?1 WHERE id = ?2")
        .bind(&ended)
        .bind(session_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE activity_sessions SET ended_at = ?1 \
         WHERE id = (SELECT activity_session_id FROM chat_sessions WHERE id = ?2)",
    )
    .bind(&ended)
    .bind(session_id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Ends a chat session that has no messages and no content entries (only
/// `connection` / `activity_tick` entries do not count as content). Its
/// `chat_sessions` row is deleted. The recorded session is kept as a
/// `terminal` session when it has ledger entries (issue #1447: a
/// terminal-only connection stays in the record), else it is deleted as
/// before. Check and delete are one write transaction. Returns whether the
/// chat session was deleted.
pub(crate) async fn delete_chat_session_if_empty(
    pool: &SqlitePool,
    session_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let res = sqlx::query(
        "DELETE FROM chat_sessions WHERE id = ? \
         AND NOT EXISTS (SELECT 1 FROM chat_messages WHERE session_id = chat_sessions.id) \
         AND NOT EXISTS (SELECT 1 FROM ledger_entries \
             WHERE session_id = COALESCE(chat_sessions.activity_session_id, chat_sessions.id) \
             AND entry_type NOT IN ('connection', 'activity_tick'))",
    )
    .bind(session_id.to_string())
    .execute(&mut *tx)
    .await?;
    let deleted = res.rows_affected() > 0;
    if deleted {
        sqlx::query(
            "DELETE FROM activity_sessions WHERE id = ?1 \
             AND NOT EXISTS (SELECT 1 FROM ledger_entries WHERE session_id = ?1)",
        )
        .bind(session_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE activity_sessions SET kind = 'terminal', ended_at = ?2 WHERE id = ?1")
            .bind(session_id.to_string())
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(deleted)
}

/// Closes every recorded session left open by a previous run (crash, killed
/// process): `ended_at = last_activity_at`, or `started_at` without any
/// activity. The linked chat sessions get the same end. Only sessions that
/// started before `before` are touched, so a session opened by this run while
/// the cleanup is still pending stays open. Returns how many recorded
/// sessions were closed.
pub(crate) async fn close_open_sessions(
    pool: &SqlitePool,
    before: chrono::DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let before = before.to_rfc3339();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    // A chat session nobody wrote to is a terminal-only session: it never
    // shows up in the resume dialog (same rule as `delete_chat_session_if_empty`).
    sqlx::query(
        "UPDATE activity_sessions SET kind = 'terminal' \
         WHERE kind = 'chat' AND ended_at IS NULL AND started_at < ?1 \
         AND NOT EXISTS (SELECT 1 FROM chat_messages m JOIN chat_sessions c ON c.id = m.session_id \
             WHERE c.activity_session_id = activity_sessions.id) \
         AND NOT EXISTS (SELECT 1 FROM ledger_entries e WHERE e.session_id = activity_sessions.id \
             AND e.entry_type NOT IN ('connection', 'activity_tick'))",
    )
    .bind(&before)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM chat_sessions WHERE activity_session_id IN \
             (SELECT id FROM activity_sessions \
              WHERE kind = 'terminal' AND ended_at IS NULL AND started_at < ?1)",
    )
    .bind(&before)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE chat_sessions SET ended_at = ( \
             SELECT COALESCE(a.last_activity_at, a.started_at) FROM activity_sessions a \
             WHERE a.id = chat_sessions.activity_session_id) \
         WHERE ended_at IS NULL AND activity_session_id IN \
             (SELECT id FROM activity_sessions WHERE ended_at IS NULL AND started_at < ?1)",
    )
    .bind(&before)
    .execute(&mut *tx)
    .await?;
    let res = sqlx::query(
        "UPDATE activity_sessions SET ended_at = COALESCE(last_activity_at, started_at) \
         WHERE ended_at IS NULL AND started_at < ?1",
    )
    .bind(&before)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(res.rows_affected())
}

/// Client name stored for an MCP recorded session whose handshake gave none.
pub const UNKNOWN_MCP_CLIENT: &str = "unknown";

/// Store for recorded sessions; shares the pool with the other stores.
#[derive(Clone)]
pub struct SqliteActivitySessionStore {
    pool: SqlitePool,
    time_zone: TimeZoneSource,
}

fn corrupt(id: Uuid, field: &str, reason: impl std::fmt::Display) -> LedgerStoreError {
    LedgerStoreError::CorruptSession {
        session_id: id.to_string(),
        field: field.to_string(),
        reason: reason.to_string(),
    }
}

fn json_list(id: Uuid, field: &str, raw: &str) -> Result<Vec<String>, LedgerStoreError> {
    serde_json::from_str(raw).map_err(|e| corrupt(id, field, e))
}

impl SqliteActivitySessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            time_zone: device_time_zone_source(),
        }
    }

    /// Creates the recorded session of an MCP-driven connection (kind `mcp`,
    /// snapshot as for chats) and returns its id. No `chat_sessions` row is
    /// written, so the session never shows up in chat lists or resume.
    /// `client_name` is the (already normalised) MCP client name; `None` or
    /// blank is stored as [`UNKNOWN_MCP_CLIENT`]. Fails when the server does
    /// not exist.
    pub async fn create_mcp_session(
        &self,
        server_id: &ServerId,
        client_name: Option<&str>,
    ) -> Result<Uuid, LedgerStoreError> {
        let client = client_name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(UNKNOWN_MCP_CLIENT);
        let id = Uuid::new_v4();
        let zone = (self.time_zone)();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        let created = insert_snapshot(
            &mut tx,
            id,
            ActivitySessionKind::Mcp,
            Some(client),
            server_id,
            zone.as_deref(),
        )
        .await
        .map_err(|e| LedgerStoreError::Database(e.into()))?;
        if !created {
            return Err(LedgerStoreError::Backend(format!(
                "server {} not found for the recorded session",
                server_id.0
            )));
        }
        tx.commit()
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        Ok(id)
    }

    /// Records a connection attempt that failed before a session existed
    /// (issue #1447): a `terminal` recorded session with one `failed`
    /// connection entry, already ended. `step` is the stable failure code of
    /// the step that failed (no message text, no secrets). Fails when the
    /// server does not exist.
    pub async fn record_failed_connection(
        &self,
        server_id: &ServerId,
        step: &str,
    ) -> Result<Uuid, LedgerStoreError> {
        let id = Uuid::new_v4();
        let zone = (self.time_zone)();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        let created = insert_snapshot(
            &mut tx,
            id,
            ActivitySessionKind::Terminal,
            None,
            server_id,
            zone.as_deref(),
        )
        .await
        .map_err(|e| LedgerStoreError::Database(e.into()))?;
        if !created {
            return Err(LedgerStoreError::Backend(format!(
                "server {} not found for the recorded session",
                server_id.0
            )));
        }
        tx.commit()
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        crate::SqliteLedgerStore::new(self.pool.clone())
            .append_entry(
                id,
                ssh_manager_core::audit::LedgerSource::User,
                &ssh_manager_core::audit::LedgerEntryContent::Connection {
                    event: ssh_manager_core::audit::LedgerConnectionEvent::Failed,
                    detail: Some(step.to_owned()),
                },
            )
            .await?;
        self.mark_ended(id).await?;
        Ok(id)
    }

    /// Whether `id` is a chat session (a `chat_sessions` row). A recorded
    /// session of another kind (MCP) is not, so it can never be resumed as a
    /// human chat.
    pub async fn is_chat_session(&self, id: Uuid) -> Result<bool, LedgerStoreError> {
        let found: Option<i64> = sqlx::query_scalar("SELECT 1 FROM chat_sessions WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        Ok(found.is_some())
    }

    /// Closes recorded sessions a previous run left open (issue #1447); see
    /// [`close_open_sessions`]. Call once at app start with the start time
    /// of this run.
    pub async fn close_open_sessions(
        &self,
        before: chrono::DateTime<Utc>,
    ) -> Result<u64, LedgerStoreError> {
        close_open_sessions(&self.pool, before)
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))
    }

    /// Sets `ended_at` of a recorded session (the MCP session was closed).
    pub async fn mark_ended(&self, id: Uuid) -> Result<(), LedgerStoreError> {
        sqlx::query("UPDATE activity_sessions SET ended_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        Ok(())
    }

    /// Loads one recorded session, `None` if it does not exist.
    pub async fn load(&self, id: Uuid) -> Result<Option<ActivitySession>, LedgerStoreError> {
        let row = sqlx::query("SELECT * FROM activity_sessions WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        row.map(|row| {
            let list = |field: &str| json_list(id, field, &row.get::<String, _>(field));
            let raw_port: i64 = row.get("server_port");
            let server_port = u16::try_from(raw_port)
                .map_err(|_| corrupt(id, "server_port", format!("{raw_port} out of range")))?;
            Ok(ActivitySession {
                id,
                kind: row.get("kind"),
                mcp_client: row.get("mcp_client"),
                server_id: row.get("server_id"),
                server_name: row.get("server_name"),
                server_host: row.get("server_host"),
                server_port,
                server_user: row.get("server_user"),
                group_ids: list("group_ids")?,
                group_names: list("group_names")?,
                tags: list("tags")?,
                time_zone: row.get("time_zone"),
                started_at: row.get("started_at"),
                ended_at: row.get("ended_at"),
                last_activity_at: row.get("last_activity_at"),
                failed_writes: row.get("failed_writes"),
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;
    use ssh_manager_core::profiles::ProfileStore;
    use ssh_manager_core::testing::ServerBuilder;

    use super::*;
    use crate::SqliteProfileStore;

    /// Creates a chat session with the given zone source; returns its recorded zone.
    async fn recorded_zone(source: TimeZoneSource) -> Option<String> {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let profiles = SqliteProfileStore::connect_with(options).await.unwrap();
        let server = ServerBuilder::new().name("Test-Server").build();
        profiles.create_server(&server).await.unwrap();
        let chats = profiles.chat_session_store().with_time_zone_source(source);
        let id = chats.create_session(&server.id, None).await.unwrap();
        let session = profiles.activity_session_store().load(id).await;
        session.unwrap().unwrap().time_zone
    }

    #[tokio::test]
    async fn test_new_session_stores_injected_time_zone() {
        let zone = recorded_zone(std::sync::Arc::new(|| Some("Europe/Berlin".into()))).await;
        assert_eq!(zone.as_deref(), Some("Europe/Berlin"));
    }

    #[tokio::test]
    async fn test_failed_time_zone_lookup_still_creates_session_with_null() {
        assert_eq!(recorded_zone(std::sync::Arc::new(|| None)).await, None);
    }
}
