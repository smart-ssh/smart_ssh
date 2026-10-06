//! Issue #15: test helpers for the end-to-end migration tests — the release
//! fixtures, a schema-and-data snapshot of a database file, and a way to give
//! a database a migration that this build does not know.
//!
//! **Behind `test-support`**, like `SqliteProfileStore::connect_plaintext`:
//! every helper here opens a database file directly, past the startup path.
//! `app-logic` enables the feature only under `[dev-dependencies]`, so the
//! `cargo build --workspace` step of the gate catches a production call.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::Connection;

use ssh_manager_core::crypto::DatabaseKey;

/// The root key K under which every release fixture's field-encrypted
/// content (chat, prompt history, ledger) is written — and, for releases
/// from SQLCipher on, the K from which the fixture's database key is
/// derived. Not a secret; it only exists in test fixtures.
pub const RELEASE_FIXTURE_ROOT_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// How a release wrote its database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureEncryption {
    /// Releases before SQLCipher (up to and including 0.5.2).
    Plaintext,
    /// Releases with SQLCipher: the database key is
    /// `DatabaseKey::from_root_key(&RELEASE_FIXTURE_ROOT_KEY)`.
    Sqlcipher,
}

/// One checked-in database file, written by a released build.
#[derive(Debug, Clone, Copy)]
pub struct ReleaseFixture {
    /// The release that wrote the file, e.g. `"0.5.2"`.
    pub release: &'static str,
    /// File name under `tests/fixtures/releases/`.
    pub file_name: &'static str,
    /// Highest migration version the release shipped.
    pub schema_version: i64,
    pub encryption: FixtureEncryption,
}

impl ReleaseFixture {
    pub fn path(&self) -> PathBuf {
        release_fixture_dir().join(self.file_name)
    }
}

/// Every release fixture, oldest first. **One entry per release that
/// changed the schema** — see `tests/fixtures/releases/README.md` for how
/// to add one when a release is cut.
pub const RELEASE_FIXTURES: &[ReleaseFixture] = &[ReleaseFixture {
    release: "0.5.2",
    file_name: "v0.5.2.sqlite3",
    schema_version: 14,
    encryption: FixtureEncryption::Plaintext,
}];

/// The newest release fixture — the database a user upgrading from the
/// current release brings along.
pub fn current_release_fixture() -> &'static ReleaseFixture {
    RELEASE_FIXTURES
        .last()
        .expect("at least one release fixture is registered")
}

pub fn release_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/releases")
}

/// The highest migration version this build knows.
pub fn max_known_migration_version() -> i64 {
    crate::SqliteProfileStore::max_known_migration_version()
}

/// Every migration of this build, as `(version, file name)`, read from the
/// crate's `migrations/` directory, sorted by version.
pub fn migration_files() -> Vec<(i64, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<(i64, String)> = std::fs::read_dir(&dir)
        .expect("migrations/ is readable")
        .map(|entry| {
            let name = entry
                .expect("directory entry is readable")
                .file_name()
                .to_string_lossy()
                .into_owned();
            let version = name
                .split('_')
                .next()
                .and_then(|prefix| prefix.parse::<i64>().ok())
                .unwrap_or_else(|| panic!("migration file without a version prefix: {name}"));
            (version, name)
        })
        .collect();
    files.sort();
    files
}

/// Opens `path` without migrating anything — encrypted if `key` is given.
pub async fn open_raw(path: &Path, key: Option<&DatabaseKey>) -> SqliteConnection {
    let options = match key {
        Some(key) => crate::encryption::encrypted_connect_options(path, key),
        None => SqliteConnectOptions::new().filename(path),
    }
    .create_if_missing(false);
    SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|err| panic!("{} cannot be opened: {err}", path.display()))
}

