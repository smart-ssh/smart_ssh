//! Machine-readable kind of a SQLite failure (issue #885).
//!
//! The stores used to flatten `sqlx::Error` into a string, so the command
//! layer could not tell "database busy" from "constraint violated". The kind
//! is derived from the SQLite extended result code, never from message text.

pub use ssh_manager_core::profiles::StoreErrorKind;

/// A classified SQLite failure: the kind plus sqlx's error text (never bound
/// values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbFailure {
    pub kind: StoreErrorKind,
    pub message: String,
}

impl From<sqlx::Error> for DbFailure {
    fn from(e: sqlx::Error) -> Self {
        Self {
            kind: classify(&e),
            message: e.to_string(),
        }
    }
}

impl From<sqlx::Error> for crate::chat_session_store::ChatSessionStoreError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.into())
    }
}

// Primary SQLite result codes (the low byte of an extended code).
const SQLITE_BUSY: i32 = 5;
const SQLITE_LOCKED: i32 = 6;
const SQLITE_CORRUPT: i32 = 11;
const SQLITE_CONSTRAINT: i32 = 19;
const SQLITE_NOTADB: i32 = 26;

/// Classifies a `sqlx` error. Unknown shapes are [`StoreErrorKind::Other`].
pub fn classify(err: &sqlx::Error) -> StoreErrorKind {
    match err {
        sqlx::Error::RowNotFound => StoreErrorKind::NotFound,
        sqlx::Error::Io(_) => StoreErrorKind::Io,
        sqlx::Error::PoolTimedOut => StoreErrorKind::Busy,
        sqlx::Error::Database(db) => {
            let primary = db
                .code()
                .and_then(|c| c.parse::<i32>().ok())
                .map(|c| c & 0xff);
            match primary {
                Some(SQLITE_BUSY | SQLITE_LOCKED) => StoreErrorKind::Busy,
                Some(SQLITE_CONSTRAINT) => StoreErrorKind::Constraint,
                Some(SQLITE_CORRUPT | SQLITE_NOTADB) => StoreErrorKind::Corrupt,
                _ => StoreErrorKind::Other,
            }
        }
        _ => StoreErrorKind::Other,
    }
}

