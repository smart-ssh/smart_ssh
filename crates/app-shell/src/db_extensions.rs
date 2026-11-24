//! Issue #462 (Spec 0038, Abschnitt 2a): database extensions from `Wiring`.
//!
//! Called from `open_and_assemble` — the one place that exists only once the
//! database is open: at startup in keychain mode, after the unlock in
//! master-password mode. Nothing here runs while the app is locked.

use std::sync::Arc;

use persistence_sqlite::{
    ExtensionDb, ExtensionMigrationError, ExtensionRegistration, SqliteProfileStore,
};

/// A `Wiring` slot: the extension's schema and the callback that receives
/// its handle once the migrations ran.
#[derive(Clone)]
pub struct DbExtension {
    pub registration: ExtensionRegistration,
    /// Called at most once per unlock with the handle; not called when the
    /// extension did not start.
    pub on_ready: Arc<dyn Fn(ExtensionDb) + Send + Sync>,
}

/// Runs every registration; an extension that fails (invalid, rejected or
/// a database error) is skipped and the core carries on. Returns the
/// prefixes of the extensions that started.
pub(crate) async fn start_db_extensions(
    store: &SqliteProfileStore,
    extensions: &[DbExtension],
) -> Vec<String> {
    let mut started: Vec<String> = Vec::new();
    for ext in extensions {
        let prefix = ext.registration.prefix.as_str();
        // Only the prefix and the error category are logged: never SQL text
        // or values of the extension.
        let result = if started.iter().any(|p| p == prefix) {
            Err(ExtensionMigrationError::InvalidRegistration(
                "two extensions registered the same prefix",
            ))
        } else {
            store.run_extension_migrations(&ext.registration).await
        };
        match result {
            Ok(db) => {
                started.push(prefix.to_owned());
                tracing::info!(prefix, "database extension started");
                (ext.on_ready)(db);
            }
            Err(err) => {
                tracing::warn!(
                    prefix,
                    error = %err,
                    "database extension not started; the core continues without it"
                );
            }
        }
    }
    started
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence_sqlite::ExtensionMigration;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ext(prefix: &str, sql: &str, calls: Arc<AtomicUsize>) -> DbExtension {
        DbExtension {
            registration: ExtensionRegistration {
                prefix: prefix.to_owned(),
                migrations: vec![ExtensionMigration {
                    version: 1,
                    description: "init".into(),
                    sql: sql.to_owned(),
                }],
            },
            on_ready: Arc::new(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
            }),
        }
    }

    #[tokio::test]
    async fn test_a_failing_extension_is_skipped_and_the_others_start() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap();
        let (bad, good, dup) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        let started = start_db_extensions(
            &store,
            &[
                ext("ext_bad_", "DROP TABLE servers;", bad.clone()),
                ext(
                    "ext_good_",
                    "CREATE TABLE ext_good_t (id INTEGER);",
                    good.clone(),
                ),
                ext(
                    "ext_good_",
                    "CREATE TABLE ext_good_u (id INTEGER);",
                    dup.clone(),
                ),
            ],
        )
        .await;
        assert_eq!(started, vec!["ext_good_".to_owned()]);
        assert_eq!(
            (
                bad.load(Ordering::SeqCst),
                good.load(Ordering::SeqCst),
                dup.load(Ordering::SeqCst)
            ),
            (0, 1, 0)
        );
    }

    // Issue #785: the handle given to `on_ready` reaches the namespaced store.
    #[tokio::test]
    async fn test_kv_store_from_on_ready_is_bound_to_own_namespace() {
        use secrecy::{ExposeSecret, SecretSlice};
        use std::sync::Mutex;

        let dir = tempfile::tempdir().unwrap();
        let store = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap();
        let got: Arc<Mutex<Vec<ExtensionDb>>> = Arc::default();
        let mk = |prefix: &str, table: &str| {
            let got = got.clone();
            DbExtension {
                registration: ExtensionRegistration {
                    prefix: prefix.to_owned(),
                    migrations: vec![ExtensionMigration {
                        version: 1,
                        description: "init".into(),
                        sql: format!("CREATE TABLE {table} (id INTEGER);"),
                    }],
                },
                on_ready: Arc::new(move |db| got.lock().unwrap().push(db)),
            }
        };
        let started = start_db_extensions(
            &store,
            &[mk("ext_one_", "ext_one_t"), mk("ext_two_", "ext_two_t")],
        )
        .await;
        assert_eq!(started.len(), 2);
        let dbs = got.lock().unwrap().clone();
        let (a, b) = (dbs[0].kv_store(), dbs[1].kv_store());

        a.set("plain", b"v").await.unwrap();
        a.set_secret("tok", &SecretSlice::from(b"s3cret".to_vec()))
            .await
            .unwrap();
        assert_eq!(a.get("plain").await.unwrap(), Some(b"v".to_vec()));
        assert_eq!(
            a.get_secret("tok").await.unwrap().unwrap().expose_secret(),
            b"s3cret"
        );
        // Secrets: not returned by get, not listed, not in Debug.
        assert_eq!(a.get("tok").await.unwrap(), None);
        assert_eq!(a.list_keys("").await.unwrap(), vec!["plain".to_owned()]);
        assert!(!format!("{a:?}").contains("s3cret"));

        // The other namespace sees nothing and cannot delete our rows.
        assert_eq!(b.get("plain").await.unwrap(), None);
        assert!(b.get_secret("tok").await.unwrap().is_none());
        assert!(b.list_keys("").await.unwrap().is_empty());
        assert!(!b.delete("plain").await.unwrap());
        assert!(a.delete("plain").await.unwrap());
        assert!(a.delete("tok").await.unwrap());
    }
}
