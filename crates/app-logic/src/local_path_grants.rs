//! Issue #89 / Spec 0020, section 5: which local paths the file browser may
//! read for an upload, the overwrite preview and the edit-session polling.
//!
//! The webview is not a trust boundary on its own — it also renders
//! AI-generated content. A local path passed in through `invoke` therefore
//! proves nothing. The backend keeps, per session, the set of local paths
//! the user actually granted by a gesture the webview cannot fake:
//!
//! - a path returned by the backend-driven native open dialog,
//! - a path from the window's native drag-and-drop event (captured in the
//!   backend, see [`LocalPathGrants::record_drop`] and
//!   [`LocalPathGrants::claim_drop`]),
//! - a file inside the session's own edit-session directory (passed to
//!   [`LocalPathGrants::check`] by the caller).
//!
//! Every comparison runs on canonicalised paths: the granted roots are
//! canonicalised when they are granted, the requested path when it is
//! checked. A `..` component or a symlink below a granted root that
//! resolves outside of it therefore never matches. A granted directory
//! covers every file below it; a granted file covers only itself.
//!
//! Grants live only in memory and are dropped with the session
//! ([`LocalPathGrants::remove_session`]); session ids are never reused.
//!
//! Decision record: ADR 0113.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::CommandError;
use crate::poison::lock_tolerating_poison;
use crate::state::SessionId;

/// How long a native drop stays claimable. The file browser claims it
/// right after the webview receives the same drop event; a drop no
/// visible file browser claimed (e.g. dropped onto the chat) must not stay
/// around for a later, unrelated claim.
pub const DROP_CLAIM_WINDOW: Duration = Duration::from_secs(10);

/// User-facing text when a path is outside every grant. Deliberately the
/// same text for "never granted", "escapes a granted root" and "session
/// gone" — the caller learns nothing about which grants exist.
pub const LOCAL_PATH_NOT_GRANTED: &str =
    "Diese lokale Datei wurde für diese Sitzung weder ausgewählt noch abgelegt \
     und darf deshalb nicht gelesen werden.";

/// A local path that passed [`LocalPathGrants::check`]: canonical, and
/// inside a grant of the session it was checked for. Only this module can
/// construct one (outside of tests), so a reader that takes a
/// `GrantedLocalPath` cannot be handed an unchecked path by mistake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantedLocalPath(PathBuf);

