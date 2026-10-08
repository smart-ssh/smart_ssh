//! Issue #93 / Spec 0054, part 4: where "open locally" keeps its edit
//! copies, and the startup sweep that removes copies a crashed or killed
//! process left behind.
//!
//! Layout: `<cache>/smart-ssh/edit-sessions/<scope>/<session_id>/<file>`.
//! `<scope>` is derived from the (canonical) data directory of this
//! instance, so a debug build and an installed release — separate data
//! directories (ADR 0032), same cache directory — never share a scope. The
//! data-directory lock (issue #19) guarantees that at most one process uses
//! a data directory, so once this process holds it, every entry in its own
//! scope is a leftover: session ids are never reused, and no session of this
//! process exists yet. See ADR 0114.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Name of the shared root below `<cache>/smart-ssh/`. Before issue #93 the
/// session directories lay directly in it.
const ROOT_DIR_NAME: &str = "edit-sessions";

/// Prefix of every scope directory. Session directories of earlier versions
/// are named by a bare UUID and so never start with it — that is how the
/// legacy cleanup tells them apart from scopes of other instances.
const SCOPE_PREFIX: &str = "data-";

/// The edit-copy root of this process. Set by [`sweep_at_startup`] from the
/// locked data directory; [`instance_root`] derives the same value lazily
/// when it was not set (tests, which never run the startup).
static INSTANCE_ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

/// FNV-1a, 64 bit. Written out instead of `DefaultHasher`, whose algorithm
/// is explicitly not stable across Rust releases — a changed hash after an
/// update would orphan the old scope, and no later start would sweep it.
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

/// Directory name of the scope belonging to `data_dir`.
fn scope_name(data_dir: &Path) -> String {
    format!(
        "{SCOPE_PREFIX}{:016x}",
        fnv1a64(data_dir.as_os_str().as_encoded_bytes())
    )
}

fn shared_root(cache_dir: &Path) -> PathBuf {
    cache_dir.join("smart-ssh").join(ROOT_DIR_NAME)
}

/// Edit-copy root of the instance that owns `data_dir`.
pub(crate) fn instance_root_for(cache_dir: &Path, data_dir: &Path) -> PathBuf {
    shared_root(cache_dir).join(scope_name(data_dir))
}

/// The edit-copy root of this process; `None` if the OS reports no cache
/// directory (the caller turns that into its error, as before issue #93).
pub(crate) fn instance_root() -> Option<PathBuf> {
    INSTANCE_ROOT
        .get_or_init(|| {
            let cache = directories::BaseDirs::new()?.cache_dir().to_path_buf();
            let db_path = persistence_sqlite::default_db_path();
            let data_dir = db_path.parent()?;
            // Same form as `DataDirLock::dir()` (canonical), so the lazy and
            // the startup value agree; the raw path only if the directory
            // does not exist (yet).
            let data_dir = data_dir
                .canonicalize()
                .unwrap_or_else(|_| data_dir.to_path_buf());
            Some(instance_root_for(&cache, &data_dir))
        })
        .clone()
}

/// What a sweep did. Only counts — the paths of failures go to the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SweepReport {
    pub removed: usize,
    pub failed: usize,
}

/// Removes one directory entry without following a symlink: a link (also a
/// link to a directory) is removed as a link, a real directory recursively
/// — `std::fs::remove_dir_all` itself does not follow links inside.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    let file_type = std::fs::symlink_metadata(path)?.file_type();
    if file_type.is_dir() {
        // `is_dir()` is false for a symlink: `symlink_metadata` describes
        // the link itself.
        return std::fs::remove_dir_all(path);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        // Windows: a directory symlink or junction is removed with
        // `remove_dir`, which removes the link, not the target.
        Err(err) if cfg!(windows) && file_type.is_symlink() => {
            std::fs::remove_dir(path).map_err(|_| err)
        }
        Err(err) => Err(err),
    }
}

