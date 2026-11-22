//! Read-only queries over the activity record (Spec 0057, issue #1449).
//!
//! The one place that knows the layout of `activity_sessions` and
//! `ledger_entries` for readers outside the writers. It never reads
//! `chat_messages`, never writes, and returns entries exactly as stored (the
//! writer redacted them). Paging is keyset-based on the table's insertion
//! order (`rowid`, which only grows on append): a page boundary stays valid
//! while entries are appended, and every row is returned exactly once.
//!
//! Every statement is bounded by `LIMIT`, because the pool has a single
//! connection: a long read would block appends. `entries_of_session` reads a
//! session in keyset chunks on `sequence` for the same reason.
//!
//! A damaged row never fails a page or a session: it is returned as
//! [`RecordedRow::Unreadable`] at its position and the walk continues.

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{QueryBuilder, Row, Sqlite};
use uuid::Uuid;

use crate::activity_session_store::ActivitySession;
use crate::ledger_store::{ledger_entry_from_row, LedgerEntry, LedgerStoreError};

/// Entries read per statement by `entries_of_session`.
const SESSION_CHUNK: i64 = 1_000;

/// Rows examined per statement while a text filter is matched in Rust.
const TEXT_SCAN_CHUNK: i64 = 500;

/// Rows examined per `list_entries` call with a text filter. A call that
/// reaches it returns what it found so far (possibly an empty page) together
/// with a cursor, so a page stays fast on a very large record.
const TEXT_SCAN_BUDGET: i64 = 5_000;

/// Largest page a caller can get.
pub const MAX_PAGE_SIZE: u32 = 200;

/// Position after which the next page starts (opaque to callers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordCursor(pub i64);

/// Page request: `limit` is clamped to `1..=MAX_PAGE_SIZE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    pub limit: u32,
    pub after: Option<RecordCursor>,
}

impl PageRequest {
    fn effective_limit(&self) -> i64 {
        i64::from(self.limit.clamp(1, MAX_PAGE_SIZE))
    }
}

/// One page plus the cursor of the next one (`None` = no further rows at
/// the time of the query).
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<RecordCursor>,
}

/// Filter for recorded sessions. Empty list / `None` = no restriction.
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    /// Sessions started at or after this instant.
    pub started_from: Option<DateTime<Utc>>,
    /// Sessions started before this instant.
    pub started_until: Option<DateTime<Utc>>,
    pub server_ids: Vec<String>,
    /// Group id from the snapshot, any level of the path (so subgroups match).
    pub group_id: Option<String>,
    /// `chat`, `mcp`, `terminal`.
    pub kinds: Vec<String>,
    /// Only sessions with at least one failed ledger write.
    pub has_failed_writes: bool,
}

/// Filter for ledger entries. Empty list / `None` = no restriction.
#[derive(Debug, Clone, Default)]
pub struct EntryFilter {
    /// Entries created at or after this instant.
    pub from: Option<DateTime<Utc>>,
    /// Entries created before this instant.
    pub until: Option<DateTime<Utc>>,
    pub session_ids: Vec<Uuid>,
    pub server_ids: Vec<String>,
    pub group_id: Option<String>,
    /// `user`, `ai`, `mcp-agent`.
    pub sources: Vec<String>,
    /// `command_proposed`, `decision`, … (the stored `entry_type` values).
    pub entry_types: Vec<String>,
    /// `Confirmed`, `Rejected`, `AutoApproved` (decision entries only).
    pub decision_outcomes: Vec<String>,
    /// Executed commands whose exit code is not 0; an unknown exit code
    /// (killed, cancelled) counts as not 0.
    pub exit_code_nonzero: bool,
    /// Case-insensitive (Unicode) substring of a command text or an AI message.
    pub text_contains: Option<String>,
}

/// A ledger entry with the recorded session it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedEntry {
    pub session_id: Uuid,
    pub sequence: i64,
    pub entry: LedgerEntry,
}