/// Windows checks the `.sql` migrations out with CRLF, so the checksums the
/// build embeds differ from those in a fixture written under LF. Aligns the
/// checksums of the given **copy** to this build — the same step as
/// `tests_fixture_t0::align_migration_checksums_to_current_build`, for
/// plaintext and encrypted copies. Touches only rows that already exist.
pub async fn align_migration_checksums(copy_path: &Path, key: Option<&DatabaseKey>) {
    let mut conn = open_raw(copy_path, key).await;
    for migration in sqlx::migrate!().iter() {
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(migration.checksum.as_ref())
            .bind(migration.version)
            .execute(&mut conn)
            .await
            .expect("checksum in _sqlx_migrations can be updated");
    }
    conn.close().await.expect("connection closes");
}

/// The applied migration versions of a file, ascending.
pub async fn applied_migrations(path: &Path, key: Option<&DatabaseKey>) -> Vec<i64> {
    let mut conn = open_raw(path, key).await;
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut conn)
            .await
            .expect("_sqlx_migrations is readable");
    conn.close().await.expect("connection closes");
    versions
}

/// Applies this build's migrations up to and including `target_version` —
/// exactly one release step at a time, instead of `sqlx::migrate!()`, which
/// always runs to the newest one.
pub async fn migrate_up_to(path: &Path, key: Option<&DatabaseKey>, target_version: i64) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let subset = tempfile_dir("migrations-subset");
    for (version, name) in migration_files() {
        if version <= target_version {
            std::fs::copy(source.join(&name), subset.join(&name))
                .expect("migration file can be copied");
        }
    }
    let mut conn = open_raw(path, key).await;
    sqlx::migrate::Migrator::new(subset.as_path())
        .await
        .expect("migrator can be built from the copied files")
        .run(&mut conn)
        .await
        .unwrap_or_else(|err| panic!("migrating to version {target_version} failed: {err}"));
    conn.close().await.expect("connection closes");
    let _ = std::fs::remove_dir_all(&subset);
}

/// Gives the database a migration with `version` that this build does not
/// know — exactly what a newer release leaves behind. Applied through a
/// real `sqlx` migrator (this build's migrations plus the extra one), so
/// `_sqlx_migrations` carries a genuine row with checksum, not a hand-made
/// one.
pub async fn apply_future_migration(
    path: &Path,
    key: Option<&DatabaseKey>,
    version: i64,
    description: &str,
    sql: &str,
) {
    assert!(
        version > max_known_migration_version(),
        "a future migration must be newer than every migration of this build"
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let with_future = tempfile_dir("migrations-future");
    for (_, name) in migration_files() {
        std::fs::copy(source.join(&name), with_future.join(&name))
            .expect("migration file can be copied");
    }
    std::fs::write(
        with_future.join(format!("{version:04}_{description}.sql")),
        sql,
    )
    .expect("future migration can be written");
    let mut conn = open_raw(path, key).await;
    sqlx::migrate::Migrator::new(with_future.as_path())
        .await
        .expect("migrator can be built")
        .run(&mut conn)
        .await
        .expect("the future migration applies");
    conn.close().await.expect("connection closes");
    let _ = std::fs::remove_dir_all(&with_future);
}

/// A temporary directory without a `tempfile` dependency in this crate's
/// `test-support` build (`tempfile` is a dev-dependency only).
fn tempfile_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "smart-ssh-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("temporary directory can be created");
    dir
}

/// Schema and content of a database file, independent of how the file is
/// laid out on disk — two snapshots are equal exactly when every schema
/// object, every row of every table (including `_sqlx_migrations` with its
/// timestamps) and `user_version` are equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseSnapshot {
    /// `type name: sql` for every entry of `sqlite_master`, sorted.
    pub schema: Vec<String>,
    pub user_version: i64,
    pub tables: BTreeMap<String, TableSnapshot>,
}

/// One table: its columns in declaration order and every row as
/// `quote()`d values per column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSnapshot {
    pub columns: Vec<String>,
    /// Sorted, so the snapshot does not depend on row order.
    pub rows: Vec<BTreeMap<String, String>>,
}

