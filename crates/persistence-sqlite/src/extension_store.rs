//! Issue #673: namespaced key-value and secret store for integrations.
//!
//! Rows live in the core table `ext_kv` inside the encrypted database, so the
//! store is only reachable once the database is open (after unlock). The
//! `ext_` table prefix keeps it out of backups (Spec 0109, C2). Nothing here
//! logs a value, a secret key or a SQL text.

use secrecy::{ExposeSecret, SecretSlice};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use crate::{validate_prefix, SqliteProfileStore};

/// Largest accepted value.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// Largest number of keys (plain and secret) per namespace.
pub const MAX_KEYS_PER_NAMESPACE: usize = 10_000;
/// Largest accepted key.
pub const MAX_KEY_BYTES: usize = 512;

/// Why a store call failed. Carries no keys, values or SQL text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionStoreError {
    /// The namespace does not look like `ext_<name>_`.
    InvalidNamespace,
    /// Empty or longer than [`MAX_KEY_BYTES`].
    InvalidKey,
    /// Value larger than [`MAX_VALUE_BYTES`].
    ValueTooLarge,
    /// The namespace already holds [`MAX_KEYS_PER_NAMESPACE`] keys.
    TooManyKeys,
    /// SQLite refused (the result code only).
    Database { code: String },
}

impl std::fmt::Display for ExtensionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNamespace => write!(f, "invalid extension namespace"),
            Self::InvalidKey => write!(f, "invalid extension store key"),
            Self::ValueTooLarge => write!(f, "extension store value too large"),
            Self::TooManyKeys => write!(f, "extension store key limit reached"),
            Self::Database { code } => write!(f, "extension store failed (SQLite code {code})"),
        }
    }
}

impl std::error::Error for ExtensionStoreError {}

fn db_error(err: &sqlx::Error) -> ExtensionStoreError {
    let code = err
        .as_database_error()
        .and_then(|e| e.code().map(|c| c.into_owned()))
        .unwrap_or_else(|| "unknown".to_owned());
    ExtensionStoreError::Database { code }
}

fn check_key(key: &str) -> Result<(), ExtensionStoreError> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(ExtensionStoreError::InvalidKey);
    }
    Ok(())
}

/// Entry point; hands out per-namespace handles.
#[derive(Clone)]
pub struct ExtensionStore {
    pool: SqlitePool,
}

impl std::fmt::Debug for ExtensionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionStore").finish_non_exhaustive()
    }
}

impl SqliteProfileStore {
    /// The key-value store for integrations. Call only on an opened store
    /// (after unlock in master-password mode).
    pub fn extension_store(&self) -> ExtensionStore {
        ExtensionStore {
            pool: self.pool.clone(),
        }
    }
}

impl ExtensionStore {
    /// A handle bound to one namespace, named like an extension prefix
    /// (`ext_<name>_`). It can only reach that namespace's rows.
    pub fn for_namespace(&self, ns: &str) -> Result<NamespacedStore, ExtensionStoreError> {
        validate_prefix(ns).map_err(|_| ExtensionStoreError::InvalidNamespace)?;
        Ok(NamespacedStore {
            pool: self.pool.clone(),
            ns: ns.to_owned(),
        })
    }
}

/// One namespace of the store.
#[derive(Clone)]
pub struct NamespacedStore {
    pool: SqlitePool,
    ns: String,
}

impl std::fmt::Debug for NamespacedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamespacedStore")
            .field("namespace", &self.ns)
            .finish_non_exhaustive()
    }
}

impl NamespacedStore {
    /// Plain value; `None` for a missing key and for a secret key.
    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ExtensionStoreError> {
        self.fetch(key, false).await
    }

    pub async fn set(&self, key: &str, value: &[u8]) -> Result<(), ExtensionStoreError> {
        self.put(key, value, false).await
    }

    /// Secret value; `None` for a missing key and for a plain key.
    pub async fn get_secret(
        &self,
        key: &str,
    ) -> Result<Option<SecretSlice<u8>>, ExtensionStoreError> {
        Ok(self.fetch(key, true).await?.map(SecretSlice::from))
    }

    pub async fn set_secret(
        &self,
        key: &str,
        value: &SecretSlice<u8>,
    ) -> Result<(), ExtensionStoreError> {
        self.put(key, value.expose_secret(), true).await
    }

    /// Deletes a plain or secret key; returns whether a row was removed.
    pub async fn delete(&self, key: &str) -> Result<bool, ExtensionStoreError> {
        check_key(key)?;
        let res = sqlx::query("DELETE FROM ext_kv WHERE namespace = ? AND key = ?")
            .bind(&self.ns)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| db_error(&e))?;
        Ok(res.rows_affected() > 0)
    }

    /// Plain keys starting with `prefix`, sorted. Secret keys are never listed.
    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, ExtensionStoreError> {
        let rows = sqlx::query(
            "SELECT key FROM ext_kv WHERE namespace = ? AND is_secret = 0 \
             AND substr(key, 1, ?) = ? ORDER BY key",
        )
        .bind(&self.ns)
        .bind(i64::try_from(prefix.chars().count()).unwrap_or(i64::MAX))
        .bind(prefix)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_error(&e))?;
        rows.iter()
            .map(|r| r.try_get::<String, _>(0).map_err(|e| db_error(&e)))
            .collect()
    }

    async fn fetch(&self, key: &str, secret: bool) -> Result<Option<Vec<u8>>, ExtensionStoreError> {
        check_key(key)?;
        let row = sqlx::query(
            "SELECT value FROM ext_kv WHERE namespace = ? AND key = ? AND is_secret = ?",
        )
        .bind(&self.ns)
        .bind(key)
        .bind(i64::from(secret))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_error(&e))?;
        row.map(|r| r.try_get::<Vec<u8>, _>(0).map_err(|e| db_error(&e)))
            .transpose()
    }

    async fn put(&self, key: &str, value: &[u8], secret: bool) -> Result<(), ExtensionStoreError> {
        check_key(key)?;
        if value.len() > MAX_VALUE_BYTES {
            return Err(ExtensionStoreError::ValueTooLarge);
        }
        let mut tx = self.pool.begin().await.map_err(|e| db_error(&e))?;
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ext_kv WHERE namespace = ? AND key = ?")
                .bind(&self.ns)
                .bind(key)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| db_error(&e))?;
        if exists == 0 {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ext_kv WHERE namespace = ?")
                .bind(&self.ns)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| db_error(&e))?;
            if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_KEYS_PER_NAMESPACE {
                return Err(ExtensionStoreError::TooManyKeys);
            }
        }
        sqlx::query(
            "INSERT INTO ext_kv (namespace, key, value, is_secret, updated_at) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (namespace, key) DO UPDATE SET \
             value = excluded.value, is_secret = excluded.is_secret, \
             updated_at = excluded.updated_at",
        )
        .bind(&self.ns)
        .bind(key)
        .bind(value)
        .bind(i64::from(secret))
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await
        .map_err(|e| db_error(&e))?;
        tx.commit().await.map_err(|e| db_error(&e))
    }
}