/// Removes every entry directly in `dir` that `keep` does not keep. Best
/// effort: each failure is logged (path and error, never file content) and
/// the sweep continues. A link or file in place of `dir` itself is removed
/// as such, never followed.
fn sweep_dir(dir: &Path, keep: impl Fn(&std::ffi::OsStr) -> bool) -> SweepReport {
    let mut report = SweepReport::default();
    match std::fs::symlink_metadata(dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return report,
        Err(err) => {
            tracing::warn!(path = %dir.display(), error = %err, "edit-copy sweep: could not inspect directory");
            report.failed += 1;
            return report;
        }
        Ok(meta) if !meta.is_dir() => {
            // Not a real directory (e.g. a symlink planted in its place):
            // remove the entry itself, never look behind it.
            match remove_entry(dir) {
                Ok(()) => report.removed += 1,
                Err(err) => {
                    tracing::warn!(path = %dir.display(), error = %err, "edit-copy sweep: could not remove entry");
                    report.failed += 1;
                }
            }
            return report;
        }
        Ok(_) => {}
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(path = %dir.display(), error = %err, "edit-copy sweep: could not list directory");
            report.failed += 1;
            return report;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(path = %dir.display(), error = %err, "edit-copy sweep: could not read directory entry");
                report.failed += 1;
                continue;
            }
        };
        if keep(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        match remove_entry(&path) {
            Ok(()) => report.removed += 1,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "edit-copy sweep: could not remove leftover edit copy");
                report.failed += 1;
            }
        }
    }
    report
}

/// Removes every leftover in the scope of `data_dir`. Must only run while
/// this process holds the lock of `data_dir` and before any session exists.
pub(crate) fn sweep_instance(cache_dir: &Path, data_dir: &Path) -> SweepReport {
    sweep_dir(&instance_root_for(cache_dir, data_dir), |_| false)
}

/// Removes session directories of earlier versions, which lay directly in
/// the shared root. Scope directories of other instances are kept.
fn sweep_legacy(cache_dir: &Path) -> SweepReport {
    sweep_dir(&shared_root(cache_dir), |name| {
        name.to_str().is_some_and(|n| n.starts_with(SCOPE_PREFIX))
    })
}

/// Whether the legacy session directories may be removed by this process.
///
/// Only a release build with the standard data directory. An older release
/// cannot run next to it (same data directory, same lock), so on a normal
/// installation nothing can still use the legacy directories. What the lock
/// cannot see is an older build with a *different* data directory — a debug
/// build or a redirected one, i.e. a development setup. The release build
/// accepts that residual case (such a build would lose its local copy, not
/// the server file); debug and redirected builds never remove legacy
/// directories, so two development builds do not sweep each other
/// (ADR 0114).
fn legacy_cleanup_allowed() -> bool {
    !cfg!(debug_assertions) && !persistence_sqlite::data_dir_is_overridden()
}

/// Startup step (issue #93): fixes this process's edit-copy root from the
/// locked data directory and removes everything a previous process of this
/// instance left behind. Never fails — a problem is logged and the start
/// goes on.
pub(crate) fn sweep_at_startup(locked_data_dir: &Path) {
    let Some(base) = directories::BaseDirs::new() else {
        tracing::warn!("edit-copy sweep skipped: no cache directory");
        return;
    };
    let cache = base.cache_dir();
    let root = instance_root_for(cache, locked_data_dir);
    if INSTANCE_ROOT.set(Some(root.clone())).is_err() && instance_root() != Some(root) {
        // Cannot happen in the app (this runs once, before any session);
        // sweeping a root the sessions do not use would be pointless.
        tracing::warn!("edit-copy sweep skipped: edit-copy root was already fixed differently");
        return;
    }
    let report = sweep_instance(cache, locked_data_dir);
    tracing::info!(
        removed = report.removed,
        failed = report.failed,
        "swept leftover edit copies of this instance (issue #93)"
    );
    if legacy_cleanup_allowed() {
        let legacy = sweep_legacy(cache);
        tracing::info!(
            removed = legacy.removed,
            failed = legacy.failed,
            "swept edit copies of earlier versions (issue #93)"
        );
    }
}

