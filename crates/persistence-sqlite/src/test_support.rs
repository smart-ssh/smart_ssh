//! Issue #15: Testhelfer für die End-zu-End-Migrationstests — die
//! Release-Fixtures, ein Abbild von Schema und Inhalt einer Datenbankdatei
//! und ein Weg, einer Datenbank eine Migration zu geben, die dieser Build
//! nicht kennt.
//!
//! **Hinter `test-support`**, wie `SqliteProfileStore::connect_plaintext`:
//! Jeder Helfer hier öffnet eine Datenbankdatei direkt, am Startablauf
//! vorbei. `app-logic` schaltet das Feature nur unter `[dev-dependencies]`
//! ein, und der Schritt `cargo build --workspace` des Gates fängt einen
//! Produktivaufruf ab.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::Connection;

use ssh_manager_core::crypto::DatabaseKey;

/// Issue #113: die vier Spalten, die bis dahin feldweise verschlüsselt
/// geschrieben wurden und beim Start auf Klartext umgestellt werden.
pub const FIELD_ENCRYPTED_COLUMNS: [(&str, &str); 4] = [
    ("chat_messages", "content"),
    ("ledger_entries", "content"),
    ("prompt_history", "content"),
    ("chat_sessions", "summary_text"),
];

/// Der Wurzelschlüssel K, unter dem jede Release-Fixture ihren feldweise
/// verschlüsselten Inhalt (Chat, Prompt-Historie, Ledger) schreibt — und ab
/// den SQLCipher-Releases der K, aus dem der Datenbankschlüssel der Fixture
/// abgeleitet ist. Kein Geheimnis; er existiert nur in Test-Fixtures.
pub const RELEASE_FIXTURE_ROOT_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// Wie ein Release seine Datenbankdatei geschrieben hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureEncryption {
    /// Releases vor SQLCipher (bis einschließlich 0.5.2).
    Plaintext,
    /// Releases mit SQLCipher: Der Datenbankschlüssel ist
    /// `DatabaseKey::from_root_key(&RELEASE_FIXTURE_ROOT_KEY)`.
    Sqlcipher,
}

/// Eine eingecheckte Datenbankdatei, geschrieben vom veröffentlichten Build.
#[derive(Debug, Clone, Copy)]
pub struct ReleaseFixture {
    /// Das Release, das die Datei geschrieben hat, z. B. `"0.5.2"`.
    pub release: &'static str,
    /// Dateiname unter `tests/fixtures/releases/`.
    pub file_name: &'static str,
    /// Höchste Migrationsversion, die das Release mitbrachte.
    pub schema_version: i64,
    pub encryption: FixtureEncryption,
}

impl ReleaseFixture {
    pub fn path(&self) -> PathBuf {
        release_fixture_dir().join(self.file_name)
    }
}

/// Alle Release-Fixtures, älteste zuerst. **Ein Eintrag je Release, das
/// das Schema geändert hat** — wie beim Release einer hinzukommt, steht in
/// `tests/fixtures/releases/README.md`.
pub const RELEASE_FIXTURES: &[ReleaseFixture] = &[ReleaseFixture {
    release: "0.5.2",
    file_name: "v0.5.2.sqlite3",
    schema_version: 14,
    encryption: FixtureEncryption::Plaintext,
}];

/// Die neueste Release-Fixture — die Datenbank, die ein Nutzer beim Update
/// vom aktuellen Release mitbringt.
pub fn current_release_fixture() -> &'static ReleaseFixture {
    RELEASE_FIXTURES
        .last()
        .expect("at least one release fixture is registered")
}

pub fn release_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/releases")
}

/// Die höchste Migrationsversion, die dieser Build kennt.
pub fn max_known_migration_version() -> i64 {
    crate::SqliteProfileStore::max_known_migration_version()
}

