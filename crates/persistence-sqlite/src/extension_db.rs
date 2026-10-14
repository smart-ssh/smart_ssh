//! Issue #462 (Spec 0038, Abschnitt 2a): extension-owned tables in the
//! encrypted application database.
//!
//! An extension registers a unique table prefix and an ordered list of its
//! own migrations ([`ExtensionRegistration`]). [`SqliteProfileStore::
//! run_extension_migrations`] runs them after the core migrations, with a
//! migration history of the extension's own (`<prefix>schema_migrations`),
//! and hands back an [`ExtensionDb`]. Why a runner of its own instead of a
//! second `sqlx::migrate::Migrator`: see ADR 0139.
//!
//! **What is enforced.** While an extension migration runs, a SQLite
//! authorizer is installed on the connection. It refuses everything that
//! reaches beyond the prefix: creating, altering or dropping a table or
//! index whose name (or whose table's name) does not start with the prefix,
//! reading or writing a foreign table, `PRAGMA`, `ATTACH`/`DETACH`, views,
//! triggers, virtual tables, transaction control and `load_extension`.
//! After the migration, foreign keys of the extension's tables must point
//! at tables of the same prefix (a foreign key into a core table is not an
//! authorizer event). A rejected migration rolls back; the error carries
//! the version and a category, never SQL text or values.
//!
//! **What is not.** The handle is the connection pool: extension code is
//! compiled into the binary and trusted like every other `Wiring` slot.
//! The check protects the core schema against a faulty migration, it is not
//! a sandbox for the extension's later queries.

use std::ffi::{c_char, c_int, c_void, CStr};

use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use crate::store::SqliteProfileStore;

/// One migration of an extension.
#[derive(Debug, Clone)]
pub struct ExtensionMigration {
    /// Strictly ascending within one registration, `> 0`.
    pub version: i64,
    /// Shown in the history table only.
    pub description: String,
    /// One or more statements.
    pub sql: String,
}

/// What an extension registers through `Wiring`.
#[derive(Debug, Clone)]
pub struct ExtensionRegistration {
    /// `ext_<name>_`: lowercase ASCII letters and digits between `ext_` and
    /// the closing underscore.
    pub prefix: String,
    pub migrations: Vec<ExtensionMigration>,
}

/// The extension's access to its tables. Only obtainable from a successful
/// [`SqliteProfileStore::run_extension_migrations`], i.e. only after the
/// database is open (after unlock in master-password mode).
#[derive(Clone)]
pub struct ExtensionDb {
    pool: SqlitePool,
    prefix: String,
}

impl ExtensionDb {
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

impl std::fmt::Debug for ExtensionDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionDb")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// Why an extension was not started. Carries no SQL text and no values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionMigrationError {
    /// Prefix malformed, versions not ascending, or the prefix collides with
    /// another registration.
    InvalidRegistration(&'static str),
    /// The migration reached beyond the prefix or used a forbidden feature.
    Rejected { version: i64, reason: &'static str },
    /// The history holds a version the registration no longer lists, or a
    /// recorded migration changed after it was applied.
    HistoryMismatch { version: i64 },
    /// SQLite refused the migration (the SQLite result code only).
    Database { version: i64, code: String },
}

impl std::fmt::Display for ExtensionMigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRegistration(why) => write!(f, "invalid extension registration: {why}"),
            Self::Rejected { version, reason } => {
                write!(f, "extension migration {version} rejected: {reason}")
            }
            Self::HistoryMismatch { version } => {
                write!(
                    f,
                    "extension migration history mismatch at version {version}"
                )
            }
            Self::Database { version, code } => {
                write!(
                    f,
                    "extension migration {version} failed (SQLite code {code})"
                )
            }
        }
    }
}

impl std::error::Error for ExtensionMigrationError {}

/// `ext_<name>_`, lowercase ASCII letters and digits.
pub fn validate_prefix(prefix: &str) -> Result<(), ExtensionMigrationError> {
    let bad = ExtensionMigrationError::InvalidRegistration("prefix must look like ext_<name>_");
    let inner = prefix
        .strip_prefix("ext_")
        .and_then(|rest| rest.strip_suffix('_'))
        .ok_or_else(|| bad.clone())?;
    if inner.is_empty()
        || !inner
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(bad);
    }
    Ok(())
}

