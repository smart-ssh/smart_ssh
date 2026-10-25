//! Import of an encrypted backup that replaces the local data (Spec 0109,
//! I1–I9, U1–U7).
//!
//! Flow: [`start_import`] opens and validates the whole file in memory without
//! touching local data (I1). With empty local data it imports at once (I2);
//! otherwise it returns a [`PendingImport`] with both sets of counts (I3), and
//! only [`PendingImport::confirm`] changes anything (I4, I5).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use base64::Engine;
use persistence_sqlite::{DatabaseSnapshot, ImportPlan, SqliteBackupStore};
use ssh_manager_core::backup::{open, BackupSecret, MAX_FILE_BYTES};

use super::validate::validate;
use super::{export_backup, BackupOpError, HostKeyRow, Snapshot, SnapshotCounts, KNOWN_SECTIONS};
use crate::host_key_store::FileHostKeyStore;

/// The open terminal sessions (I8). Closing them is the caller's job because
/// it needs the app shell (events, terminals); the import only asks.
#[async_trait]
pub trait OpenSessions: Send + Sync {
    /// Sessions that the replacement closes: all but those to the local
    /// pseudo-server.
    fn affected(&self) -> usize;
    /// Closes them. Called after the safety backup was written and before
    /// the data is replaced.
    async fn close_affected(&self);
}

/// For callers without sessions (and tests).
pub struct NoSessions;

#[async_trait]
impl OpenSessions for NoSessions {
    fn affected(&self) -> usize {
        0
    }
    async fn close_affected(&self) {}
}

#[async_trait]
impl OpenSessions for crate::session::SessionManager {
    fn affected(&self) -> usize {
        self.snapshot()
            .iter()
            .filter(|s| !s.server_id.0.is_nil())
            .count()
    }
    /// Only removes the sessions from the registry; the app shell's close
    /// path (which also disconnects) is what a command should call first.
    async fn close_affected(&self) {
        for s in self.snapshot() {
            if !s.server_id.0.is_nil() {
                self.remove(s.session_id);
            }
        }
    }
}

/// Everything an import needs from the app.
pub struct ImportContext<'a> {
    pub store: &'a SqliteBackupStore,
    pub host_keys: &'a FileHostKeyStore,
    pub sessions: &'a dyn OpenSessions,
    /// Default directory of the safety backup (shown to the user first).
    pub safety_dir: &'a Path,
    /// Localized "Previous data".
    pub folder_label: &'a str,
    pub writer_version: &'a str,
}

/// I3: both sides' numbers, never taken from the file's header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPreview {
    pub local: SnapshotCounts,
    pub file: SnapshotCounts,
    pub open_sessions: usize,
    pub safety_backup_dir: PathBuf,
}

/// I8: what the user is told afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub counts: SnapshotCounts,
    pub safety_backup: Option<PathBuf>,
    pub previous_data_folder: Option<String>,
    /// The host-key file could not be written right after the commit; the
    /// next start completes it (I5).
    pub host_keys_pending: bool,
}

#[derive(Debug)]
pub enum ImportStart {
    /// Local data was empty (I2): done.
    Imported(ImportSummary),
    /// Local data exists: show the counts and ask (I3).
    NeedsConfirmation(Box<PendingImport>),
}

/// A validated file waiting for the user's confirmation. Dropping it changes
/// nothing.
pub struct PendingImport {
    snapshot: Snapshot,
    preview: ImportPreview,
}

impl std::fmt::Debug for PendingImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingImport(<redacted>)")
    }
}

/// I1 and U1–U7: reads, decrypts and validates everything in memory.
pub async fn open_backup(path: &Path, secret: BackupSecret) -> Result<Snapshot, BackupOpError> {
    let meta = std::fs::metadata(path).map_err(|_| BackupOpError::Read)?;
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(BackupOpError::Format(
            ssh_manager_core::backup::BackupError::LimitExceeded(
                ssh_manager_core::backup::Limit::FileSize,
            ),
        ));
    }
    let bytes = std::fs::read(path).map_err(|_| BackupOpError::Read)?;
    let snapshot = tokio::task::spawn_blocking(move || {
        let opened = open(&bytes, &secret, KNOWN_SECTIONS)?;
        Snapshot::from_sections(&opened.sections)
    })
    .await
    .map_err(|_| BackupOpError::Read)??;
    validate(&snapshot)?;
    Ok(snapshot)
}

fn is_empty(c: &SnapshotCounts) -> bool {
    c.servers == 0 && c.groups == 0 && c.rules == 0 && c.notes == 0 && c.ai_providers == 0
}

/// Opens the file; imports directly into empty local data, otherwise asks.
pub async fn start_import(
    ctx: &ImportContext<'_>,
    file: &Path,
    secret: BackupSecret,
) -> Result<ImportStart, BackupOpError> {
    let result = start(ctx, file, secret).await;
    if let Err(err) = &result {
        tracing::warn!(result = ?err, "backup import rejected");
    }
    result
}

/// Like [`start_import`] for a [`Snapshot`] the caller already holds in
/// memory: validates it, then imports directly into empty local data or
/// returns [`ImportStart::NeedsConfirmation`]. Nothing is written to disk
/// by this function itself; a rejected snapshot changes no local data.
pub async fn start_import_snapshot(
    ctx: &ImportContext<'_>,
    snapshot: Snapshot,
) -> Result<ImportStart, BackupOpError> {
    let result = start_snapshot(ctx, snapshot).await;
    if let Err(err) = &result {
        tracing::warn!(result = ?err, "backup import rejected");
    }
    result
}

async fn start(
    ctx: &ImportContext<'_>,
    file: &Path,
    secret: BackupSecret,
) -> Result<ImportStart, BackupOpError> {
    let snapshot = open_backup(file, secret).await?;
    start_snapshot(ctx, snapshot).await
}

