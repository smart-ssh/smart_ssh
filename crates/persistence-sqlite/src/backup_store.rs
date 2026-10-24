//! Backup support of the database (Spec 0109, issue #664): the consistent
//! snapshot of every table the backup contains (C1) and the storage of the
//! device-local backup key (K4).
//!
//! The backup key lives as one reserved entry in `secrets` (ADR 0134,
//! decision 3), so it needs no schema change and sits inside the encrypted
//! database in both modes. [`BACKUP_KEY_REF`] is neither a `server:` nor an
//! `ai-provider:` reference: the secret move skips it, and the snapshot reads
//! `secrets` without it (C2).

use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use uuid::Uuid;
use zeroize::Zeroize;

/// The reserved `secrets` entry that holds the backup key (K4).
pub const BACKUP_KEY_REF: &str = "app:backup_key";

/// Only the kind of a failure, never a library text (S-7, Spec 0094).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackupStoreError {
    #[error("the database could not be read or written ({0})")]
    Database(&'static str),
    #[error("the data contains a value the backup format cannot hold ({0})")]
    Data(&'static str),
}

fn kind(err: &sqlx::Error) -> BackupStoreError {
    tracing::warn!(error_kind = error_kind(err), "backup store query failed");
    BackupStoreError::Database(error_kind(err))
}

fn error_kind(err: &sqlx::Error) -> &'static str {
    match err {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolTimedOut => "pool_timeout",
        sqlx::Error::PoolClosed => "pool_closed",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode",
        _ => "other",
    }
}

macro_rules! rows {
    ($($(#[$m:meta])* $name:ident { $($field:ident : $ty:ty),* $(,)? })*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    )*};
}

rows! {
    /// Row of `groups`.
    GroupRow {
        id: Uuid, name: String, parent_id: Option<Uuid>, notes: String,
        created_at: String, updated_at: String,
    }
    /// Row of `servers`. `auth_method` is the stored JSON text, as is (C3:
    /// a key file is exported as the path the record holds).
    ServerRow {
        id: Uuid, name: String, host: String, port: u16, username: String,
        group_id: Option<Uuid>, auth_method: String, notes: String,
        jump_host_id: Option<Uuid>, created_at: String, updated_at: String,
        post_ingest_policy: String, ai_injection_check_enabled: bool,
        sftp_server_path: Option<String>, start_directory: Option<String>,
    }
    /// Row of `server_tags`.
    TagRow { server_id: Uuid, tag: String }
    /// Row of `filter_rules`. `scope_value` is a server id or a tag name.
    RuleRow {
        id: Uuid, pattern_type: String, pattern_value: String, action: String,
        scope_type: String, scope_value: Option<String>, priority: i64,
        created_at: String, updated_at: String,
    }
    /// Row of `note_revisions`.
    NoteRevisionRow {
        id: Uuid, target_type: String, target_id: Uuid, content: String,
        editor_type: String, ai_provider: Option<String>,
        ai_model: Option<String>, created_at: String,
    }
    /// Row of `ai_provider_configs`.
    AiProviderRow {
        id: Uuid, provider_type: String, display_name: String,
        base_url: Option<String>, model: String,
        supports_native_tool_calling: bool, credential_ref: String,
        is_active: bool, created_at: String, updated_at: String,
        extra_headers: String, attestation_url: Option<String>,
        max_tokens_override: Option<i64>, web_research_enabled: bool,
    }
}

/// Row of `secrets` (`value` is cleared when dropped).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRow {
    pub owner_ref: String,
    pub value: String,
}

impl Drop for SecretRow {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl std::fmt::Debug for SecretRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretRow(<redacted>)")
    }
}

/// All C1 tables, read in one transaction (X2).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DatabaseSnapshot {
    pub groups: Vec<GroupRow>,
    pub servers: Vec<ServerRow>,
    pub tags: Vec<TagRow>,
    pub rules: Vec<RuleRow>,
    pub notes: Vec<NoteRevisionRow>,
    pub ai_providers: Vec<AiProviderRow>,
    pub secrets: Vec<SecretRow>,
}

/// Pool-backed access for the backup feature. Cheap to clone.
#[derive(Clone)]
pub struct SqliteBackupStore {
    pool: SqlitePool,
    /// Serialises changes of the backup key (create / replace).
    key_lock: Arc<tokio::sync::Mutex<()>>,
}

fn uuid(s: &str) -> Result<Uuid, BackupStoreError> {
    Uuid::parse_str(s).map_err(|_| BackupStoreError::Data("id"))
}

fn opt_uuid(s: Option<String>) -> Result<Option<Uuid>, BackupStoreError> {
    s.as_deref().map(uuid).transpose()
}