/// No prefix may be the start of another one (`ext_a_` would own the tables
/// of `ext_a_b_`... only if the latter were allowed; the validation above
/// forbids inner underscores, so equality is the only collision).
pub fn validate_unique_prefixes<'a>(
    prefixes: impl IntoIterator<Item = &'a str>,
) -> Result<(), ExtensionMigrationError> {
    let mut seen = std::collections::BTreeSet::new();
    for p in prefixes {
        if !seen.insert(p.to_owned()) {
            return Err(ExtensionMigrationError::InvalidRegistration(
                "two extensions registered the same prefix",
            ));
        }
    }
    Ok(())
}

fn history_table(prefix: &str) -> String {
    format!("{prefix}schema_migrations")
}

/// FNV-1a over the SQL text: detects an edited, already applied migration.
fn checksum(sql: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in sql.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

// SQLite authorizer action codes (sqlite3.h).
const CREATE_INDEX: c_int = 1;
const CREATE_TABLE: c_int = 2;
const DELETE: c_int = 9;
const DROP_INDEX: c_int = 10;
const DROP_TABLE: c_int = 11;
const INSERT: c_int = 18;
const READ: c_int = 20;
const SELECT: c_int = 21;
const UPDATE: c_int = 23;
const ALTER_TABLE: c_int = 26;
const REINDEX: c_int = 27;
const ANALYZE: c_int = 28;
const FUNCTION: c_int = 31;
const RECURSIVE: c_int = 33;

fn owned(prefix: &str, name: Option<&str>) -> bool {
    name.is_some_and(|n| n.to_ascii_lowercase().starts_with(prefix))
}

fn is_schema_table(name: Option<&str>) -> bool {
    name.is_some_and(|n| {
        let n = n.to_ascii_lowercase();
        n == "sqlite_master" || n == "sqlite_schema"
    })
}

/// Pure decision of the authorizer: may this operation run inside an
/// extension migration with `prefix`?
fn permitted(prefix: &str, action: c_int, a1: Option<&str>, a2: Option<&str>) -> bool {
    match action {
        // Index/table DDL: the table (and for indexes the index) must be owned.
        CREATE_INDEX | DROP_INDEX => owned(prefix, a1) && owned(prefix, a2),
        CREATE_TABLE | DROP_TABLE => owned(prefix, a1),
        // `a1` is the database name for ALTER TABLE, `a2` the table.
        ALTER_TABLE => owned(prefix, a2),
        // SQLite reports its own schema bookkeeping (CREATE/ALTER/DROP) as
        // writes to `sqlite_master`; a hand-written one needs
        // `PRAGMA writable_schema`, which is refused.
        INSERT | UPDATE | DELETE => owned(prefix, a1) || is_schema_table(a1),
        READ => owned(prefix, a1) || is_schema_table(a1),
        REINDEX | ANALYZE => owned(prefix, a1),
        SELECT | RECURSIVE => true,
        FUNCTION => !a2.is_some_and(|f| f.eq_ignore_ascii_case("load_extension")),
        // Views, triggers, temp objects, PRAGMA, ATTACH/DETACH, virtual
        // tables, transactions, savepoints and anything unknown.
        _ => false,
    }
}

struct AuthContext {
    prefix: String,
    /// Set by the callback on the first refusal.
    refused: std::sync::atomic::AtomicBool,
}

unsafe extern "C" fn authorizer(
    user: *mut c_void,
    action: c_int,
    a1: *const c_char,
    a2: *const c_char,
    _a3: *const c_char,
    _a4: *const c_char,
) -> c_int {
    // SAFETY: `user` is the `AuthContext` boxed by `apply_migration`, alive
    // until the authorizer is removed again.
    let ctx = unsafe { &*(user as *const AuthContext) };
    let text = |p: *const c_char| -> Option<String> {
        if p.is_null() {
            None
        } else {
            // SAFETY: SQLite passes NUL-terminated strings.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    };
    let (a1, a2) = (text(a1), text(a2));
    if permitted(&ctx.prefix, action, a1.as_deref(), a2.as_deref()) {
        libsqlite3_sys::SQLITE_OK
    } else {
        ctx.refused.store(true, std::sync::atomic::Ordering::SeqCst);
        libsqlite3_sys::SQLITE_DENY
    }
}

fn db_error(version: i64, err: &sqlx::Error) -> ExtensionMigrationError {
    let code = err
        .as_database_error()
        .and_then(|e| e.code().map(|c| c.into_owned()))
        .unwrap_or_else(|| "unknown".to_owned());
    ExtensionMigrationError::Database { version, code }
}

impl SqliteProfileStore {
    /// Runs the extension's pending migrations and returns its handle.
    ///
    /// Call after the core migrations (i.e. on an opened store) and only
    /// after unlock. On error nothing of the failing migration remains and
    /// no handle is returned; migrations applied before it stay applied.
    pub async fn run_extension_migrations(
        &self,
        registration: &ExtensionRegistration,
    ) -> Result<ExtensionDb, ExtensionMigrationError> {
        let prefix = registration.prefix.as_str();
        validate_prefix(prefix)?;
        let mut last = 0;
        for m in &registration.migrations {
            if m.version <= last {
                return Err(ExtensionMigrationError::InvalidRegistration(
                    "migration versions must be positive and strictly ascending",
                ));
            }
            last = m.version;
        }

        let history = history_table(prefix);
        // The history table is created lazily, so a database without
        // extensions stays identical to before.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE TABLE IF NOT EXISTS {history} (\
             version INTEGER PRIMARY KEY, description TEXT NOT NULL, \
             checksum TEXT NOT NULL, \
             applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)"
        )))
        .execute(&self.pool)
        .await
        .map_err(|e| db_error(0, &e))?;

        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT version, checksum FROM {history} ORDER BY version"
        )))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_error(0, &e))?;
        let applied: Vec<(i64, String)> = rows
            .iter()
            .map(|r| (r.get::<i64, _>(0), r.get::<String, _>(1)))
            .collect();
        for (version, sum) in &applied {
            match registration
                .migrations
                .iter()
                .find(|m| m.version == *version)
            {
                Some(m) if checksum(&m.sql) == *sum => {}
                _ => return Err(ExtensionMigrationError::HistoryMismatch { version: *version }),
            }
        }

        for m in &registration.migrations {
            if applied.iter().any(|(v, _)| *v == m.version) {
                continue;
            }
            self.apply_extension_migration(prefix, &history, m).await?;
        }
        Ok(ExtensionDb {
            pool: self.pool.clone(),
            prefix: prefix.to_owned(),
        })
    }

    async fn apply_extension_migration(
        &self,
        prefix: &str,
        history: &str,
        m: &ExtensionMigration,
    ) -> Result<(), ExtensionMigrationError> {
        let version = m.version;
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| db_error(version, &e))?;
        // A connection with an authorizer must never go back into the pool
        // if this future is dropped half way.
        conn.close_on_drop();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(|e| db_error(version, &e))?;

        let ctx = Box::new(AuthContext {
            prefix: prefix.to_owned(),
            refused: std::sync::atomic::AtomicBool::new(false),
        });
        let ctx_ptr = &*ctx as *const AuthContext as usize;
        install(&mut conn, Some(ctx_ptr), version).await?;
        let run = sqlx::raw_sql(sqlx::AssertSqlSafe(m.sql.clone()))
            .execute(&mut *conn)
            .await;
        install(&mut conn, None, version).await?;

        let refused = ctx.refused.load(std::sync::atomic::Ordering::SeqCst);
        let outcome = match run {
            Err(_) if refused => Err(ExtensionMigrationError::Rejected {
                version,
                reason: "touches objects outside the extension prefix or uses a forbidden feature",
            }),
            Err(e) => Err(db_error(version, &e)),
            Ok(_) => foreign_keys_stay_inside(&mut conn, prefix, version).await,
        };
        let outcome = match outcome {
            Ok(()) => record_and_commit(&mut conn, history, m).await,
            Err(e) => Err(e),
        };
        if outcome.is_err() {
            // Best effort: the connection is closed on drop anyway.
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        outcome
    }
}