/// A row of `ledger_entries` that could not be mapped (invalid content,
/// timestamp, rule snapshot, session id or an unknown source).
#[derive(Debug, Clone, PartialEq)]
pub struct UnreadableEntry {
    pub id: String,
    /// `None` when the stored session id is itself not a valid UUID.
    pub session_id: Option<Uuid>,
    pub sequence: i64,
    pub reason: String,
}

/// One row of a read: the entry, or a marker for a damaged one.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedRow {
    Entry(Box<RecordedEntry>),
    Unreadable(UnreadableEntry),
}

impl RecordedRow {
    pub fn sequence(&self) -> i64 {
        match self {
            Self::Entry(e) => e.sequence,
            Self::Unreadable(u) => u.sequence,
        }
    }

    pub fn session_id(&self) -> Option<Uuid> {
        match self {
            Self::Entry(e) => Some(e.session_id),
            Self::Unreadable(u) => u.session_id,
        }
    }
}

/// Read-only query handle; shares the pool with the other stores.
#[derive(Clone)]
pub struct SqliteRecordQuery {
    pool: SqlitePool,
}

fn err(e: sqlx::Error) -> LedgerStoreError {
    LedgerStoreError::Database(e.into())
}

fn push_in(qb: &mut QueryBuilder<Sqlite>, column: &'static str, values: &[String]) {
    qb.push(" AND ").push(column).push(" IN (");
    let mut sep = qb.separated(", ");
    for v in values {
        sep.push_bind(v.clone());
    }
    sep.push_unseparated(")");
}

fn push_group(qb: &mut QueryBuilder<Sqlite>, group_id: &str) {
    qb.push(" AND EXISTS (SELECT 1 FROM json_each(a.group_ids) WHERE value = ")
        .push_bind(group_id.to_owned())
        .push(")");
}

