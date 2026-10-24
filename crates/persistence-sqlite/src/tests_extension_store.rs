//! Issue #673: namespaced key-value and secret store.

use secrecy::{ExposeSecret, SecretSlice};

use crate::{ExtensionStoreError, SqliteProfileStore};

async fn store() -> (SqliteProfileStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
        .await
        .unwrap();
    (store, dir)
}

#[tokio::test]
async fn test_round_trip_and_namespace_isolation() {
    let (s, _d) = store().await;
    let ext = s.extension_store();
    let a = ext.for_namespace("ext_a_").unwrap();
    let b = ext.for_namespace("ext_b_").unwrap();
    a.set("k1", b"one").await.unwrap();
    a.set("k2", b"two").await.unwrap();
    a.set("other", b"x").await.unwrap();
    assert_eq!(a.get("k1").await.unwrap().as_deref(), Some(&b"one"[..]));
    assert_eq!(a.list_keys("k").await.unwrap(), vec!["k1", "k2"]);
    assert_eq!(b.get("k1").await.unwrap(), None);
    assert!(b.list_keys("").await.unwrap().is_empty());
    assert!(!b.delete("k1").await.unwrap());
    assert_eq!(a.get("k1").await.unwrap().as_deref(), Some(&b"one"[..]));
    assert!(a.delete("k1").await.unwrap());
    assert_eq!(a.get("k1").await.unwrap(), None);
    a.set("k2", b"new").await.unwrap();
    assert_eq!(a.get("k2").await.unwrap().as_deref(), Some(&b"new"[..]));
}

#[tokio::test]
async fn test_invalid_namespace_and_key() {
    let (s, _d) = store().await;
    let ext = s.extension_store();
    for ns in ["", "a", "ext_", "ext_A_", "ext_a_b_", "ext_a"] {
        assert_eq!(
            ext.for_namespace(ns).unwrap_err(),
            ExtensionStoreError::InvalidNamespace
        );
    }
    let a = ext.for_namespace("ext_a_").unwrap();
    assert_eq!(a.set("", b"x").await, Err(ExtensionStoreError::InvalidKey));
}

#[tokio::test]
async fn test_secret_not_returned_by_get_nor_listed_nor_debug() {
    let (s, _d) = store().await;
    let a = s.extension_store().for_namespace("ext_a_").unwrap();
    let secret = SecretSlice::from(b"hunter2".to_vec());
    a.set_secret("tok", &secret).await.unwrap();
    assert_eq!(a.get("tok").await.unwrap(), None);
    assert!(a.list_keys("").await.unwrap().is_empty());
    let got = a.get_secret("tok").await.unwrap().unwrap();
    assert_eq!(got.expose_secret(), b"hunter2");
    assert!(!format!("{got:?}").contains("hunter2"));
    assert!(!format!("{a:?}").contains("tok"));
    // A plain key is not a secret.
    a.set("plain", b"v").await.unwrap();
    assert!(a.get_secret("plain").await.unwrap().is_none());
    assert!(a.delete("tok").await.unwrap());
    assert!(a.get_secret("tok").await.unwrap().is_none());
}

#[tokio::test]
async fn test_limits() {
    let (s, _d) = store().await;
    let a = s.extension_store().for_namespace("ext_a_").unwrap();
    a.set("big", &vec![0u8; crate::extension_store::MAX_VALUE_BYTES])
        .await
        .unwrap();
    assert_eq!(
        a.set(
            "big2",
            &vec![0u8; crate::extension_store::MAX_VALUE_BYTES + 1]
        )
        .await,
        Err(ExtensionStoreError::ValueTooLarge)
    );
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 9999) \
         INSERT INTO ext_kv SELECT 'ext_a_', 'k' || i, x'00', 0, '' FROM n",
    )
    .execute(&s.pool)
    .await
    .unwrap();
    // 1 + 9999 = 10 000 keys: full. Overwriting is fine, a new key is not.
    a.set("big", b"small").await.unwrap();
    assert_eq!(
        a.set("new", b"x").await,
        Err(ExtensionStoreError::TooManyKeys)
    );
    assert_eq!(
        a.set_secret("new", &SecretSlice::from(vec![1])).await,
        Err(ExtensionStoreError::TooManyKeys)
    );
    // Another namespace has its own budget.
    let b = s.extension_store().for_namespace("ext_b_").unwrap();
    b.set("new", b"x").await.unwrap();
}

#[tokio::test]
async fn test_table_is_excluded_from_backups_by_prefix() {
    // Spec 0109, C2 excludes every table named `ext_*`; the store's table
    // must fall under that rule.
    let (s, _d) = store().await;
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'ext\\_%' ESCAPE '\\'",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    assert_eq!(names, vec!["ext_kv"]);
}