async fn install(
    conn: &mut sqlx::SqliteConnection,
    ctx: Option<usize>,
    version: i64,
) -> Result<(), ExtensionMigrationError> {
    let mut handle = conn
        .lock_handle()
        .await
        .map_err(|e| db_error(version, &e))?;
    let raw = handle.as_raw_handle().as_ptr();
    // SAFETY: `raw` is the live connection handle, held under sqlx's lock.
    // The context outlives the installation (see `apply_extension_migration`).
    let rc = unsafe {
        match ctx {
            Some(p) => {
                libsqlite3_sys::sqlite3_set_authorizer(raw, Some(authorizer), p as *mut c_void)
            }
            None => libsqlite3_sys::sqlite3_set_authorizer(raw, None, std::ptr::null_mut()),
        }
    };
    if rc != libsqlite3_sys::SQLITE_OK {
        return Err(ExtensionMigrationError::Database {
            version,
            code: rc.to_string(),
        });
    }
    Ok(())
}

async fn foreign_keys_stay_inside(
    conn: &mut sqlx::SqliteConnection,
    prefix: &str,
    version: i64,
) -> Result<(), ExtensionMigrationError> {
    let like = format!("{}%", prefix.replace('_', "\\_"));
    let outside: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master m, pragma_foreign_key_list(m.name) f \
         WHERE m.type = 'table' AND m.name LIKE ?1 ESCAPE '\\' \
         AND f.\"table\" NOT LIKE ?1 ESCAPE '\\'",
    )
    .bind(&like)
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| db_error(version, &e))?;
    if outside > 0 {
        return Err(ExtensionMigrationError::Rejected {
            version,
            reason: "a foreign key points outside the extension prefix",
        });
    }
    Ok(())
}