/// Alle Migrationen dieses Builds als `(Version, Dateiname)`, gelesen aus
/// `migrations/` dieser Crate, nach Version sortiert.
pub fn migration_files() -> Vec<(i64, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<(i64, String)> = std::fs::read_dir(&dir)
        .expect("migrations/ is readable")
        .map(|entry| {
            entry
                .expect("directory entry is readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".sql"))
        .map(|name| {
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

/// Öffnet `path`, ohne etwas zu migrieren — verschlüsselt, wenn `key` gesetzt ist.
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

/// Windows checkt die `.sql`-Migrationen mit CRLF aus; die Prüfsummen, die
/// der Build einbettet, weichen dann von denen einer unter LF geschriebenen
/// Fixture ab. Richtet die Prüfsummen der übergebenen **Kopie** auf diesen
/// Build aus — derselbe Schritt wie
/// `tests_fixture_t0::align_migration_checksums_to_current_build`, für
/// Klartext- und verschlüsselte Kopien. Ändert nur vorhandene Zeilen.
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

/// Die angewandten Migrationsversionen einer Datei, aufsteigend.
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

/// Wendet die Migrationen dieses Builds bis einschließlich `target_version`
/// an — so lässt sich Schritt für Schritt migrieren, anders als mit
/// `sqlx::migrate!()`, das immer bis zur neuesten läuft.
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

/// Gibt der Datenbank eine Migration `version`, die dieser Build nicht
/// kennt — genau das, was ein neueres Release hinterlässt. Angewandt über
/// einen echten `sqlx`-Migrator (die Migrationen dieses Builds plus die
/// zusätzliche), damit `_sqlx_migrations` eine echte Zeile mit Prüfsumme
/// trägt und keine von Hand eingetragene.
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

/// Ein temporäres Verzeichnis ohne `tempfile` — das ist nur
/// Dev-Abhängigkeit und fehlt im `test-support`-Build für `app-logic`.
fn tempfile_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "smart-ssh-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("temporary directory can be created");
    dir
}

/// Schema und Inhalt einer Datenbankdatei, unabhängig vom Layout auf der
/// Platte — zwei Abbilder sind genau dann gleich, wenn jedes
/// Schema-Objekt, jede Zeile jeder Tabelle (auch `_sqlx_migrations` mit
/// ihren Zeitstempeln) und `user_version` gleich sind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseSnapshot {
    /// `type name: sql` für jeden Eintrag in `sqlite_master`, sortiert.
    pub schema: Vec<String>,
    pub user_version: i64,
    pub tables: BTreeMap<String, TableSnapshot>,
}

/// Eine Tabelle: ihre Spalten in Deklarationsreihenfolge und jede Zeile als
/// `quote()`-Werte je Spalte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSnapshot {
    pub columns: Vec<String>,
    /// Sortiert, damit das Abbild nicht von der Zeilenreihenfolge abhängt.
    pub rows: Vec<BTreeMap<String, String>>,
}

impl TableSnapshot {
    /// Die Zeilen, reduziert auf `columns` und sortiert — für den Vergleich
    /// einer Tabelle vor und nach einer Migration, die Spalten hinzufügt.
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
    /// Prüft, dass `later` alles noch enthält, was `self` enthielt: jede
    /// Tabelle, jede Spalte und jede Zeile mit unveränderten Werten in
    /// diesen Spalten. Neue Tabellen, Spalten und Zeilen sind erlaubt — die
    /// bringt eine Migration mit.
    pub fn assert_preserved_in(&self, later: &DatabaseSnapshot, step: &str) {
        self.assert_preserved_in_except(later, step, &[]);
    }

    /// Wie [`Self::assert_preserved_in`], aber die Werte der Spalten in
    /// `changed` (je `(Tabelle, Spalte)`) dürfen sich ändern — für einen
    /// Schritt, der genau diese Spalten bewusst umschreibt (Issue #113: die
    /// Umstellung der feldweise verschlüsselten Spalten auf Klartext).
    /// Spalten und Zeilen dürfen trotzdem nicht verschwinden; das prüft der
    /// Aufrufer für die umgeschriebenen Spalten selbst.
    pub fn assert_preserved_in_except(
        &self,
        later: &DatabaseSnapshot,
        step: &str,
        changed: &[(&str, &str)],
    ) {
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
                // Hier fügt jeder Schritt absichtlich eine Zeile hinzu; die
                // vorhandenen müssen unverändert bleiben.
                let after_rows = after.rows_projected(&before.columns);
                for row in before.rows_projected(&before.columns) {
                    assert!(
                        after_rows.contains(&row),
                        "{step}: _sqlx_migrations lost or changed {row:?}"
                    );
                }
                continue;
            }
            let compared: Vec<String> = before
                .columns
                .iter()
                .filter(|c| !changed.contains(&(name.as_str(), c.as_str())))
                .cloned()
                .collect();
            assert_eq!(
                before.rows_projected(&compared),
                after.rows_projected(&compared),
                "{step}: rows of table {name} changed"
            );
        }
    }

    /// Die Werte einer Spalte in `quote()`-Darstellung (s. [`snapshot_database`]),
    /// sortiert. Ein Blob beginnt mit `X'`, Text mit `'`, `NULL` ist `NULL`.
    pub fn column_values(&self, table: &str, column: &str) -> Vec<String> {
        let mut values: Vec<String> = self
            .tables
            .get(table)
            .map(|t| {
                t.rows
                    .iter()
                    .filter_map(|r| r.get(column).cloned())
                    .collect()
            })
            .unwrap_or_default();
        values.sort();
        values
    }

    /// Anzahl der Zeilen in `table`, 0, wenn es die Tabelle nicht gibt.
    pub fn row_count(&self, table: &str) -> usize {
        self.tables.get(table).map_or(0, |t| t.rows.len())
    }
}

/// Liest das Abbild von `path`, ohne zu migrieren oder zu schreiben.
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
        // `quote()` stellt jede Speicherklasse eindeutig dar (Text in
        // Anführungszeichen, Blobs als X'..', NULL als NULL) — eine
        // Typänderung erscheint so als Unterschied, statt in einer
        // Umwandlung zu verschwinden.
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

/// Jede Datei in `dir` mit vollem Inhalt — für den Nachweis, dass ein
/// gescheiterter Start das Datenverzeichnis Byte für Byte unverändert
/// lässt.
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

/// Nur die Dateinamen in `dir`, sortiert — für das Warten darauf, dass
/// SQLite seine Nebendateien entfernt hat. Liest keinen Inhalt, verträgt
/// also eine Datei, die zwischen Auflisten und Lesen verschwindet.
pub fn directory_file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("directory is readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Issue #113: schreibt eine Prompt-Historie-Zeile mit dem rohen Wert `blob`
/// — so, wie die Stores vor Issue #113 eine feldweise verschlüsselte Zeile
/// geschrieben haben (`nonce || ciphertext` als Blob). Für Tests außerhalb
/// dieser Crate, die keinen eigenen SQL-Zugang haben.
pub async fn insert_legacy_prompt_history_row(
    store: &crate::SqliteProfileStore,
    server_id: &ssh_manager_core::shared::ServerId,
    blob: Vec<u8>,
) {
    sqlx::query(
        "INSERT INTO prompt_history (id, server_id, content, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(server_id.0.to_string())
    .bind(blob)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&store.pool)
    .await
    .expect("legacy prompt_history row can be inserted");
}

/// Issue #113: Wie viele Werte der vier früher feldweise verschlüsselten
/// Spalten noch Blobs (also Chiffrate) sind.
pub async fn field_encrypted_blob_count(store: &crate::SqliteProfileStore) -> i64 {
    let mut total = 0;
    for (table, column) in FIELD_ENCRYPTED_COLUMNS {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE typeof({column}) = 'blob'");
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(&store.pool)
            .await
            .expect("column is readable");
        total += n;
    }
    total
}