async fn start_snapshot(
    ctx: &ImportContext<'_>,
    snapshot: Snapshot,
) -> Result<ImportStart, BackupOpError> {
    validate(&snapshot)?;
    let local = Snapshot::read(ctx.store, ctx.host_keys).await?.counts();
    let preview = ImportPreview {
        local,
        file: snapshot.counts(),
        open_sessions: ctx.sessions.affected(),
        safety_backup_dir: ctx.safety_dir.to_path_buf(),
    };
    let pending = PendingImport { snapshot, preview };
    if is_empty(&local) {
        return pending
            .run(ctx, false, &mut |_| false)
            .await
            .map(ImportStart::Imported);
    }
    Ok(ImportStart::NeedsConfirmation(Box::new(pending)))
}

impl PendingImport {
    pub fn preview(&self) -> &ImportPreview {
        &self.preview
    }

    /// I4, I5: safety backup, then folder and import in one transaction.
    pub async fn confirm(self, ctx: &ImportContext<'_>) -> Result<ImportSummary, BackupOpError> {
        self.confirm_with(ctx, &mut |_| false).await
    }

    /// As [`Self::confirm`]; `abort_after` can fail a step inside the
    /// transaction (tests).
    pub async fn confirm_with(
        self,
        ctx: &ImportContext<'_>,
        abort_after: &mut (dyn FnMut(&'static str) -> bool + Send),
    ) -> Result<ImportSummary, BackupOpError> {
        let result = self.run(ctx, true, abort_after).await;
        if let Err(err) = &result {
            tracing::warn!(result = ?err, "backup import failed; local data unchanged");
        }
        result
    }

    async fn run(
        self,
        ctx: &ImportContext<'_>,
        keep_previous: bool,
        abort_after: &mut (dyn FnMut(&'static str) -> bool + Send),
    ) -> Result<ImportSummary, BackupOpError> {
        // Step 1: a complete backup of the current state. Any failure,
        // including a missing backup key, ends the import here.
        let safety_backup = if keep_previous {
            let path = safety_backup_path(ctx.safety_dir)?;
            export_backup(ctx.store, ctx.host_keys, &path, ctx.writer_version).await?;
            Some(path)
        } else {
            None
        };
        ctx.sessions.close_affected().await;

        // Steps 2 and 3 in one transaction.
        let host_keys_json =
            serde_json::to_string(&self.snapshot.host_keys).map_err(|_| BackupOpError::HostKeys)?;
        let plan = ImportPlan {
            data: self.snapshot.to_database(),
            archive_folder: keep_previous.then(|| {
                format!(
                    "{} {}",
                    ctx.folder_label,
                    chrono::Local::now().format("%Y-%m-%d")
                )
            }),
            now: chrono::Utc::now().to_rfc3339(),
            pending_host_keys: host_keys_json,
        };
        let outcome = ctx.store.apply_import_with(&plan, abort_after).await?;

        let host_keys_pending = match complete_pending_host_keys(ctx.store, ctx.host_keys).await {
            Ok(_) => false,
            Err(err) => {
                tracing::warn!(result = ?err, "host keys will be completed at the next start");
                true
            }
        };
        let counts = self.snapshot.counts();
        tracing::info!(
            servers = counts.servers,
            groups = counts.groups,
            rules = counts.rules,
            notes = counts.notes,
            ai_providers = counts.ai_providers,
            moved_servers = outcome.counts.servers,
            moved_groups = outcome.counts.groups,
            "backup import succeeded"
        );
        Ok(ImportSummary {
            counts,
            safety_backup,
            previous_data_folder: outcome.folder_name,
            host_keys_pending,
        })
    }
}

/// File name with date and time; an existing file is never overwritten.
fn safety_backup_path(dir: &Path) -> Result<PathBuf, BackupOpError> {
    std::fs::create_dir_all(dir).map_err(|_| BackupOpError::Write)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    for n in 0u32.. {
        let name = if n == 0 {
            format!("smart-ssh-before-import-{stamp}.sshbackup")
        } else {
            format!("smart-ssh-before-import-{stamp}-{n}.sshbackup")
        };
        let path = dir.join(name);
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(BackupOpError::Write)
}

/// I5: writes the host keys of a committed import into their file and clears
/// the marker. Run at every start **before** any connection is allowed, and
/// right after an import. Returns whether there was something to complete.
pub async fn complete_pending_host_keys(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
) -> Result<bool, BackupOpError> {
    let Some(json) = store.pending_host_keys().await? else {
        return Ok(false);
    };
    let rows: Vec<HostKeyRow> = serde_json::from_str(&json).map_err(|_| BackupOpError::HostKeys)?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let entries = rows
        .into_iter()
        .map(|r| {
            b64.decode(&r.raw_key)
                .map(|raw| (r.host, r.port, raw))
                .map_err(|_| BackupOpError::HostKeys)
        })
        .collect::<Result<Vec<_>, _>>()?;
    host_keys
        .replace_all(entries)
        .map_err(|_| BackupOpError::HostKeys)?;
    store.clear_pending_host_keys().await?;
    tracing::info!("pending host keys completed");
    Ok(true)
}

impl Snapshot {
    /// The database part, for [`ImportPlan`].
    pub fn to_database(&self) -> DatabaseSnapshot {
        DatabaseSnapshot {
            groups: self.groups.clone(),
            servers: self.servers.clone(),
            tags: self.tags.clone(),
            rules: self.rules.clone(),
            notes: self.notes.clone(),
            ai_providers: self.ai_providers.clone(),
            secrets: self.secrets.clone(),
        }
    }
}