async fn record_and_commit(
    conn: &mut sqlx::SqliteConnection,
    history: &str,
    m: &ExtensionMigration,
) -> Result<(), ExtensionMigrationError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {history} (version, description, checksum) VALUES (?1, ?2, ?3)"
    )))
    .bind(m.version)
    .bind(&m.description)
    .bind(checksum(&m.sql))
    .execute(&mut *conn)
    .await
    .map_err(|e| db_error(m.version, &e))?;
    sqlx::query("COMMIT")
        .execute(&mut *conn)
        .await
        .map_err(|e| db_error(m.version, &e))?;
    Ok(())
}

#[cfg(test)]
mod permitted_tests {
    use super::*;

    const P: &str = "ext_demo_";

    #[test]
    fn own_objects_pass_foreign_ones_fail() {
        assert!(permitted(P, CREATE_TABLE, Some("ext_demo_a"), None));
        assert!(!permitted(P, CREATE_TABLE, Some("servers"), None));
        assert!(permitted(
            P,
            CREATE_INDEX,
            Some("ext_demo_i"),
            Some("ext_demo_a")
        ));
        // an own-named index on a core table
        assert!(!permitted(
            P,
            CREATE_INDEX,
            Some("ext_demo_i"),
            Some("servers")
        ));
        assert!(!permitted(P, DROP_TABLE, Some("servers"), None));
        assert!(!permitted(P, ALTER_TABLE, Some("main"), Some("servers")));
        assert!(permitted(P, ALTER_TABLE, Some("main"), Some("EXT_DEMO_A")));
        assert!(!permitted(P, READ, Some("credentials"), Some("secret")));
        assert!(!permitted(P, 19 /* PRAGMA */, Some("key"), None));
        assert!(!permitted(P, 24 /* ATTACH */, Some("x.db"), None));
        assert!(!permitted(
            P,
            7, /* CREATE_TRIGGER */
            Some("ext_demo_t"),
            Some("ext_demo_a")
        ));
        assert!(!permitted(P, FUNCTION, None, Some("load_extension")));
    }
}
