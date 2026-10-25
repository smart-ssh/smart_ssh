//! Encrypted full backup, export side (Spec 0109, issue #664): the device-local
//! backup key, the typed snapshot and the export itself. The file format and
//! its crypto live in `ssh_manager_core::backup`.

mod changes;
mod export;
mod import;
mod key;
mod snapshot;
#[cfg(test)]
mod tests;
mod validate;

pub use changes::{
    apply_changes, apply_changes_with, read_snapshot, ApplySummary, Change, ChangeSet,
};
pub use export::{export_backup, export_backup_with, ExportSummary};
pub use import::{
    complete_pending_host_keys, open_backup, start_import, start_import_snapshot, ImportContext,
    ImportPreview, ImportStart, ImportSummary, NoSessions, OpenSessions, PendingImport,
};
pub use key::{BackupKey, BackupKeyStatus};
pub use snapshot::{HostKeyRow, Snapshot, SnapshotCounts, KNOWN_SECTIONS};

use ssh_manager_core::backup::BackupError as FormatError;

/// Failure of a backup operation. Every text is fixed: none carries a code,
/// passphrase, key, path or server name (K8, S-7).
#[derive(Debug, PartialEq, Eq)]
pub enum BackupOpError {
    /// K2: there is no backup key yet; the first-export flow must run.
    KeyRequired,
    /// A create call on a device that already has a backup key.
    KeyAlreadyExists,
    /// K5 and the other format errors.
    Format(FormatError),
    /// The stored backup key cannot be read.
    KeyUnreadable,
    Store(persistence_sqlite::BackupStoreError),
    HostKeys,
    /// The file could not be written.
    Write,
    /// The backup file could not be read.
    Read,
    /// U5, U6: the file opened but its content is not acceptable. The text
    /// never names an entry.
    Invalid(&'static str),
    /// X4: the written file did not verify; it was removed.
    VerificationFailed,
}

impl std::fmt::Display for BackupOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyRequired => f.write_str("there is no backup key yet; set it up first"),
            Self::KeyAlreadyExists => f.write_str("this device already has a backup key"),
            Self::Format(e) => write!(f, "{e}"),
            Self::KeyUnreadable => f.write_str("the stored backup key cannot be read"),
            Self::Store(e) => write!(f, "{e}"),
            Self::HostKeys => f.write_str("the trusted host keys could not be read"),
            Self::Write => f.write_str("the backup file could not be written"),
            Self::Read => f.write_str("the backup file could not be read"),
            Self::Invalid(what) => write!(f, "the backup content is not valid: {what}"),
            Self::VerificationFailed => {
                f.write_str("the written backup did not verify and was removed")
            }
        }
    }
}

impl std::error::Error for BackupOpError {}

impl From<FormatError> for BackupOpError {
    fn from(e: FormatError) -> Self {
        Self::Format(e)
    }
}

impl From<persistence_sqlite::BackupStoreError> for BackupOpError {
    fn from(e: persistence_sqlite::BackupStoreError) -> Self {
        Self::Store(e)
    }
}
