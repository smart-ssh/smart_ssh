//! Issue #462 (Spec 0038, 2a): extension-owned migrations.

use crate::{
    ExtensionMigration, ExtensionMigrationError, ExtensionRegistration, SqliteProfileStore,
};

async fn store() -> (SqliteProfileStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
        .await
        .unwrap();
    (store, dir)
}

fn mig(version: i64, sql: &str) -> ExtensionMigration {
    ExtensionMigration {
        version,
        description: format!("m{version}"),
        sql: sql.to_owned(),
    }
}

fn reg(prefix: &str, migrations: Vec<ExtensionMigration>) -> ExtensionRegistration {
    ExtensionRegistration {
        prefix: prefix.to_owned(),
        migrations,
    }
}

async fn schema(store: &SqliteProfileStore) -> Vec<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT name, COALESCE(sql, '') FROM sqlite_master ORDER BY name",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap()
}

async fn core_snapshot(store: &SqliteProfileStore) -> Vec<(String, String)> {
    schema(store)
        .await
        .into_iter()
        .filter(|(n, _)| !n.starts_with("ext_"))
        .collect()
}

#[tokio::test]
async fn test_two_migrations_run_once_in_order_with_own_history() {
    let (store, _dir) = store().await;
    let r = reg(
        "ext_demo_",
        vec![
            mig(
                1,
                "CREATE TABLE ext_demo_items (id INTEGER PRIMARY KEY, n TEXT);",
            ),
            mig(
                2,
                "CREATE INDEX ext_demo_items_n ON ext_demo_items (n); \
                 INSERT INTO ext_demo_items (n) VALUES ('a');",
            ),
        ],
    );
    let db = store.run_extension_migrations(&r).await.unwrap();
    // idempotent: the second run applies nothing (an INSERT would duplicate)
    store.run_extension_migrations(&r).await.unwrap();

    let history: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version, description FROM ext_demo_schema_migrations ORDER BY applied_at, version",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(history, vec![(1, "m1".into()), (2, "m2".into())]);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ext_demo_items")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(rows, 1);
    // the core history is not touched
    let core_versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(
        core_versions,
        crate::store::SqliteProfileStore::max_known_migration_version()
    );
}

#[tokio::test]
async fn test_migration_outside_prefix_is_rejected_and_core_data_unchanged() {
    for evil in [
        "DROP TABLE servers;",
        "ALTER TABLE servers ADD COLUMN x TEXT;",
        "CREATE TABLE other_table (id INTEGER);",
        "CREATE INDEX ext_demo_i ON servers (id);",
        "DELETE FROM servers;",
        "CREATE TABLE ext_demo_a AS SELECT * FROM servers;",
        "CREATE TABLE ext_demo_a (s TEXT REFERENCES servers(id));",
        "CREATE VIEW ext_demo_v AS SELECT 1;",
        "CREATE TRIGGER ext_demo_t AFTER INSERT ON servers BEGIN SELECT 1; END;",
        "PRAGMA writable_schema = 1;",
        "ATTACH DATABASE ':memory:' AS x;",
        "COMMIT;",
    ] {
        let (store, _dir) = store().await;
        let before = core_snapshot(&store).await;
        // a good first migration stays applied, the bad second one does not
        let r = reg(
            "ext_demo_",
            vec![
                mig(1, "CREATE TABLE ext_demo_ok (id INTEGER PRIMARY KEY);"),
                mig(2, evil),
            ],
        );
        let err = store.run_extension_migrations(&r).await.unwrap_err();
        assert!(
            matches!(err, ExtensionMigrationError::Rejected { version: 2, .. }),
            "{evil}: {err}"
        );
        assert!(!err.to_string().contains("servers"), "no SQL in the error");
        assert_eq!(core_snapshot(&store).await, before, "{evil}");
        let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ext_demo_schema_migrations")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(applied, 1, "{evil}");
        // the pool is usable and carries no authorizer afterwards
        sqlx::query("CREATE TABLE core_probe (id INTEGER)")
            .execute(&store.pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_no_extension_leaves_the_schema_untouched() {
    let (store, _dir) = store().await;
    assert!(schema(&store)
        .await
        .iter()
        .all(|(n, _)| !n.starts_with("ext_")));
    let before = schema(&store).await;
    // an empty registration is the only thing that creates objects: its
    // history table, in the extension's own namespace
    assert_eq!(core_snapshot(&store).await, before);
}

#[tokio::test]
async fn test_invalid_registrations_and_edited_history() {
    let (store, _dir) = store().await;
    for p in [
        "servers_",
        "ext_",
        "ext__",
        "ext_Demo_",
        "ext_demo",
        "ext_a_b_",
    ] {
        assert!(
            matches!(
                store.run_extension_migrations(&reg(p, vec![])).await,
                Err(ExtensionMigrationError::InvalidRegistration(_))
            ),
            "{p}"
        );
    }
    let unordered = reg("ext_demo_", vec![mig(2, "SELECT 1;"), mig(1, "SELECT 1;")]);
    assert!(matches!(
        store.run_extension_migrations(&unordered).await,
        Err(ExtensionMigrationError::InvalidRegistration(_))
    ));
    store
        .run_extension_migrations(&reg(
            "ext_demo_",
            vec![mig(1, "CREATE TABLE ext_demo_a (id INTEGER);")],
        ))
        .await
        .unwrap();
    let edited = reg(
        "ext_demo_",
        vec![mig(1, "CREATE TABLE ext_demo_a (id INTEGER, x TEXT);")],
    );
    assert_eq!(
        store.run_extension_migrations(&edited).await.unwrap_err(),
        ExtensionMigrationError::HistoryMismatch { version: 1 }
    );
}
