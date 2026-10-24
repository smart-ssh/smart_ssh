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

/// The reserved `secrets` entry that holds the host keys of an import whose
/// database transaction has committed but whose host-key file is not yet
/// written (Spec 0109, I5). Device state: never part of a snapshot.
pub const PENDING_HOST_KEYS_REF: &str = "app:pending_host_keys";

/// Whether a `secrets` reference belongs to the backup content (C1): the
/// credentials of servers and AI providers.
pub fn is_content_secret_ref(owner_ref: &str) -> bool {
    owner_ref.starts_with("server:") || owner_ref.starts_with("ai-provider:")
}

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

fn is_false(b: &bool) -> bool {
    !*b
}

/// Row of `groups`. `is_copy` is an optional field (Spec 0109, F7): absent
/// means `false`, and it is only written when set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupRow {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_copy: bool,
}

/// Row of `servers`. `auth_method` is the stored JSON text, as is (C3:
/// a key file is exported as the path the record holds). `is_copy` as for
/// [`GroupRow`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerRow {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<Uuid>,
    pub auth_method: String,
    pub notes: String,
    pub jump_host_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    pub post_ingest_policy: String,
    pub ai_injection_check_enabled: bool,
    pub sftp_server_path: Option<String>,
    pub start_directory: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_copy: bool,
}

rows! {
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
    /// Told about every committed whole-set change (import, change set).
    data_changes: ssh_manager_core::data_change::DataChangeNotifier,
}

fn uuid(s: &str) -> Result<Uuid, BackupStoreError> {
    Uuid::parse_str(s).map_err(|_| BackupStoreError::Data("id"))
}

fn opt_uuid(s: Option<String>) -> Result<Option<Uuid>, BackupStoreError> {
    s.as_deref().map(uuid).transpose()
}

impl SqliteBackupStore {
    pub(crate) fn new(
        pool: SqlitePool,
        data_changes: ssh_manager_core::data_change::DataChangeNotifier,
    ) -> Self {
        Self {
            pool,
            key_lock: Arc::new(tokio::sync::Mutex::new(())),
            data_changes,
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
        let snap = read_in(&mut tx, &mut between).await?;
        Ok(snap)
    }
}

/// The snapshot read, on an open transaction's connection.
async fn read_in<F, Fut>(
    conn: &mut sqlx::SqliteConnection,
    between: &mut F,
) -> Result<DatabaseSnapshot, BackupStoreError>
where
    F: FnMut(&'static str) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    {
        let mut snap = DatabaseSnapshot::default();
        let nil = Uuid::nil();

        let rows = sqlx::query("SELECT * FROM groups ORDER BY id")
            .fetch_all(&mut *conn)
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
                is_copy: r.get("is_copy"),
            });
        }
        between("groups").await;

        let rows = sqlx::query("SELECT * FROM servers ORDER BY id")
            .fetch_all(&mut *conn)
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
                is_copy: r.get("is_copy"),
            });
        }
        between("servers").await;

        let rows = sqlx::query("SELECT * FROM server_tags ORDER BY server_id, tag")
            .fetch_all(&mut *conn)
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
            .fetch_all(&mut *conn)
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
            .fetch_all(&mut *conn)
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
            .fetch_all(&mut *conn)
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
        let rows = sqlx::query("SELECT ref, value FROM secrets ORDER BY ref")
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
        for r in &rows {
            let owner_ref: String = r.get("ref");
            // Positive list: only credentials of servers and AI providers are
            // content. `app:` and `mcp:` entries are device state (C2, I9).
            if owner_ref.starts_with(&local_prefix) || !is_content_secret_ref(&owner_ref) {
                continue;
            }
            snap.secrets.push(SecretRow {
                owner_ref,
                value: r.get("value"),
            });
        }
        between("secrets").await;

        Ok(snap)
    }
}

/// What an import writes (Spec 0109, I4, I5). The content is already
/// validated by the caller.
pub struct ImportPlan {
    /// The content to import, with the IDs from the file.
    pub data: DatabaseSnapshot,
    /// Base name of the "Previous data" folder (the store appends ` (2)`,
    /// ` (3)` … when a top-level folder of that name exists); `None` when the local data is empty
    /// (I2) and nothing has to be kept.
    pub archive_folder: Option<String>,
    /// Timestamp (RFC 3339) for the rows created for the archive.
    pub now: String,
    /// JSON of the host keys to write after the commit (I5).
    pub pending_host_keys: String,
}