impl SqliteBackupStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            key_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Guards a read-modify-write of the backup key.
    pub async fn lock_key(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.key_lock.lock().await
    }

    /// The stored backup key blob (K4), if the device has one.
    pub async fn load_backup_key(&self) -> Result<Option<SecretString>, BackupStoreError> {
        let row: Option<String> = sqlx::query_scalar("SELECT value FROM secrets WHERE ref = ?")
            .bind(BACKUP_KEY_REF)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| kind(&e))?;
        Ok(row.map(SecretString::from))
    }

    /// Writes (or replaces) the backup key blob.
    pub async fn store_backup_key(&self, blob: &SecretString) -> Result<(), BackupStoreError> {
        sqlx::query(
            "INSERT INTO secrets (ref, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(ref) DO UPDATE SET value = excluded.value, \
             updated_at = excluded.updated_at",
        )
        .bind(BACKUP_KEY_REF)
        .bind(blob.expose_secret())
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| kind(&e))?;
        Ok(())
    }

    /// Reads every C1 table in **one** read transaction (X2).
    pub async fn read_snapshot(&self) -> Result<DatabaseSnapshot, BackupStoreError> {
        self.read_snapshot_with(|_| async {}).await
    }

    /// As [`Self::read_snapshot`]; `between` runs after each table was read
    /// (tests write there to prove the cut is consistent).
    pub async fn read_snapshot_with<F, Fut>(
        &self,
        mut between: F,
    ) -> Result<DatabaseSnapshot, BackupStoreError>
    where
        F: FnMut(&'static str) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let mut tx = self.pool.begin().await.map_err(|e| kind(&e))?;
        let mut snap = DatabaseSnapshot::default();
        let nil = Uuid::nil();

        let rows = sqlx::query("SELECT * FROM groups ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            snap.groups.push(GroupRow {
                id: uuid(&r.get::<String, _>("id"))?,
                name: r.get("name"),
                parent_id: opt_uuid(r.get("parent_id"))?,
                notes: r.get("notes"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            });
        }
        between("groups").await;

        let rows = sqlx::query("SELECT * FROM servers ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            let id = uuid(&r.get::<String, _>("id"))?;
            if id == nil {
                continue; // the local pseudo-server is never exported (C2)
            }
            snap.servers.push(ServerRow {
                id,
                name: r.get("name"),
                host: r.get("host"),
                port: u16::try_from(r.get::<i64, _>("port"))
                    .map_err(|_| BackupStoreError::Data("port"))?,
                username: r.get("username"),
                group_id: opt_uuid(r.get("group_id"))?,
                auth_method: r.get("auth_method"),
                notes: r.get("notes"),
                jump_host_id: opt_uuid(r.get("jump_host_id"))?,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                post_ingest_policy: r.get("post_ingest_policy"),
                ai_injection_check_enabled: r.get("ai_injection_check_enabled"),
                sftp_server_path: r.get("sftp_server_path"),
                start_directory: r.get("start_directory"),
            });
        }
        between("servers").await;

        let rows = sqlx::query("SELECT * FROM server_tags ORDER BY server_id, tag")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            let server_id = uuid(&r.get::<String, _>("server_id"))?;
            if server_id != nil {
                snap.tags.push(TagRow {
                    server_id,
                    tag: r.get("tag"),
                });
            }
        }
        between("server_tags").await;

        let rows = sqlx::query("SELECT * FROM filter_rules ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            let scope_type: String = r.get("scope_type");
            let scope_value: Option<String> = r.get("scope_value");
            if scope_type == "server" && scope_value.as_deref().map(uuid).transpose()? == Some(nil)
            {
                continue; // rule of the local pseudo-server (C2)
            }
            snap.rules.push(RuleRow {
                id: uuid(&r.get::<String, _>("id"))?,
                pattern_type: r.get("pattern_type"),
                pattern_value: r.get("pattern_value"),
                action: r.get("action"),
                scope_type,
                scope_value,
                priority: r.get("priority"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            });
        }
        between("filter_rules").await;

        let rows = sqlx::query("SELECT * FROM note_revisions ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            let target_id = uuid(&r.get::<String, _>("target_id"))?;
            if target_id == nil {
                continue;
            }
            snap.notes.push(NoteRevisionRow {
                id: uuid(&r.get::<String, _>("id"))?,
                target_type: r.get("target_type"),
                target_id,
                content: r.get("content"),
                editor_type: r.get("editor_type"),
                ai_provider: r.get("ai_provider"),
                ai_model: r.get("ai_model"),
                created_at: r.get("created_at"),
            });
        }
        between("note_revisions").await;

        let rows = sqlx::query("SELECT * FROM ai_provider_configs ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            snap.ai_providers.push(AiProviderRow {
                id: uuid(&r.get::<String, _>("id"))?,
                provider_type: r.get("provider_type"),
                display_name: r.get("display_name"),
                base_url: r.get("base_url"),
                model: r.get("model"),
                supports_native_tool_calling: r.get("supports_native_tool_calling"),
                credential_ref: r.get("credential_ref"),
                is_active: r.get("is_active"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                extra_headers: r.get("extra_headers"),
                attestation_url: r.get("attestation_url"),
                max_tokens_override: r.get("max_tokens_override"),
                web_research_enabled: r.get("web_research_enabled"),
            });
        }
        between("ai_provider_configs").await;

        // C2: the backup key (and the database root key's reference) are not
        // content. Of the rest, the local pseudo-server's slots are skipped.
        let local_prefix = format!("server:{nil}:");
        let rows = sqlx::query("SELECT ref, value FROM secrets WHERE ref <> ? ORDER BY ref")
            .bind(BACKUP_KEY_REF)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            let owner_ref: String = r.get("ref");
            if owner_ref.starts_with(&local_prefix)
                || owner_ref == ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF
            {
                continue;
            }
            snap.secrets.push(SecretRow {
                owner_ref,
                value: r.get("value"),
            });
        }
        between("secrets").await;

        tx.rollback().await.map_err(|e| kind(&e))?;
        Ok(snap)
    }
}
