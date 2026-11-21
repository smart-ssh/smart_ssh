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
        "WITH RECURSIVE path(gid, name, parent_id, depth) AS ( \
             SELECT g.id, g.name, g.parent_id, 0 FROM servers s \
                 JOIN groups g ON g.id = s.group_id WHERE s.id = ?1 \
             UNION ALL \
             SELECT g.id, g.name, g.parent_id, p.depth + 1 FROM path p \
                 JOIN groups g ON g.id = p.parent_id WHERE p.depth < 64 \
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
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let _server_exists = insert_snapshot(
        &mut tx,
        id,
        ActivitySessionKind::Chat,
        None,
        server_id,
        None,
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

/// Deletes a chat session that has no messages and no ledger entries, plus
/// its (then unused) recorded session. Check and delete are one write
/// transaction. Returns whether the chat session was deleted.
pub(crate) async fn delete_chat_session_if_empty(
    pool: &SqlitePool,
    session_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let res = sqlx::query(
        "DELETE FROM chat_sessions WHERE id = ? \
         AND NOT EXISTS (SELECT 1 FROM chat_messages WHERE session_id = chat_sessions.id) \
         AND NOT EXISTS (SELECT 1 FROM ledger_entries \
             WHERE session_id = COALESCE(chat_sessions.activity_session_id, chat_sessions.id))",
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
    }
    tx.commit().await?;
    Ok(deleted)
}

/// Store for recorded sessions; shares the pool with the other stores.
#[derive(Clone)]
pub struct SqliteActivitySessionStore {
    pool: SqlitePool,
}

fn json_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

impl SqliteActivitySessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Loads one recorded session, `None` if it does not exist.
    pub async fn load(&self, id: Uuid) -> Result<Option<ActivitySession>, LedgerStoreError> {
        let row = sqlx::query("SELECT * FROM activity_sessions WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LedgerStoreError::Database(e.into()))?;
        Ok(row.map(|row| ActivitySession {
            id,
            kind: row.get("kind"),
            mcp_client: row.get("mcp_client"),
            server_id: row.get("server_id"),
            server_name: row.get("server_name"),
            server_host: row.get("server_host"),
            server_port: u16::try_from(row.get::<i64, _>("server_port")).unwrap_or(0),
            server_user: row.get("server_user"),
            group_ids: json_list(&row.get::<String, _>("group_ids")),
            group_names: json_list(&row.get::<String, _>("group_names")),
            tags: json_list(&row.get::<String, _>("tags")),
            time_zone: row.get("time_zone"),
            started_at: row.get("started_at"),
            ended_at: row.get("ended_at"),
            last_activity_at: row.get("last_activity_at"),
            failed_writes: row.get("failed_writes"),
        }))
    }
}
