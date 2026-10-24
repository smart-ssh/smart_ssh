//! Export of a consistent snapshot into an encrypted backup file (Spec 0109,
//! X2–X6).

use std::path::Path;

use persistence_sqlite::SqliteBackupStore;
use ssh_manager_core::atomic_write::write_atomically;
use ssh_manager_core::backup::{open_with_slot, seal, KdfParams, SealInput};

use super::{BackupKey, BackupOpError, Snapshot, SnapshotCounts, KNOWN_SECTIONS};
use crate::host_key_store::FileHostKeyStore;

/// What the export reports on success: counts, never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSummary {
    pub counts: SnapshotCounts,
}

/// Writes a full backup of the current data to `target`.
///
/// Without a backup key the call fails with [`BackupOpError::KeyRequired`]
/// and creates nothing (K2). The data is not changed (X6). The file is
/// written atomically (X3), read back and fully verified under every slot
/// before success is reported; on a failed check it is removed (X4).
pub async fn export_backup(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
    target: &Path,
    writer_version: &str,
) -> Result<ExportSummary, BackupOpError> {
    export_backup_with(store, host_keys, target, writer_version, &mut |_| {}).await
}

/// As [`export_backup`]; `after_write` runs on the finished file before the
/// read-back (tests damage the file there).
pub async fn export_backup_with(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
    target: &Path,
    writer_version: &str,
    after_write: &mut dyn FnMut(&Path),
) -> Result<ExportSummary, BackupOpError> {
    let result = run(store, host_keys, target, writer_version, after_write).await;
    match &result {
        Ok(summary) => tracing::info!(
            servers = summary.counts.servers,
            groups = summary.counts.groups,
            rules = summary.counts.rules,
            notes = summary.counts.notes,
            ai_providers = summary.counts.ai_providers,
            secrets = summary.counts.secrets,
            host_keys = summary.counts.host_keys,
            "backup export succeeded"
        ),
        Err(err) => tracing::warn!(result = ?err, "backup export failed"),
    }
    result
}

async fn run(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
    target: &Path,
    writer_version: &str,
    after_write: &mut dyn FnMut(&Path),
) -> Result<ExportSummary, BackupOpError> {
    let slots = BackupKey::active_slots(store).await?;
    let snapshot = Snapshot::read(store, host_keys).await?;
    let sections = snapshot.to_sections()?;
    let bytes = seal(&SealInput {
        sections: &sections,
        slots: &slots,
        created_at: chrono::Utc::now().timestamp(),
        writer_version,
        extensions: &[],
        kdf: KdfParams::WRITE,
    })?;

    write_atomically(target, &bytes).map_err(|err| {
        tracing::warn!(io_kind = ?err.kind(), "backup file could not be written");
        BackupOpError::Write
    })?;
    after_write(target);

    // X4: read back and authenticate every section under every slot.
    let verified = verify(target, &slots, &sections);
    if !verified {
        if let Err(err) = std::fs::remove_file(target) {
            tracing::error!(io_kind = ?err.kind(), "unverified backup file could not be removed");
        }
        return Err(BackupOpError::VerificationFailed);
    }
    Ok(ExportSummary {
        counts: snapshot.counts(),
    })
}

fn verify(
    target: &Path,
    slots: &[ssh_manager_core::backup::SlotMaterial],
    expected: &[ssh_manager_core::backup::Section],
) -> bool {
    let Ok(bytes) = std::fs::read(target) else {
        return false;
    };
    slots.iter().all(|slot| {
        open_with_slot(&bytes, slot, KNOWN_SECTIONS)
            .map(|opened| opened.sections == expected)
            .unwrap_or(false)
    })
}