impl TableSnapshot {
    /// The rows, reduced to `columns` and sorted — to compare a table
    /// before and after a migration that added columns.
    pub fn rows_projected(&self, columns: &[String]) -> Vec<Vec<String>> {
        let mut rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|c| {
                        row.get(c)
                            .cloned()
                            .unwrap_or_else(|| "<column missing>".to_string())
                    })
                    .collect()
            })
            .collect();
        rows.sort();
        rows
    }
}

impl DatabaseSnapshot {
    /// Asserts that `later` still holds everything `self` held: every
    /// table, every column, and every row with unchanged values in those
    /// columns. New tables, new columns and new rows are allowed — that is
    /// what a migration adds.
    pub fn assert_preserved_in(&self, later: &DatabaseSnapshot, step: &str) {
        for (name, before) in &self.tables {
            let after = later
                .tables
                .get(name)
                .unwrap_or_else(|| panic!("{step}: table {name} disappeared"));
            let missing: BTreeSet<&String> = before
                .columns
                .iter()
                .filter(|c| !after.columns.contains(c))
                .collect();
            assert!(
                missing.is_empty(),
                "{step}: table {name} lost columns {missing:?}"
            );
            if name == "_sqlx_migrations" {
                // A step adds rows here by design; the rows that were
                // there must stay untouched.
                let after_rows = after.rows_projected(&before.columns);
                for row in before.rows_projected(&before.columns) {
                    assert!(
                        after_rows.contains(&row),
                        "{step}: _sqlx_migrations lost or changed {row:?}"
                    );
                }
                continue;
            }
            assert_eq!(
                before.rows_projected(&before.columns),
                after.rows_projected(&before.columns),
                "{step}: rows of table {name} changed"
            );
        }
    }

    /// Number of rows in `table`, 0 if the table does not exist.
    pub fn row_count(&self, table: &str) -> usize {
        self.tables.get(table).map_or(0, |t| t.rows.len())
    }
}

/// Reads the snapshot of `path`, without migrating or writing anything.
pub async fn snapshot_database(path: &Path, key: Option<&DatabaseKey>) -> DatabaseSnapshot {
    let mut conn = open_raw(path, key).await;

    let schema_rows: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT type, name, sql FROM sqlite_master")
            .fetch_all(&mut conn)
            .await
            .expect("sqlite_master is readable");
    let mut schema: Vec<String> = schema_rows
        .iter()
        .map(|(kind, name, sql)| format!("{kind} {name}: {}", sql.as_deref().unwrap_or("")))
        .collect();
    schema.sort();

    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut conn)
        .await
        .expect("user_version is readable");

    let mut tables = BTreeMap::new();
    for (kind, name, _) in &schema_rows {
        if kind != "table" || name.starts_with("sqlite_") {
            continue;
        }
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(name)
                .fetch_all(&mut conn)
                .await
                .expect("table info is readable");
        // `quote()` renders every storage class unambiguously (text in
        // quotes, blobs as X'..', NULL as NULL), so a type change shows up
        // as a difference instead of being hidden by a conversion.
        let select = columns
            .iter()
            .map(|c| format!("quote({})", quote_identifier(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT {select} FROM {}", quote_identifier(name));
        let raw_rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut conn)
            .await
            .unwrap_or_else(|err| panic!("table {name} is readable: {err}"));
        let mut rows: Vec<BTreeMap<String, String>> = raw_rows
            .iter()
            .map(|row| {
                use sqlx::Row;
                columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.clone(), row.get::<String, _>(i)))
                    .collect()
            })
            .collect();
        rows.sort();
        tables.insert(name.clone(), TableSnapshot { columns, rows });
    }

    conn.close().await.expect("connection closes");
    DatabaseSnapshot {
        schema,
        user_version,
        tables,
    }
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Every file in `dir` with its full content — to prove that a failed start
/// left the data directory byte for byte as it was.
pub fn directory_contents(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("directory is readable")
        .map(|entry| {
            let entry = entry.expect("directory entry is readable");
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).expect("file is readable"),
            )
        })
        .collect()
}
