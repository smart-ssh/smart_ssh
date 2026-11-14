//! Best-effort tightening of file permissions for sensitive files
//! (database, key wrapping file, host keys, logs, settings).
//!
//! A failing `chmod` never fails the caller or aborts start-up, but it is
//! not silent either: it emits a `tracing::warn!` naming the file kind, the
//! target mode and the I/O error kind. The path is deliberately not logged
//! (Spec 0094: no content in default logs; paths may carry user names).

use std::path::Path;

/// Failure of one best-effort permission change; log it with
/// [`PermissionFailure::warn`]. Used where the subscriber is not yet
/// installed when the chmod runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionFailure {
    pub kind: &'static str,
    pub mode: u32,
    pub error_kind: std::io::ErrorKind,
}

impl PermissionFailure {
    pub fn warn(&self) {
        tracing::warn!(
            file_kind = self.kind,
            target_mode = format!("{:o}", self.mode),
            error_kind = ?self.error_kind,
            "could not tighten file permissions; the file keeps its current mode"
        );
    }
}

/// Sets `mode` on `path`; returns the failure instead of logging it.
#[cfg(unix)]
pub fn try_harden_permissions(
    path: &Path,
    mode: u32,
    kind: &'static str,
) -> Result<(), PermissionFailure> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|err| {
        PermissionFailure {
            kind,
            mode,
            error_kind: err.kind(),
        }
    })
}

#[cfg(not(unix))]
pub fn try_harden_permissions(
    _path: &Path,
    _mode: u32,
    _kind: &'static str,
) -> Result<(), PermissionFailure> {
    Ok(())
}

/// Sets `mode` on `path`; on failure logs a warning and returns normally.
pub fn harden_permissions(path: &Path, mode: u32, kind: &'static str) {
    if let Err(failure) = try_harden_permissions(path, mode, kind) {
        failure.warn();
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::log_capture;

    #[test]
    fn test_failing_chmod_warns_and_does_not_fail() {
        // Global thread-local capture, never a scoped dispatcher (#1336).
        log_capture::start_recording();
        let missing = Path::new("/nonexistent-dir-for-harden-test/secret.db");
        harden_permissions(missing, 0o600, "database");
        let out = log_capture::recorded_lines().join("\n");
        assert!(out.contains("WARN"), "{out}");
        assert!(out.contains("database"), "{out}");
        assert!(out.contains("600"), "{out}");
        assert!(out.contains("NotFound"), "{out}");
        assert!(
            !out.contains("nonexistent-dir"),
            "path must not be logged: {out}"
        );
    }

    #[test]
    fn test_successful_chmod_sets_mode_and_is_silent() {
        use std::os::unix::fs::PermissionsExt;
        let file = std::env::temp_dir().join(format!("harden-test-{}", std::process::id()));
        std::fs::write(&file, "x").unwrap();
        assert!(try_harden_permissions(&file, 0o600, "log_file").is_ok());
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_file(&file);
        assert_eq!(mode, 0o600);
    }
}