impl SqliteRecordQuery {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Recorded sessions, oldest first.
    pub async fn list_sessions(
        &self,
        filter: &SessionFilter,
        page: PageRequest,
    ) -> Result<Page<ActivitySession>, LedgerStoreError> {
        let limit = page.effective_limit();
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT a.rowid AS rid, a.id, a.kind, a.mcp_client, a.server_id, a.server_name, \
             a.server_host, a.server_port, a.server_user, a.group_ids, a.group_names, a.tags, \
             a.time_zone, a.started_at, a.ended_at, a.last_activity_at, a.failed_writes \
             FROM activity_sessions a WHERE 1 = 1",
        );
        if let Some(c) = page.after {
            qb.push(" AND a.rowid > ").push_bind(c.0);
        }
        if let Some(t) = filter.started_from {
            qb.push(" AND a.started_at >= ").push_bind(t.to_rfc3339());
        }
        if let Some(t) = filter.started_until {
            qb.push(" AND a.started_at < ").push_bind(t.to_rfc3339());
        }
        if !filter.server_ids.is_empty() {
            push_in(&mut qb, "a.server_id", &filter.server_ids);
        }
        if let Some(g) = filter.group_id.as_deref() {
            push_group(&mut qb, g);
        }
        if !filter.kinds.is_empty() {
            push_in(&mut qb, "a.kind", &filter.kinds);
        }
        if filter.has_failed_writes {
            qb.push(" AND a.failed_writes > 0");
        }
        qb.push(" ORDER BY a.rowid ASC LIMIT ").push_bind(limit + 1);
        let mut rows = qb.build().fetch_all(&self.pool).await.map_err(err)?;
        let next = take_next(&mut rows, limit, |r| r.get::<i64, _>("rid"));
        let items = rows
            .iter()
            .map(session_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page { items, next })
    }

    /// Ledger entries across sessions, in the order they were appended.
    ///
    /// A text filter is matched in Rust (Unicode case-insensitive; SQLite's
    /// `lower()` only folds ASCII) over keyset chunks of `TEXT_SCAN_CHUNK`
    /// rows, so each statement stays bounded. A call examines at most
    /// `TEXT_SCAN_BUDGET` rows; when it stops there, the page may be short
    /// (or empty) and `next` continues after the last row examined, so
    /// callers page on `next` rather than on the page being full.
    pub async fn list_entries(
        &self,
        filter: &EntryFilter,
        page: PageRequest,
    ) -> Result<Page<RecordedRow>, LedgerStoreError> {
        let limit = page.effective_limit();
        let needle = filter
            .text_contains
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase);
        let Some(needle) = needle else {
            let mut rows = self
                .entries_chunk(filter, page.after.map(|c| c.0), limit + 1, false)
                .await?;
            let next = take_next(&mut rows, limit, |r| r.get::<i64, _>("rid"));
            let items = rows.iter().map(row_to_item).collect();
            return Ok(Page { items, next });
        };
        let mut after = page.after.map(|c| c.0);
        let mut hit_ids: Vec<i64> = Vec::new();
        let mut scanned: i64 = 0;
        let mut budget_cursor: Option<i64> = None;
        loop {
            // Only the row id and the searched text are read while scanning.
            let chunk = self
                .entries_chunk(filter, after, TEXT_SCAN_CHUNK, true)
                .await?;
            let exhausted = (chunk.len() as i64) < TEXT_SCAN_CHUNK;
            scanned += chunk.len() as i64;
            for row in chunk {
                let rid: i64 = row.get("rid");
                after = Some(rid);
                let text: String = row.get("txt");
                if text.to_lowercase().contains(&needle) {
                    hit_ids.push(rid);
                    if hit_ids.len() as i64 > limit {
                        break;
                    }
                }
            }
            if hit_ids.len() as i64 > limit || exhausted {
                break;
            }
            if scanned >= TEXT_SCAN_BUDGET {
                budget_cursor = after;
                break;
            }
        }
        let has_more = hit_ids.len() as i64 > limit;
        hit_ids.truncate(limit as usize);
        let next = if has_more {
            hit_ids.last().map(|r| RecordCursor(*r))
        } else {
            budget_cursor.map(RecordCursor)
        };
        let mut items = Vec::with_capacity(hit_ids.len());
        for id in &hit_ids {
            let row = sqlx::query(
                "SELECT e.id, e.session_id, e.sequence, e.source, e.content, e.created_at, \
                 e.action_id, e.redaction_incomplete, e.rule_snapshot \
                 FROM ledger_entries e WHERE e.rowid = ?",
            )
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(err)?;
            items.push(row_to_item(&row));
        }
        Ok(Page { items, next })
    }

    async fn entries_chunk(
        &self,
        filter: &EntryFilter,
        after: Option<i64>,
        limit: i64,
        with_text: bool,
    ) -> Result<Vec<SqliteRow>, LedgerStoreError> {
        let mut qb = QueryBuilder::<Sqlite>::new(if with_text {
            "SELECT e.rowid AS rid"
        } else {
            "SELECT e.rowid AS rid, e.id, e.session_id, e.sequence, e.source, e.content, \
             e.created_at, e.action_id, e.redaction_incomplete, e.rule_snapshot"
        });
        if with_text {
            qb.push(
                ", COALESCE(json_extract(e.content, '$.CommandProposed.command'), \
                 json_extract(e.content, '$.CommandExecuted.command'), \
                 json_extract(e.content, '$.AiMessage.text'), '') AS txt",
            );
        }
        qb.push(
            " FROM ledger_entries e JOIN activity_sessions a ON a.id = e.session_id \
             WHERE 1 = 1",
        );
        if let Some(c) = after {
            qb.push(" AND e.rowid > ").push_bind(c);
        }
        if let Some(t) = filter.from {
            qb.push(" AND e.created_at >= ").push_bind(t.to_rfc3339());
        }
        if let Some(t) = filter.until {
            qb.push(" AND e.created_at < ").push_bind(t.to_rfc3339());
        }
        if !filter.session_ids.is_empty() {
            let ids: Vec<String> = filter.session_ids.iter().map(Uuid::to_string).collect();
            qb.push(" AND e.session_id IN (");
            let mut sep = qb.separated(", ");
            for id in ids {
                sep.push_bind(id);
            }
            sep.push_unseparated(")");
        }
        if !filter.server_ids.is_empty() {
            push_in(&mut qb, "a.server_id", &filter.server_ids);
        }
        if let Some(g) = filter.group_id.as_deref() {
            push_group(&mut qb, g);
        }
        if !filter.sources.is_empty() {
            push_in(&mut qb, "e.source", &filter.sources);
        }
        if !filter.entry_types.is_empty() {
            push_in(&mut qb, "e.entry_type", &filter.entry_types);
        }
        if !filter.decision_outcomes.is_empty() {
            qb.push(" AND e.entry_type = 'decision'");
            push_in(
                &mut qb,
                "json_extract(e.content, '$.Decision.outcome')",
                &filter.decision_outcomes,
            );
        }
        if filter.exit_code_nonzero {
            qb.push(
                " AND e.entry_type = 'command_executed' AND \
                 COALESCE(json_extract(e.content, '$.CommandExecuted.output.exit_code'), -1) <> 0",
            );
        }
        qb.push(" ORDER BY e.rowid ASC LIMIT ").push_bind(limit);
        qb.build().fetch_all(&self.pool).await.map_err(err)
    }

    /// All entries of one recorded session, in `sequence` order. Read in
    /// chunks of `SESSION_CHUNK` (one statement each), so the single
    /// connection is free for appends between chunks. A damaged row is
    /// returned as [`RecordedRow::Unreadable`] at its place.
    pub async fn entries_of_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<RecordedRow>, LedgerStoreError> {
        let mut out = Vec::new();
        let mut after = i64::MIN;
        loop {
            let rows = sqlx::query(
                "SELECT e.id, e.session_id, e.sequence, e.source, e.content, e.created_at, \
                 e.action_id, e.redaction_incomplete, e.rule_snapshot \
                 FROM ledger_entries e WHERE e.session_id = ? AND e.sequence > ? \
                 ORDER BY e.sequence ASC LIMIT ?",
            )
            .bind(session_id.to_string())
            .bind(after)
            .bind(SESSION_CHUNK)
            .fetch_all(&self.pool)
            .await
            .map_err(err)?;
            let Some(last) = rows.last() else { break };
            after = last.get::<i64, _>("sequence");
            let full = rows.len() as i64 == SESSION_CHUNK;
            out.extend(rows.iter().map(row_to_item));
            if !full {
                break;
            }
        }
        Ok(out)
    }
}

