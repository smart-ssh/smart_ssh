//! The persisted `servers` columns, defined once (issue #904).
//!
//! The profile store (`create_server`, `update_server`, SELECT) and the backup
//! store (upsert, plain insert) all build their SQL from [`SERVER_COLUMNS`] and
//! bind through [`bind_server_row`], so a new column is added in exactly one
//! place and cannot be dropped silently by backup or restore.

use std::sync::LazyLock;

use sqlx::query::Query;
use sqlx::sqlite::{Sqlite, SqliteArguments};

use crate::backup_store::ServerRow;

/// Every column of `servers`, in the order the statements below use.
pub(crate) const SERVER_COLUMNS: &[&str] = &[
    "id",
    "name",
    "host",
    "port",
    "username",
    "group_id",
    "auth_method",
    "notes",
    "jump_host_id",
    "created_at",
    "updated_at",
    "post_ingest_policy",
    "ai_injection_check_enabled",
    "sftp_server_path",
    "start_directory",
    "is_copy",
    "elevation_allowed",
];

/// Columns the profile store does not change on `UPDATE`: the key, the
/// creation time, and `is_copy` (the import/backup flow owns the copy marker,
/// the domain `Server` does not carry it).
const NOT_UPDATED_BY_PROFILE_STORE: &[&str] = &["id", "created_at", "is_copy"];

/// Columns the profile store writes on `INSERT`: all but `is_copy`, which stays
/// at the schema default (`false`) for a server created through the UI.
const NOT_INSERTED_BY_PROFILE_STORE: &[&str] = &["is_copy"];

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

fn without(excluded: &'static [&'static str]) -> Vec<&'static str> {
    SERVER_COLUMNS
        .iter()
        .copied()
        .filter(|c| !excluded.contains(c))
        .collect()
}

/// `SELECT` list covering every column.
pub(crate) static SELECT_LIST: LazyLock<String> = LazyLock::new(|| SERVER_COLUMNS.join(", "));

/// Plain `INSERT` of every column (backup restore), bound with [`bind_server_row`]
/// over [`SERVER_COLUMNS`].
pub(crate) static INSERT_ALL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "INSERT INTO servers ({}) VALUES ({})",
        SERVER_COLUMNS.join(", "),
        placeholders(SERVER_COLUMNS.len())
    )
});

/// `INSERT … ON CONFLICT(id) DO UPDATE` of every column (backup import diff).
pub(crate) static UPSERT_ALL: LazyLock<String> = LazyLock::new(|| {
    let set: Vec<String> = SERVER_COLUMNS[1..]
        .iter()
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    format!(
        "{} ON CONFLICT(id) DO UPDATE SET {}",
        *INSERT_ALL,
        set.join(", ")
    )
});

/// Columns of the profile store's `INSERT`.
pub(crate) static PROFILE_INSERT_COLUMNS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| without(NOT_INSERTED_BY_PROFILE_STORE));

pub(crate) static PROFILE_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "INSERT INTO servers ({}) VALUES ({})",
        PROFILE_INSERT_COLUMNS.join(", "),
        placeholders(PROFILE_INSERT_COLUMNS.len())
    )
});

/// Columns of the profile store's `UPDATE … WHERE id = ?` (the id is bound last).
pub(crate) static PROFILE_UPDATE_COLUMNS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| without(NOT_UPDATED_BY_PROFILE_STORE));

pub(crate) static PROFILE_UPDATE: LazyLock<String> = LazyLock::new(|| {
    let set: Vec<String> = PROFILE_UPDATE_COLUMNS
        .iter()
        .map(|c| format!("{c} = ?"))
        .collect();
    format!("UPDATE servers SET {} WHERE id = ?", set.join(", "))
});

type Q<'q> = Query<'q, Sqlite, SqliteArguments>;

/// Binds the value of one column. An unknown column name panics: it can only
/// come from a column added to [`SERVER_COLUMNS`] without a binding here,
/// which the round-trip test turns into a failure.
fn bind_column<'q>(q: Q<'q>, column: &str, s: &'q ServerRow) -> Q<'q> {
    match column {
        "id" => q.bind(s.id.to_string()),
        "name" => q.bind(&s.name),
        "host" => q.bind(&s.host),
        "port" => q.bind(i64::from(s.port)),
        "username" => q.bind(&s.username),
        "group_id" => q.bind(s.group_id.map(|p| p.to_string())),
        "auth_method" => q.bind(&s.auth_method),
        "notes" => q.bind(&s.notes),
        "jump_host_id" => q.bind(s.jump_host_id.map(|p| p.to_string())),
        "created_at" => q.bind(&s.created_at),
        "updated_at" => q.bind(&s.updated_at),
        "post_ingest_policy" => q.bind(&s.post_ingest_policy),
        "ai_injection_check_enabled" => q.bind(s.ai_injection_check_enabled),
        "sftp_server_path" => q.bind(&s.sftp_server_path),
        "start_directory" => q.bind(&s.start_directory),
        "is_copy" => q.bind(s.is_copy),
        "elevation_allowed" => q.bind(s.elevation_allowed),
        other => panic!("servers column `{other}` has no binding in server_columns.rs"),
    }
}

/// Binds `columns` (in that order) from `row`.
pub(crate) fn bind_server_row<'q>(mut q: Q<'q>, columns: &[&str], row: &'q ServerRow) -> Q<'q> {
    for c in columns {
        q = bind_column(q, c, row);
    }
    q
}

/// Binds every column in [`SERVER_COLUMNS`] order.
pub(crate) fn bind_all<'q>(q: Q<'q>, row: &'q ServerRow) -> Q<'q> {
    bind_server_row(q, SERVER_COLUMNS, row)
}