/// Counts of what the archive step moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArchiveCounts {
    pub servers: usize,
    pub groups: usize,
}

/// What the archive step reports back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArchiveOutcome {
    pub counts: ArchiveCounts,
    /// The folder name actually used.
    pub folder_name: Option<String>,
}

fn new_id() -> Uuid {
    Uuid::new_v4()
}

impl SqliteBackupStore {
    /// The host keys of an import that committed but did not reach their
    /// file yet (I5), if any.
    pub async fn pending_host_keys(&self) -> Result<Option<String>, BackupStoreError> {
        sqlx::query_scalar("SELECT value FROM secrets WHERE ref = ?")
            .bind(PENDING_HOST_KEYS_REF)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| kind(&e))
    }

    /// Removes the pending marker once the host-key file is written.
    pub async fn clear_pending_host_keys(&self) -> Result<(), BackupStoreError> {
        sqlx::query("DELETE FROM secrets WHERE ref = ?")
            .bind(PENDING_HOST_KEYS_REF)
            .execute(&self.pool)
            .await
            .map_err(|e| kind(&e))?;
        Ok(())
    }

    /// Replaces the local data with `plan.data` in **one** transaction (I5).
    pub async fn apply_import(
        &self,
        plan: &ImportPlan,
    ) -> Result<ArchiveOutcome, BackupStoreError> {
        self.apply_import_with(plan, &mut |_| false).await
    }

    /// As [`Self::apply_import`]; `abort_after` is asked after each step
    /// ("archive", "replace") and aborts the transaction when it returns
    /// `true` (tests inject a failure there).
    pub async fn apply_import_with(
        &self,
        plan: &ImportPlan,
        abort_after: &mut (dyn FnMut(&'static str) -> bool + Send),
    ) -> Result<ArchiveOutcome, BackupStoreError> {
        let mut tx = self.pool.begin().await.map_err(|e| kind(&e))?;
        let conn = &mut *tx;
        // References are checked at commit: rows go in file order.
        sqlx::query("PRAGMA defer_foreign_keys = ON")
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
        let current = read_in(&mut *conn, &mut |_| async {}).await?;
        let mut outcome = ArchiveOutcome::default();
        let mut moved = DatabaseSnapshot::default();

        if let Some(folder) = &plan.archive_folder {
            let name = unique_folder_name(&current, folder);
            outcome.counts = archive(&mut *conn, &current, &name, &plan.now, &mut moved).await?;
            outcome.folder_name = Some(name);
        }
        // Old servers and groups go (their copies exist); then everything
        // that is replaced wholesale.
        for s in &current.servers {
            exec_id(&mut *conn, "DELETE FROM servers WHERE id = ?", s.id).await?;
        }
        for g in &current.groups {
            exec_id(&mut *conn, "DELETE FROM groups WHERE id = ?", g.id).await?;
        }
        if abort_after("archive") {
            return Err(BackupStoreError::Database("aborted"));
        }
        let nil = Uuid::nil();
        sqlx::query("DELETE FROM note_revisions WHERE target_id <> ?")
            .bind(nil.to_string())
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
        sqlx::query(
            "DELETE FROM filter_rules WHERE NOT (scope_type = 'server' AND scope_value = ?)",
        )
        .bind(nil.to_string())
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
        sqlx::query("DELETE FROM ai_provider_configs")
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
        for s in &current.secrets {
            sqlx::query("DELETE FROM secrets WHERE ref = ?")
                .bind(&s.owner_ref)
                .execute(&mut *conn)
                .await
                .map_err(|e| kind(&e))?;
        }

        // The moved copies (new IDs), then the file's content (its IDs).
        insert_owned_rows(&mut *conn, &moved).await?;
        insert_all(&mut *conn, &plan.data).await?;

        sqlx::query(
            "INSERT INTO secrets (ref, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(ref) DO UPDATE SET value = excluded.value, \
             updated_at = excluded.updated_at",
        )
        .bind(PENDING_HOST_KEYS_REF)
        .bind(&plan.pending_host_keys)
        .bind(&plan.now)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
        if abort_after("replace") {
            return Err(BackupStoreError::Database("aborted"));
        }
        tx.commit().await.map_err(|e| kind(&e))?;
        self.notify_all_changed();
        Ok(outcome)
    }

    fn notify_all_changed(&self) {
        use ssh_manager_core::data_change::DataKind;
        self.data_changes.notify_many(&[
            DataKind::Servers,
            DataKind::Groups,
            DataKind::Tags,
            DataKind::Rules,
            DataKind::Notes,
            DataKind::AiProviders,
            DataKind::Secrets,
        ]);
    }

    /// Applies a change set in **one** transaction (issue #670). Inside the
    /// transaction the current content is read and handed to `build`, which
    /// returns the complete target content (and, if host keys change, the
    /// JSON for the pending marker, I5) or refuses it. Rows are then
    /// inserted, updated and deleted to make the tables equal the target; the
    /// pseudo-server's rows and the device state are never touched. Any
    /// failure, including `build`'s, rolls everything back.
    ///
    /// `abort_after` is asked after "upsert" and "delete" and aborts the
    /// transaction when it returns `true` (tests inject a failure there).
    pub async fn apply_changes_with<E>(
        &self,
        build: impl FnOnce(&DatabaseSnapshot) -> Result<(DatabaseSnapshot, Option<String>), E>,
        now: &str,
        abort_after: &mut (dyn FnMut(&'static str) -> bool + Send),
    ) -> Result<(), E>
    where
        E: From<BackupStoreError>,
    {
        let mut tx = self.pool.begin().await.map_err(|e| kind(&e))?;
        let conn = &mut *tx;
        sqlx::query("PRAGMA defer_foreign_keys = ON")
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
        let current = read_in(&mut *conn, &mut |_| async {}).await?;
        let (target, pending_host_keys) = build(&current)?;
        write_difference(&mut *conn, &current, &target, now).await?;
        if abort_after("upsert") {
            return Err(BackupStoreError::Database("aborted").into());
        }
        delete_difference(&mut *conn, &current, &target).await?;
        if let Some(json) = pending_host_keys {
            sqlx::query(
                "INSERT INTO secrets (ref, value, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT(ref) DO UPDATE SET value = excluded.value, \
                 updated_at = excluded.updated_at",
            )
            .bind(PENDING_HOST_KEYS_REF)
            .bind(json)
            .bind(now)
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
        }
        if abort_after("delete") {
            return Err(BackupStoreError::Database("aborted").into());
        }
        tx.commit().await.map_err(|e| kind(&e))?;
        self.notify_all_changed();
        Ok(())
    }
}

/// Inserts new rows and updates changed ones, parents before children is not
/// needed: references are checked at commit.
async fn write_difference(
    conn: &mut sqlx::SqliteConnection,
    current: &DatabaseSnapshot,
    target: &DatabaseSnapshot,
    now: &str,
) -> Result<(), BackupStoreError> {
    use std::collections::HashMap;
    fn changed<'a, T: PartialEq, K: std::hash::Hash + Eq>(
        old: &[T],
        new: &'a [T],
        key: impl Fn(&T) -> K,
    ) -> Vec<&'a T> {
        let old: HashMap<K, &T> = old.iter().map(|r| (key(r), r)).collect();
        new.iter()
            .filter(|r| old.get(&key(r)).is_none_or(|o| *o != *r))
            .collect()
    }
    for g in changed(&current.groups, &target.groups, |g| g.id) {
        sqlx::query(
            "INSERT INTO groups (id, name, parent_id, notes, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, \
             parent_id = excluded.parent_id, notes = excluded.notes, \
             created_at = excluded.created_at, updated_at = excluded.updated_at",
        )
        .bind(g.id.to_string())
        .bind(&g.name)
        .bind(g.parent_id.map(|p| p.to_string()))
        .bind(&g.notes)
        .bind(&g.created_at)
        .bind(&g.updated_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for s in changed(&current.servers, &target.servers, |s| s.id) {
        sqlx::query(
            "INSERT INTO servers (id, name, host, port, username, group_id, auth_method, notes, \
             jump_host_id, created_at, updated_at, post_ingest_policy, \
             ai_injection_check_enabled, sftp_server_path, start_directory) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, host = excluded.host, \
             port = excluded.port, username = excluded.username, group_id = excluded.group_id, \
             auth_method = excluded.auth_method, notes = excluded.notes, \
             jump_host_id = excluded.jump_host_id, created_at = excluded.created_at, \
             updated_at = excluded.updated_at, post_ingest_policy = excluded.post_ingest_policy, \
             ai_injection_check_enabled = excluded.ai_injection_check_enabled, \
             sftp_server_path = excluded.sftp_server_path, \
             start_directory = excluded.start_directory",
        )
        .bind(s.id.to_string())
        .bind(&s.name)
        .bind(&s.host)
        .bind(i64::from(s.port))
        .bind(&s.username)
        .bind(s.group_id.map(|p| p.to_string()))
        .bind(&s.auth_method)
        .bind(&s.notes)
        .bind(s.jump_host_id.map(|p| p.to_string()))
        .bind(&s.created_at)
        .bind(&s.updated_at)
        .bind(&s.post_ingest_policy)
        .bind(s.ai_injection_check_enabled)
        .bind(&s.sftp_server_path)
        .bind(&s.start_directory)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for t in changed(&current.tags, &target.tags, |t| {
        (t.server_id, t.tag.clone())
    }) {
        sqlx::query("INSERT OR IGNORE INTO server_tags (server_id, tag) VALUES (?, ?)")
            .bind(t.server_id.to_string())
            .bind(&t.tag)
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
    }
    for r in changed(&current.rules, &target.rules, |r| r.id) {
        sqlx::query(
            "INSERT INTO filter_rules (id, pattern_type, pattern_value, action, scope_type, \
             scope_value, priority, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET pattern_type = excluded.pattern_type, \
             pattern_value = excluded.pattern_value, action = excluded.action, \
             scope_type = excluded.scope_type, scope_value = excluded.scope_value, \
             priority = excluded.priority, created_at = excluded.created_at, \
             updated_at = excluded.updated_at",
        )
        .bind(r.id.to_string())
        .bind(&r.pattern_type)
        .bind(&r.pattern_value)
        .bind(&r.action)
        .bind(&r.scope_type)
        .bind(&r.scope_value)
        .bind(r.priority)
        .bind(&r.created_at)
        .bind(&r.updated_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for n in changed(&current.notes, &target.notes, |n| n.id) {
        sqlx::query(
            "INSERT INTO note_revisions (id, target_type, target_id, content, editor_type, \
             ai_provider, ai_model, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET target_type = excluded.target_type, \
             target_id = excluded.target_id, content = excluded.content, \
             editor_type = excluded.editor_type, ai_provider = excluded.ai_provider, \
             ai_model = excluded.ai_model, created_at = excluded.created_at",
        )
        .bind(n.id.to_string())
        .bind(&n.target_type)
        .bind(n.target_id.to_string())
        .bind(&n.content)
        .bind(&n.editor_type)
        .bind(&n.ai_provider)
        .bind(&n.ai_model)
        .bind(&n.created_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for a in changed(&current.ai_providers, &target.ai_providers, |a| a.id) {
        sqlx::query(
            "INSERT INTO ai_provider_configs (id, provider_type, display_name, base_url, model, \
             supports_native_tool_calling, credential_ref, is_active, created_at, updated_at, \
             extra_headers, attestation_url, max_tokens_override, web_research_enabled) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET provider_type = excluded.provider_type, \
             display_name = excluded.display_name, base_url = excluded.base_url, \
             model = excluded.model, \
             supports_native_tool_calling = excluded.supports_native_tool_calling, \
             credential_ref = excluded.credential_ref, is_active = excluded.is_active, \
             created_at = excluded.created_at, updated_at = excluded.updated_at, \
             extra_headers = excluded.extra_headers, attestation_url = excluded.attestation_url, \
             max_tokens_override = excluded.max_tokens_override, \
             web_research_enabled = excluded.web_research_enabled",
        )
        .bind(a.id.to_string())
        .bind(&a.provider_type)
        .bind(&a.display_name)
        .bind(&a.base_url)
        .bind(&a.model)
        .bind(a.supports_native_tool_calling)
        .bind(&a.credential_ref)
        .bind(a.is_active)
        .bind(&a.created_at)
        .bind(&a.updated_at)
        .bind(&a.extra_headers)
        .bind(&a.attestation_url)
        .bind(a.max_tokens_override)
        .bind(a.web_research_enabled)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for s in changed(&current.secrets, &target.secrets, |s| s.owner_ref.clone()) {
        sqlx::query(
            "INSERT INTO secrets (ref, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(ref) DO UPDATE SET value = excluded.value, \
             updated_at = excluded.updated_at",
        )
        .bind(&s.owner_ref)
        .bind(&s.value)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    Ok(())
}

/// Removes the rows that are in `current` but not in `target`. Children go
/// before the rows they depend on; deleting a server cascades to its tags,
/// chat sessions and history through the schema, exactly like the UI delete.
async fn delete_difference(
    conn: &mut sqlx::SqliteConnection,
    current: &DatabaseSnapshot,
    target: &DatabaseSnapshot,
) -> Result<(), BackupStoreError> {
    use std::collections::HashSet;
    async fn del(
        conn: &mut sqlx::SqliteConnection,
        sql: &'static str,
        key: String,
    ) -> Result<(), BackupStoreError> {
        sqlx::query(sql)
            .bind(key)
            .execute(conn)
            .await
            .map_err(|e| kind(&e))?;
        Ok(())
    }
    let ids = |v: &[Uuid]| v.iter().copied().collect::<HashSet<_>>();
    let keep = ids(&target.notes.iter().map(|n| n.id).collect::<Vec<_>>());
    for n in current.notes.iter().filter(|n| !keep.contains(&n.id)) {
        del(
            conn,
            "DELETE FROM note_revisions WHERE id = ?",
            n.id.to_string(),
        )
        .await?;
    }
    let keep = ids(&target.rules.iter().map(|r| r.id).collect::<Vec<_>>());
    for r in current.rules.iter().filter(|r| !keep.contains(&r.id)) {
        del(
            conn,
            "DELETE FROM filter_rules WHERE id = ?",
            r.id.to_string(),
        )
        .await?;
    }
    let keep: HashSet<&str> = target
        .secrets
        .iter()
        .map(|s| s.owner_ref.as_str())
        .collect();
    for s in current
        .secrets
        .iter()
        .filter(|s| !keep.contains(s.owner_ref.as_str()))
    {
        del(
            conn,
            "DELETE FROM secrets WHERE ref = ?",
            s.owner_ref.clone(),
        )
        .await?;
    }
    let keep: HashSet<(Uuid, &str)> = target
        .tags
        .iter()
        .map(|t| (t.server_id, t.tag.as_str()))
        .collect();
    for t in current
        .tags
        .iter()
        .filter(|t| !keep.contains(&(t.server_id, t.tag.as_str())))
    {
        sqlx::query("DELETE FROM server_tags WHERE server_id = ? AND tag = ?")
            .bind(t.server_id.to_string())
            .bind(&t.tag)
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
    }
    let keep = ids(&target.servers.iter().map(|s| s.id).collect::<Vec<_>>());
    for s in current.servers.iter().filter(|s| !keep.contains(&s.id)) {
        del(conn, "DELETE FROM servers WHERE id = ?", s.id.to_string()).await?;
    }
    let keep = ids(&target.groups.iter().map(|g| g.id).collect::<Vec<_>>());
    for g in current.groups.iter().filter(|g| !keep.contains(&g.id)) {
        del(conn, "DELETE FROM groups WHERE id = ?", g.id.to_string()).await?;
    }
    let keep = ids(&target.ai_providers.iter().map(|a| a.id).collect::<Vec<_>>());
    for a in current
        .ai_providers
        .iter()
        .filter(|a| !keep.contains(&a.id))
    {
        del(
            conn,
            "DELETE FROM ai_provider_configs WHERE id = ?",
            a.id.to_string(),
        )
        .await?;
    }
    Ok(())
}

/// Ids of the entries that carry the copy marker (Spec 0103).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyMarked {
    pub servers: Vec<Uuid>,
    pub groups: Vec<Uuid>,
}

/// What "Keep as regular entry" on a group affects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyMarkerScope {
    pub servers: usize,
    pub groups: usize,
}

/// Static SQL over the group and all its descendants (`?1` is the group id).
macro_rules! tree_sql {
    ($rest:literal) => {
        concat!(
            "WITH RECURSIVE tree(id) AS ( SELECT id FROM groups WHERE id = ?1 \
             UNION SELECT g.id FROM groups g JOIN tree t ON g.parent_id = t.id ) ",
            $rest
        )
    };
}

impl SqliteBackupStore {
    /// Every server and group that carries the copy marker.
    pub async fn copy_marked(&self) -> Result<CopyMarked, BackupStoreError> {
        let mut out = CopyMarked::default();
        for (sql, target) in [
            (
                "SELECT id FROM servers WHERE is_copy <> 0",
                &mut out.servers,
            ),
            ("SELECT id FROM groups WHERE is_copy <> 0", &mut out.groups),
        ] {
            let ids: Vec<String> = sqlx::query_scalar(sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| kind(&e))?;
            for id in ids {
                target.push(uuid(&id)?);
            }
        }
        Ok(out)
    }

    /// Clears the marker of one server. Returns whether a row changed.
    pub async fn clear_server_copy_marker(&self, id: Uuid) -> Result<bool, BackupStoreError> {
        let res = sqlx::query("UPDATE servers SET is_copy = 0 WHERE id = ? AND is_copy <> 0")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| kind(&e))?;
        Ok(res.rows_affected() > 0)
    }

    /// How many marked servers and groups a group action would clear.
    pub async fn group_copy_marker_scope(
        &self,
        id: Uuid,
    ) -> Result<CopyMarkerScope, BackupStoreError> {
        let groups: i64 = sqlx::query_scalar(tree_sql!(
            "SELECT COUNT(*) FROM groups WHERE id IN (SELECT id FROM tree) AND is_copy <> 0"
        ))
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| kind(&e))?;
        let servers: i64 = sqlx::query_scalar(tree_sql!(
            "SELECT COUNT(*) FROM servers WHERE group_id IN (SELECT id FROM tree) AND is_copy <> 0"
        ))
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| kind(&e))?;
        Ok(CopyMarkerScope {
            servers: usize::try_from(servers).unwrap_or(0),
            groups: usize::try_from(groups).unwrap_or(0),
        })
    }

    /// Clears the marker of a group, its sub-groups and all servers inside,
    /// in one transaction.
    pub async fn clear_group_copy_marker(
        &self,
        id: Uuid,
    ) -> Result<CopyMarkerScope, BackupStoreError> {
        let mut tx = self.pool.begin().await.map_err(|e| kind(&e))?;
        let mut out = CopyMarkerScope::default();
        let servers = sqlx::query(tree_sql!(
            "UPDATE servers SET is_copy = 0 \
             WHERE group_id IN (SELECT id FROM tree) AND is_copy <> 0"
        ))
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| kind(&e))?;
        let groups = sqlx::query(tree_sql!(
            "UPDATE groups SET is_copy = 0 \
             WHERE id IN (SELECT id FROM tree) AND is_copy <> 0"
        ))
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| kind(&e))?;
        out.servers = usize::try_from(servers.rows_affected()).unwrap_or(0);
        out.groups = usize::try_from(groups.rows_affected()).unwrap_or(0);
        tx.commit().await.map_err(|e| kind(&e))?;
        Ok(out)
    }
}

fn unique_folder_name(current: &DatabaseSnapshot, base: &str) -> String {
    let taken = |name: &str| {
        current
            .groups
            .iter()
            .any(|g| g.parent_id.is_none() && g.name == name)
    };
    if !taken(base) {
        return base.to_string();
    }
    (2u32..)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| base.to_string())
}

async fn exec_id(
    conn: &mut sqlx::SqliteConnection,
    sql: &'static str,
    id: Uuid,
) -> Result<(), BackupStoreError> {
    sqlx::query(sql)
        .bind(id.to_string())
        .execute(conn)
        .await
        .map_err(|e| kind(&e))?;
    Ok(())
}

/// Step 2 of I4: puts copies of the current servers and groups, with new IDs,
/// under a new folder and re-points what belongs to them. Rows that are
/// deleted and re-inserted afterwards (notes, rules, secrets) are collected
/// in `moved` instead of being written here.
async fn archive(
    conn: &mut sqlx::SqliteConnection,
    current: &DatabaseSnapshot,
    folder: &str,
    now: &str,
    moved: &mut DatabaseSnapshot,
) -> Result<ArchiveCounts, BackupStoreError> {
    use std::collections::HashMap;
    let folder_id = new_id();
    let group_map: HashMap<Uuid, Uuid> = current.groups.iter().map(|g| (g.id, new_id())).collect();
    let server_map: HashMap<Uuid, Uuid> =
        current.servers.iter().map(|s| (s.id, new_id())).collect();

    let mut groups = vec![GroupRow {
        id: folder_id,
        name: folder.to_string(),
        parent_id: None,
        notes: String::new(),
        created_at: now.to_string(),
        updated_at: now.to_string(),
        // The folder is created by the import; only what it moved is a copy.
        is_copy: false,
    }];
    for g in &current.groups {
        let mut g = g.clone();
        g.id = group_map[&g.id];
        g.is_copy = true;
        g.parent_id = Some(
            g.parent_id
                .and_then(|p| group_map.get(&p).copied())
                .unwrap_or(folder_id),
        );
        groups.push(g);
    }
    let mut servers = Vec::new();
    for s in &current.servers {
        let mut copy = s.clone();
        let new = server_map[&s.id];
        copy.id = new;
        copy.is_copy = true;
        copy.group_id = Some(
            s.group_id
                .and_then(|g| group_map.get(&g).copied())
                .unwrap_or(folder_id),
        );
        // The nil (local) server cannot be a jump host target in a snapshot;
        // an unknown target keeps its value.
        copy.jump_host_id = s
            .jump_host_id
            .map(|j| server_map.get(&j).copied().unwrap_or(j));
        copy.auth_method = s.auth_method.replace(&s.id.to_string(), &new.to_string());
        servers.push(copy);
    }
    let tags: Vec<TagRow> = current
        .tags
        .iter()
        .filter_map(|t| {
            server_map.get(&t.server_id).map(|n| TagRow {
                server_id: *n,
                tag: t.tag.clone(),
            })
        })
        .collect();
    // Groups first (servers refer to them), then servers and tags.
    let head = DatabaseSnapshot {
        groups,
        servers,
        tags,
        ..Default::default()
    };
    insert_all(&mut *conn, &head).await?;
    // History follows its server to the copy.
    for (old, new) in &server_map {
        for sql in [
            "UPDATE chat_sessions SET server_id = ? WHERE server_id = ?",
            "UPDATE prompt_history SET server_id = ? WHERE server_id = ?",
        ] {
            sqlx::query(sql)
                .bind(new.to_string())
                .bind(old.to_string())
                .execute(&mut *conn)
                .await
                .map_err(|e| kind(&e))?;
        }
    }

    for n in &current.notes {
        let target = match n.target_type.as_str() {
            "server" => server_map.get(&n.target_id),
            "group" => group_map.get(&n.target_id),
            _ => None,
        };
        if let Some(t) = target {
            let mut n = n.clone();
            n.id = new_id();
            n.target_id = *t;
            moved.notes.push(n);
        }
    }
    for r in &current.rules {
        if r.scope_type == "server" {
            let moved_to = r
                .scope_value
                .as_deref()
                .and_then(|v| Uuid::parse_str(v).ok())
                .and_then(|v| server_map.get(&v));
            if let Some(n) = moved_to {
                let mut r = r.clone();
                r.id = new_id();
                r.scope_value = Some(n.to_string());
                moved.rules.push(r);
            }
        }
    }
    for s in &current.secrets {
        for (old, new) in &server_map {
            let prefix = format!("server:{old}:");
            if let Some(rest) = s.owner_ref.strip_prefix(&prefix) {
                moved.secrets.push(SecretRow {
                    owner_ref: format!("server:{new}:{rest}"),
                    value: s.value.clone(),
                });
            }
        }
    }
    Ok(ArchiveCounts {
        servers: current.servers.len(),
        groups: current.groups.len(),
    })
}

/// Notes, rules and secrets (the rows that need no ordering).
async fn insert_owned_rows(
    conn: &mut sqlx::SqliteConnection,
    d: &DatabaseSnapshot,
) -> Result<(), BackupStoreError> {
    for n in &d.notes {
        sqlx::query(
            "INSERT INTO note_revisions (id, target_type, target_id, content, editor_type, \
             ai_provider, ai_model, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(n.id.to_string())
        .bind(&n.target_type)
        .bind(n.target_id.to_string())
        .bind(&n.content)
        .bind(&n.editor_type)
        .bind(&n.ai_provider)
        .bind(&n.ai_model)
        .bind(&n.created_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for r in &d.rules {
        sqlx::query(
            "INSERT INTO filter_rules (id, pattern_type, pattern_value, action, scope_type, \
             scope_value, priority, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(r.id.to_string())
        .bind(&r.pattern_type)
        .bind(&r.pattern_value)
        .bind(&r.action)
        .bind(&r.scope_type)
        .bind(&r.scope_value)
        .bind(r.priority)
        .bind(&r.created_at)
        .bind(&r.updated_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for s in &d.secrets {
        sqlx::query(
            "INSERT INTO secrets (ref, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(ref) DO UPDATE SET value = excluded.value, \
             updated_at = excluded.updated_at",
        )
        .bind(&s.owner_ref)
        .bind(&s.value)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    Ok(())
}

async fn insert_all(
    conn: &mut sqlx::SqliteConnection,
    d: &DatabaseSnapshot,
) -> Result<(), BackupStoreError> {
    for g in &d.groups {
        sqlx::query(
            "INSERT INTO groups (id, name, parent_id, notes, created_at, updated_at, is_copy) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(g.id.to_string())
        .bind(&g.name)
        .bind(g.parent_id.map(|p| p.to_string()))
        .bind(&g.notes)
        .bind(&g.created_at)
        .bind(&g.updated_at)
        .bind(g.is_copy)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for s in &d.servers {
        sqlx::query(
            "INSERT INTO servers (id, name, host, port, username, group_id, auth_method, notes, \
             jump_host_id, created_at, updated_at, post_ingest_policy, \
             ai_injection_check_enabled, sftp_server_path, start_directory, is_copy) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(s.id.to_string())
        .bind(&s.name)
        .bind(&s.host)
        .bind(i64::from(s.port))
        .bind(&s.username)
        .bind(s.group_id.map(|p| p.to_string()))
        .bind(&s.auth_method)
        .bind(&s.notes)
        .bind(s.jump_host_id.map(|p| p.to_string()))
        .bind(&s.created_at)
        .bind(&s.updated_at)
        .bind(&s.post_ingest_policy)
        .bind(s.ai_injection_check_enabled)
        .bind(&s.sftp_server_path)
        .bind(&s.start_directory)
        .bind(s.is_copy)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    for t in &d.tags {
        sqlx::query("INSERT INTO server_tags (server_id, tag) VALUES (?, ?)")
            .bind(t.server_id.to_string())
            .bind(&t.tag)
            .execute(&mut *conn)
            .await
            .map_err(|e| kind(&e))?;
    }
    insert_owned_rows(&mut *conn, d).await?;
    for a in &d.ai_providers {
        sqlx::query(
            "INSERT INTO ai_provider_configs (id, provider_type, display_name, base_url, model, \
             supports_native_tool_calling, credential_ref, is_active, created_at, updated_at, \
             extra_headers, attestation_url, max_tokens_override, web_research_enabled) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(a.id.to_string())
        .bind(&a.provider_type)
        .bind(&a.display_name)
        .bind(&a.base_url)
        .bind(&a.model)
        .bind(a.supports_native_tool_calling)
        .bind(&a.credential_ref)
        .bind(a.is_active)
        .bind(&a.created_at)
        .bind(&a.updated_at)
        .bind(&a.extra_headers)
        .bind(&a.attestation_url)
        .bind(a.max_tokens_override)
        .bind(a.web_research_enabled)
        .execute(&mut *conn)
        .await
        .map_err(|e| kind(&e))?;
    }
    Ok(())
}
