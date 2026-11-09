//! Android tester switch (issue #782): force the credential store to count as
//! unavailable so the startup dialog offers master-password setup (Spec 0101 §8).
//!
//! Trigger: an empty marker file named [`MARKER_FILE`] in the app data
//! directory. On a debug build a tester creates it with
//! `adb shell run-as <package> touch files/force-keystore-unavailable`
//! (`run-as` only works for debuggable apps). Release builds never look at it.

use std::path::Path;

/// Name of the marker file inside the data directory.
pub(crate) const MARKER_FILE: &str = "force-keystore-unavailable";

/// Whether the switch is active: only when `debug` is set and the marker exists.
/// Kept free of `cfg` so the release behaviour is testable on the host.
pub(crate) fn switch_active(data_dir: &Path, debug: bool) -> bool {
    debug && data_dir.join(MARKER_FILE).exists()
}

/// Called once at startup, before the first credential-store access.
#[cfg(target_os = "android")]
pub(crate) fn apply(data_dir: &Path) {
    if switch_active(data_dir, cfg!(debug_assertions)) {
        tracing::warn!("debug switch: forcing the Android Keystore to count as unavailable");
        credentials_keyring::set_forced_unavailable(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_only_in_debug_with_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!switch_active(dir.path(), true), "no marker: unchanged");
        std::fs::write(dir.path().join(MARKER_FILE), b"").unwrap();
        assert!(switch_active(dir.path(), true));
        assert!(!switch_active(dir.path(), false), "release ignores marker");
    }
}