/// Writes an edit copy into `dir` with owner-only permissions on Unix
/// (`0700` for the directory, `0600` for the file): the content is server
/// content, possibly secrets in a config file. The file is created with
/// `0600` right away (Spec 0067, A5 — in elevated mode it can be a root
/// file); `set_permissions` covers a file that already existed (`mode` only
/// applies on creation).
pub(crate) fn write_edit_copy(dir: &Path, file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = options.open(file)?;
        std::io::Write::write_all(&mut handle, bytes)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _tmp: tempfile::TempDir,
        cache: PathBuf,
        data_a: PathBuf,
        data_b: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let data_a = tmp.path().join("Smart SSH");
        let data_b = tmp.path().join("Smart SSH (dev)");
        std::fs::create_dir_all(&cache).unwrap();
        Fixture {
            _tmp: tmp,
            cache,
            data_a,
            data_b,
        }
    }

    fn plant_leftover(root: &Path) -> PathBuf {
        let dir = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("nginx.conf"), b"password=secret").unwrap();
        dir
    }

    /// Acceptance criterion 1: a crash leftover `<session-id>/file` in this
    /// instance's root is gone after the startup sweep.
    #[test]
    fn test_sweep_removes_a_crash_leftover_of_this_instance() {
        let f = fixture();
        let root = instance_root_for(&f.cache, &f.data_a);
        let leftover = plant_leftover(&root);

        let report = sweep_instance(&f.cache, &f.data_a);

        assert!(!leftover.exists(), "leftover edit copy must be removed");
        assert_eq!(
            report,
            SweepReport {
                removed: 1,
                failed: 0
            }
        );
    }

    /// Acceptance criterion 2: the scope of another data directory (another
    /// running instance, e.g. a debug build next to a release) is not
    /// touched.
    #[test]
    fn test_sweep_leaves_the_scope_of_another_data_directory_alone() {
        let f = fixture();
        let own = plant_leftover(&instance_root_for(&f.cache, &f.data_a));
        let foreign = plant_leftover(&instance_root_for(&f.cache, &f.data_b));
        assert_ne!(
            instance_root_for(&f.cache, &f.data_a),
            instance_root_for(&f.cache, &f.data_b)
        );

        sweep_instance(&f.cache, &f.data_a);

        assert!(!own.exists());
        assert_eq!(
            std::fs::read(foreign.join("nginx.conf")).unwrap(),
            b"password=secret",
            "another instance's edit copy must stay byte for byte"
        );
    }

    /// The scope is a pure function of the data directory: a later start
    /// with the same data directory finds the same root again.
    #[test]
    fn test_scope_is_stable_for_the_same_data_directory() {
        let f = fixture();
        assert_eq!(
            instance_root_for(&f.cache, &f.data_a),
            instance_root_for(&f.cache, &f.data_a)
        );
        // Pinned value: a changed hash would orphan existing scopes.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    /// Acceptance criterion 3: a symlink in the root that points outside is
    /// removed as a link; its target (file or directory) survives.
    #[cfg(unix)]
    #[test]
    fn test_sweep_removes_symlinks_without_following_them() {
        let f = fixture();
        let root = instance_root_for(&f.cache, &f.data_a);
        std::fs::create_dir_all(&root).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("id_ed25519");
        std::fs::write(&outside_file, b"KEY").unwrap();
        let outside_dir = outside.path().join("project");
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("keep.txt"), b"keep").unwrap();

        std::os::unix::fs::symlink(&outside_file, root.join("file-link")).unwrap();
        std::os::unix::fs::symlink(&outside_dir, root.join("dir-link")).unwrap();
        // Also a link inside a session directory.
        let session = plant_leftover(&root);
        std::os::unix::fs::symlink(&outside_dir, session.join("nested-link")).unwrap();

        let report = sweep_instance(&f.cache, &f.data_a);

        assert_eq!(
            report,
            SweepReport {
                removed: 3,
                failed: 0
            }
        );
        assert!(std::fs::symlink_metadata(root.join("file-link")).is_err());
        assert!(std::fs::symlink_metadata(root.join("dir-link")).is_err());
        assert!(!session.exists());
        assert_eq!(std::fs::read(&outside_file).unwrap(), b"KEY");
        assert_eq!(
            std::fs::read(outside_dir.join("keep.txt")).unwrap(),
            b"keep"
        );
    }

    /// A symlink planted in place of the instance root itself is removed,
    /// and the directory it points to is not swept.
    #[cfg(unix)]
    #[test]
    fn test_sweep_does_not_follow_a_symlinked_root() {
        let f = fixture();
        let root = instance_root_for(&f.cache, &f.data_a);
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), &root).unwrap();

        sweep_instance(&f.cache, &f.data_a);

        assert!(std::fs::symlink_metadata(&root).is_err());
        assert_eq!(
            std::fs::read(outside.path().join("keep.txt")).unwrap(),
            b"keep"
        );
    }

    /// Acceptance criterion 4: an entry that cannot be deleted is counted
    /// as a failure (and logged with `warn!`), and the sweep goes on with
    /// the next entry. `sweep_at_startup` returns nothing to fail on.
    #[cfg(unix)]
    #[test]
    fn test_sweep_continues_after_an_undeletable_entry() {
        use std::os::unix::fs::PermissionsExt;

        let f = fixture();
        let root = instance_root_for(&f.cache, &f.data_a);
        let stuck = plant_leftover(&root);
        let locked = stuck.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("inner.conf"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        // Running as root ignores the permission bits — nothing to test then.
        if std::fs::write(locked.join("probe"), b"").is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        let other = plant_leftover(&root);
        let other_too = plant_leftover(&root);

        let report = sweep_instance(&f.cache, &f.data_a);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            report,
            SweepReport {
                removed: 2,
                failed: 1
            }
        );
        assert!(!other.exists() && !other_too.exists());
        assert!(locked.join("inner.conf").exists());
    }

    /// Acceptance criterion 5: after the sweep, a new edit copy is created
    /// as before — `0700` directory, `0600` file on Unix.
    #[test]
    fn test_open_after_sweep_creates_the_copy_with_owner_only_permissions() {
        let f = fixture();
        let root = instance_root_for(&f.cache, &f.data_a);
        plant_leftover(&root);
        sweep_instance(&f.cache, &f.data_a);

        let dir = root.join(uuid::Uuid::new_v4().to_string());
        let file = dir.join("app.conf");
        write_edit_copy(&dir, &file, b"content").unwrap();

        assert_eq!(std::fs::read(&file).unwrap(), b"content");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&file), 0o600);
        }
    }

    /// Legacy cleanup: session directories of earlier versions (directly in
    /// the shared root) go, every instance's scope stays.
    #[test]
    fn test_legacy_sweep_removes_old_session_dirs_but_keeps_all_scopes() {
        let f = fixture();
        let legacy = plant_leftover(&shared_root(&f.cache));
        let own = plant_leftover(&instance_root_for(&f.cache, &f.data_a));
        let foreign = plant_leftover(&instance_root_for(&f.cache, &f.data_b));

        let report = sweep_legacy(&f.cache);

        assert_eq!(
            report,
            SweepReport {
                removed: 1,
                failed: 0
            }
        );
        assert!(!legacy.exists());
        assert!(own.exists() && foreign.exists());
    }

    /// Never in a debug build: an older release may be running next to it.
    #[test]
    fn test_legacy_cleanup_is_off_in_debug_builds() {
        if cfg!(debug_assertions) {
            assert!(!legacy_cleanup_allowed());
        }
    }
}