/// Drops the probe row (`limit + 1`) and returns the cursor of the last
/// returned row when there is a further page.
fn take_next(
    rows: &mut Vec<SqliteRow>,
    limit: i64,
    rid: impl Fn(&SqliteRow) -> i64,
) -> Option<RecordCursor> {
    if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        rows.last().map(|r| RecordCursor(rid(r)))
    } else {
        None
    }
}

fn json_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn session_from_row(row: &SqliteRow) -> Result<ActivitySession, LedgerStoreError> {
    let id: String = row.get("id");
    let id = Uuid::parse_str(&id)
        .map_err(|e| LedgerStoreError::Backend(format!("invalid session id: {e}")))?;
    Ok(ActivitySession {
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
    })
}

/// Maps a row; a damaged one becomes an [`UnreadableEntry`] (never an error).
fn row_to_item(row: &SqliteRow) -> RecordedRow {
    let id: String = row.get("id");
    let sequence: i64 = row.get("sequence");
    let session_id = Uuid::parse_str(&row.get::<String, _>("session_id"));
    let unreadable = |reason: String| {
        RecordedRow::Unreadable(UnreadableEntry {
            id: id.clone(),
            session_id: session_id.as_ref().ok().copied(),
            sequence,
            reason,
        })
    };
    let Ok(sid) = session_id.as_ref() else {
        return unreadable("session id: invalid".to_owned());
    };
    match ledger_entry_from_row(row) {
        Ok(entry) => RecordedRow::Entry(Box::new(RecordedEntry {
            session_id: *sid,
            sequence,
            entry,
        })),
        Err(LedgerStoreError::CorruptContent { reason, .. }) => unreadable(reason),
        Err(e) => unreadable(e.to_string()),
    }
}
