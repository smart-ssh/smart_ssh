//! Machine-readable kind of a SQLite failure (issue #885).
//!
//! The stores used to flatten `sqlx::Error` into a string, so the command
//! layer could not tell "database busy" from "constraint violated". The kind
//! is derived from the SQLite extended result code, never from message text.

pub use ssh_manager_core::profiles::StoreErrorKind;

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
            .busy_timeout(Duration::from_millis(50));
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
}