impl crate::chat_session_store::ChatSessionStoreError {
    /// Machine-readable kind of the failure; `None` for errors that are not
    /// a database failure.
    pub fn kind(&self) -> Option<StoreErrorKind> {
        use crate::chat_session_store::ChatSessionStoreError as E;
        match self {
            E::Database(d) => Some(d.kind),
            E::Backend(_) => Some(StoreErrorKind::Other),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl crate::ai_provider_store::AiProviderStoreError {
    /// Machine-readable kind of the failure; `None` for errors that are not
    /// a database failure.
    pub fn kind(&self) -> Option<StoreErrorKind> {
        use crate::ai_provider_store::AiProviderStoreError as E;
        match self {
            E::Database(d) => Some(d.kind),
            E::Backend(_) => Some(StoreErrorKind::Other),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl crate::policy_store::PolicyStoreError {
    /// Machine-readable kind of the failure; `None` for errors that are not
    /// a database failure.
    pub fn kind(&self) -> Option<StoreErrorKind> {
        use crate::policy_store::PolicyStoreError as E;
        match self {
            E::Database(d) => Some(d.kind),
            E::Backend(_) => Some(StoreErrorKind::Other),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl crate::ledger_store::LedgerStoreError {
    /// Machine-readable kind of the failure; `None` for errors that are not
    /// a database failure.
    pub fn kind(&self) -> Option<StoreErrorKind> {
        use crate::ledger_store::LedgerStoreError as E;
        match self {
            E::Database(d) => Some(d.kind),
            E::Backend(_) => Some(StoreErrorKind::Other),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl crate::prompt_history_store::PromptHistoryStoreError {
    /// Machine-readable kind of the failure; `None` for errors that are not
    /// a database failure.
    pub fn kind(&self) -> Option<StoreErrorKind> {
        use crate::prompt_history_store::PromptHistoryStoreError as E;
        match self {
            E::Database(d) => Some(d.kind),
            E::Backend(_) => Some(StoreErrorKind::Other),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// Spec 0077, 3.1.3: a filter rule pattern that does not compile.
pub const FILTER_RULE_PATTERN_INVALID: &str = "FILTER_RULE_PATTERN_INVALID";

/// Codes of the `ErrorCode` impls in this crate that are not `STORE_*`
/// (those are `StoreErrorKind::CODES`).
pub const STORE_ERROR_CODES: &[&str] = &[
    "AI_PROVIDER_NOT_FOUND",
    "AI_PROVIDER_ACTIVE_DELETION_FORBIDDEN",
    "POLICY_RULE_NOT_FOUND",
    FILTER_RULE_PATTERN_INVALID,
];

use ssh_manager_core::ipc_error::ErrorCode;

/// The database failure kind wins; domain variants get their own code.
fn kind_code(kind: Option<StoreErrorKind>, domain: Option<&'static str>) -> &'static str {
    match (domain, kind) {
        (Some(code), _) => code,
        (None, Some(kind)) => kind.code(),
        // Unreachable for the current variants; a generic store code rather
        // than none if a variant without a kind is ever added.
        (None, None) => StoreErrorKind::Other.code(),
    }
}

impl ErrorCode for crate::chat_session_store::ChatSessionStoreError {
    fn code(&self) -> &'static str {
        use crate::chat_session_store::ChatSessionStoreError as E;
        match self {
            E::CorruptContent { .. } => StoreErrorKind::Corrupt.code(),
            _ => kind_code(self.kind(), None),
        }
    }
}

impl ErrorCode for crate::ai_provider_store::AiProviderStoreError {
    fn code(&self) -> &'static str {
        use crate::ai_provider_store::AiProviderStoreError as E;
        let domain = match self {
            E::NotFound(_) => Some("AI_PROVIDER_NOT_FOUND"),
            E::ActiveProviderDeletionForbidden(_) => Some("AI_PROVIDER_ACTIVE_DELETION_FORBIDDEN"),
            _ => None,
        };
        kind_code(self.kind(), domain)
    }
}

impl ErrorCode for crate::policy_store::PolicyStoreError {
    fn code(&self) -> &'static str {
        use crate::policy_store::PolicyStoreError as E;
        let domain = match self {
            E::NotFound(_) => Some("POLICY_RULE_NOT_FOUND"),
            E::InvalidPattern(_) => Some(FILTER_RULE_PATTERN_INVALID),
            _ => None,
        };
        kind_code(self.kind(), domain)
    }
}

impl ErrorCode for crate::ledger_store::LedgerStoreError {
    fn code(&self) -> &'static str {
        use crate::ledger_store::LedgerStoreError as E;
        match self {
            E::CorruptContent { .. } => StoreErrorKind::Corrupt.code(),
            _ => kind_code(self.kind(), None),
        }
    }
}

impl ErrorCode for crate::backup_store::BackupStoreError {
    fn code(&self) -> &'static str {
        use crate::backup_store::BackupStoreError as E;
        match self {
            E::Database(_) => StoreErrorKind::Io.code(),
            E::Data(_) => StoreErrorKind::Corrupt.code(),
        }
    }
}

impl ErrorCode for crate::prompt_history_store::PromptHistoryStoreError {
    fn code(&self) -> &'static str {
        kind_code(self.kind(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::time::Duration;

    async fn memory_pool() -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn unique_violation_is_constraint() {
        let pool = memory_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT UNIQUE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t (v) VALUES ('a')")
            .execute(&pool)
            .await
            .unwrap();
        let err = sqlx::query("INSERT INTO t (v) VALUES ('a')")
            .execute(&pool)
            .await
            .unwrap_err();
        assert_eq!(classify(&err), StoreErrorKind::Constraint);
    }

    #[tokio::test]
    async fn missing_row_is_not_found() {
        let pool = memory_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        let err = sqlx::query("SELECT id FROM t WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap_err();
        assert_eq!(classify(&err), StoreErrorKind::NotFound);
    }

    #[tokio::test]
    async fn second_writer_with_short_timeout_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("busy.db");
        let opts = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(30));
        let a = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts.clone())
            .await
            .unwrap();
        let b = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (id INTEGER)")
            .execute(&a)
            .await
            .unwrap();
        // Only the contended statement gets the short timeout. Connecting and
        // setting up the schema are uncontended in theory, but with a short
        // timeout on the whole connection a slow runner (e.g. macOS CI) could
        // hit a spurious busy error there instead of at the assertion.
        sqlx::query("PRAGMA busy_timeout = 50")
            .execute(&b)
            .await
            .unwrap();
        let mut tx = a.begin().await.unwrap();
        sqlx::query("INSERT INTO t VALUES (1)")
            .execute(&mut *tx)
            .await
            .unwrap();
        // `a` holds the write lock until `tx` ends.
        let err = sqlx::query("INSERT INTO t VALUES (2)")
            .execute(&b)
            .await
            .unwrap_err();
        assert_eq!(classify(&err), StoreErrorKind::Busy);
        drop(tx);
    }

    #[tokio::test]
    async fn non_database_file_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("garbage.db");
        std::fs::write(&path, vec![b'x'; 8192]).unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
        let err = sqlx::query("SELECT name FROM sqlite_master")
            .fetch_all(&pool)
            .await
            .unwrap_err();
        assert_eq!(classify(&err), StoreErrorKind::Corrupt);
    }

    #[test]
    fn io_error_is_io() {
        let err = sqlx::Error::Io(std::io::Error::other("x"));
        assert_eq!(classify(&err), StoreErrorKind::Io);
    }

    /// Issue #885: a foreign-key violation keeps its kind in the chat store.
    #[tokio::test]
    async fn chat_store_constraint_failure_keeps_its_kind() {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let profile_store = crate::SqliteProfileStore::connect_with(options)
            .await
            .unwrap();
        let err = profile_store
            .chat_session_store()
            .create_session(&ssh_manager_core::shared::ServerId::new(), None)
            .await
            .unwrap_err();
        assert_eq!(err.kind(), Some(StoreErrorKind::Constraint));
    }
}