impl GrantedLocalPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// For tests that exercise a reader directly, without a grant set.
    #[cfg(any(test, feature = "test-support"))]
    pub fn unchecked_for_tests(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

#[derive(Debug)]
struct PendingDrop {
    paths: Vec<PathBuf>,
    at: Instant,
}

/// Per-session grant set plus the most recent, not yet claimed native drop.
#[derive(Debug, Default)]
pub struct LocalPathGrants {
    roots: Mutex<HashMap<SessionId, Vec<PathBuf>>>,
    pending_drop: Mutex<Option<PendingDrop>>,
}

impl LocalPathGrants {
    pub fn new() -> Self {
        Self::default()
    }

    /// Grants `paths` to `session_id` (open dialog, claimed drop). Each path
    /// is canonicalised now; a path that cannot be canonicalised (vanished
    /// in the meantime) is skipped — it could not be read anyway.
    ///
    /// Returns the paths as given, minus the skipped ones, so the caller
    /// hands the frontend exactly what the user picked (display names stay
    /// unchanged even if the pick was a symlink).
    pub fn grant(&self, session_id: SessionId, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut granted = Vec::with_capacity(paths.len());
        let mut canonical_roots = Vec::with_capacity(paths.len());
        for path in paths {
            match std::fs::canonicalize(&path) {
                Ok(canonical) => {
                    canonical_roots.push(canonical);
                    granted.push(path);
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "a picked or dropped local path could not be resolved and was not granted"
                    );
                }
            }
        }
        if !canonical_roots.is_empty() {
            lock_tolerating_poison(&self.roots)
                .entry(session_id)
                .or_default()
                .extend(canonical_roots);
        }
        granted
    }

    /// Checks `requested` against the grants of `session_id` and the
    /// session's edit-session directory. Reads nothing but file-system
    /// metadata (to canonicalise).
    ///
    /// `edit_session_dir`: the edit-session directory of **this** session,
    /// or `None` if it cannot be determined. It does not need to exist.
    pub fn check(
        &self,
        session_id: SessionId,
        requested: &Path,
        edit_session_dir: Option<&Path>,
    ) -> Result<GrantedLocalPath, CommandError> {
        let not_granted = || CommandError::from(LOCAL_PATH_NOT_GRANTED);
        // A path that does not resolve (missing file, broken link) is not
        // granted — there is nothing to compare.
        let canonical = std::fs::canonicalize(requested).map_err(|_| not_granted())?;

        let in_granted_root = lock_tolerating_poison(&self.roots)
            .get(&session_id)
            .is_some_and(|roots| roots.iter().any(|root| canonical.starts_with(root)));
        if in_granted_root {
            return Ok(GrantedLocalPath(canonical));
        }

        let in_edit_dir = edit_session_dir
            .and_then(|dir| std::fs::canonicalize(dir).ok())
            .is_some_and(|dir| canonical.starts_with(&dir) && canonical != dir);
        if in_edit_dir {
            return Ok(GrantedLocalPath(canonical));
        }

        tracing::warn!(
            session_id = %session_id,
            "a local path outside every grant of this session was refused"
        );
        Err(not_granted())
    }

    /// Drops every grant of `session_id` (disconnect).
    pub fn remove_session(&self, session_id: SessionId) {
        lock_tolerating_poison(&self.roots).remove(&session_id);
    }

    /// Stores the path list of a native drop event. Called only from the
    /// window's native drag-and-drop handler, never from a command. A newer
    /// drop replaces an unclaimed older one.
    pub fn record_drop(&self, paths: Vec<PathBuf>) {
        self.record_drop_at(paths, Instant::now());
    }

    fn record_drop_at(&self, paths: Vec<PathBuf>, at: Instant) {
        *lock_tolerating_poison(&self.pending_drop) = Some(PendingDrop { paths, at });
    }

    /// Takes the most recent native drop (at most once) and grants its
    /// paths to `session_id`. Returns the granted paths, or an empty list
    /// if there is no drop, or it is older than [`DROP_CLAIM_WINDOW`].
    pub fn claim_drop(&self, session_id: SessionId) -> Vec<PathBuf> {
        self.claim_drop_at(session_id, Instant::now())
    }

    fn claim_drop_at(&self, session_id: SessionId, now: Instant) -> Vec<PathBuf> {
        let pending = lock_tolerating_poison(&self.pending_drop).take();
        match pending {
            Some(drop) if now.saturating_duration_since(drop.at) <= DROP_CLAIM_WINDOW => {
                self.grant(session_id, drop.paths)
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_a_path_outside_every_grant_is_rejected() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let picked = dir.path().join("picked.txt");
        let secret = dir.path().join("id_ed25519");
        write(&picked, b"ok");
        write(&secret, b"secret");
        grants.grant(session, vec![picked.clone()]);

        assert!(grants.check(session, &picked, None).is_ok());
        let err = grants.check(session, &secret, None).unwrap_err();
        assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
    }

    #[test]
    fn test_a_session_without_any_grant_reads_nothing() {
        let grants = LocalPathGrants::new();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        write(&file, b"x");

        assert!(grants.check(SessionId::new_v4(), &file, None).is_err());
    }

    #[test]
    fn test_a_granted_file_does_not_cover_its_siblings() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        write(&a, b"a");
        write(&b, b"b");
        grants.grant(session, vec![a]);

        assert!(grants.check(session, &b, None).is_err());
    }

    #[test]
    fn test_a_granted_directory_covers_files_below_it() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let nested = root.join("sub/deep/file.txt");
        write(&nested, b"x");
        grants.grant(session, vec![root.clone()]);

        let granted = grants.check(session, &nested, None).unwrap();
        assert_eq!(granted.as_path(), std::fs::canonicalize(&nested).unwrap());
        assert!(grants.check(session, &root, None).is_ok());
    }

    #[test]
    fn test_a_sibling_directory_with_a_common_name_prefix_is_not_covered() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let sibling = dir.path().join("project-secrets/key");
        write(&root.join("a.txt"), b"a");
        write(&sibling, b"secret");
        grants.grant(session, vec![root]);

        assert!(grants.check(session, &sibling, None).is_err());
    }

    #[test]
    fn test_dot_dot_out_of_a_granted_root_is_rejected() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        write(&root.join("a.txt"), b"a");
        let outside = dir.path().join("secret.txt");
        write(&outside, b"secret");
        grants.grant(session, vec![root.clone()]);

        let escaping = root.join("..").join("secret.txt");
        assert!(escaping.exists(), "precondition: the path resolves");
        assert!(grants.check(session, &escaping, None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_a_symlink_below_a_granted_root_that_points_outside_is_rejected() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside");
        write(&outside.join("id_ed25519"), b"secret");
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        std::os::unix::fs::symlink(outside.join("id_ed25519"), root.join("key")).unwrap();
        grants.grant(session, vec![root.clone()]);

        assert!(grants
            .check(session, &root.join("link/id_ed25519"), None)
            .is_err());
        assert!(grants.check(session, &root.join("key"), None).is_err());
    }

    #[test]
    fn test_a_missing_path_is_rejected() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        grants.grant(session, vec![dir.path().to_path_buf()]);

        assert!(grants
            .check(session, &dir.path().join("does-not-exist"), None)
            .is_err());
    }

    #[test]
    fn test_grants_are_per_session() {
        let grants = LocalPathGrants::new();
        let mine = SessionId::new_v4();
        let other = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        write(&file, b"x");
        grants.grant(mine, vec![file.clone()]);

        assert!(grants.check(other, &file, None).is_err());
    }

    #[test]
    fn test_grants_are_gone_after_remove_session() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        write(&file, b"x");
        grants.grant(session, vec![file.clone()]);

        grants.remove_session(session);

        assert!(grants.check(session, &file, None).is_err());
    }

    #[test]
    fn test_a_file_in_the_own_edit_session_dir_is_allowed() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let edit_dir = tempfile::tempdir().unwrap();
        let file = edit_dir.path().join("a.conf");
        write(&file, b"x");

        assert!(grants.check(session, &file, Some(edit_dir.path())).is_ok());
        // The directory itself is not a file to upload.
        assert!(grants
            .check(session, edit_dir.path(), Some(edit_dir.path()))
            .is_err());
    }

    #[test]
    fn test_a_file_in_another_sessions_edit_dir_is_rejected() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let base = tempfile::tempdir().unwrap();
        let own_dir = base.path().join("own");
        std::fs::create_dir_all(&own_dir).unwrap();
        let foreign = base.path().join("other/a.conf");
        write(&foreign, b"x");

        assert!(grants.check(session, &foreign, Some(&own_dir)).is_err());
        let escaping = own_dir.join("..").join("other").join("a.conf");
        assert!(grants.check(session, &escaping, Some(&own_dir)).is_err());
    }

    #[test]
    fn test_a_claimed_drop_grants_its_paths_once() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dropped.txt");
        let folder = dir.path().join("dropped-folder");
        write(&file, b"x");
        write(&folder.join("inner.txt"), b"y");

        grants.record_drop(vec![file.clone(), folder.clone()]);
        let claimed = grants.claim_drop(session);

        assert_eq!(claimed, vec![file.clone(), folder.clone()]);
        assert!(grants.check(session, &file, None).is_ok());
        assert!(grants
            .check(session, &folder.join("inner.txt"), None)
            .is_ok());
        assert!(
            grants.claim_drop(SessionId::new_v4()).is_empty(),
            "a drop is claimable only once"
        );
    }

    /// ADR 0113, "Restrisiko: Abholen ist nicht an Sitzung oder Ziel
    /// gebunden": the pending drop is global, so any live session may claim
    /// it — not only the one whose file browser received it — and the first
    /// claim consumes it. Pins the documented behaviour; if a later change
    /// binds the claim to a session or drop target, update the ADR with it.
    #[test]
    fn test_a_drop_is_claimable_by_any_session_but_only_once() {
        let grants = LocalPathGrants::new();
        let intended = SessionId::new_v4();
        let other = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dropped.txt");
        write(&file, b"x");

        grants.record_drop(vec![file.clone()]);
        let claimed_by_other = grants.claim_drop(other);
        let claimed_by_intended = grants.claim_drop(intended);

        assert_eq!(
            claimed_by_other,
            vec![file.clone()],
            "the claim is not bound to the session the drop was meant for"
        );
        assert!(grants.check(other, &file, None).is_ok());
        assert!(
            claimed_by_intended.is_empty(),
            "the first claim consumes the drop"
        );
        assert!(grants.check(intended, &file, None).is_err());
    }

    #[test]
    fn test_claim_without_a_drop_grants_nothing() {
        let grants = LocalPathGrants::new();
        assert!(grants.claim_drop(SessionId::new_v4()).is_empty());
    }

    #[test]
    fn test_a_stale_drop_is_not_claimable() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dropped.txt");
        write(&file, b"x");
        let then = Instant::now();

        grants.record_drop_at(vec![file.clone()], then);
        let claimed =
            grants.claim_drop_at(session, then + DROP_CLAIM_WINDOW + Duration::from_secs(1));

        assert!(claimed.is_empty());
        assert!(grants.check(session, &file, None).is_err());
    }

    #[test]
    fn test_grant_skips_a_path_that_does_not_resolve() {
        let grants = LocalPathGrants::new();
        let session = SessionId::new_v4();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        write(&file, b"x");
        let missing = dir.path().join("gone.txt");

        let granted = grants.grant(session, vec![missing, file.clone()]);

        assert_eq!(granted, vec![file]);
    }
}
