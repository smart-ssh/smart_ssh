//! Atomic file replacement (Spec 0109, X3, ADR 0134 decision 11).
//!
//! The shared helper for every write of the backup feature: write to a
//! temporary file in the **same directory**, flush it to disk, rename it over
//! the target, sync the directory. At any failing step the target is either
//! the complete old file or the complete new one, and the temporary file is
//! removed. The three older local copies are not migrated by this module.

use std::io::Write;
use std::path::{Path, PathBuf};

/// One step of [`write_atomically`]; the test hook can fail each of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Create,
    Write,
    Flush,
    Rename,
}

/// Replaces `path` with `bytes` atomically (X3). The temporary file is
/// created new with owner-only permissions (0600 on Unix). A leftover
/// temporary file of an earlier, interrupted run is removed first. Only one
/// writer per target at a time is supported.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomically_with(path, bytes, &mut |_| Ok(()))
}

pub(crate) fn write_atomically_with(
    path: &Path,
    bytes: &[u8],
    before: &mut dyn FnMut(Step) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let tmp = temporary_path(path)?;
    remove_leftover(&tmp)?;
    let result = write_and_rename(path, &tmp, bytes, before);
    if result.is_err() {
        // Never leave a (possibly complete) copy of the content behind. A
        // failing cleanup must not hide the original error.
        let _ = std::fs::remove_file(&tmp);
    }
    if result.is_ok() {
        sync_directory_of(path);
    }
    result
}

fn write_and_rename(
    path: &Path,
    tmp: &Path,
    bytes: &[u8],
    before: &mut dyn FnMut(Step) -> std::io::Result<()>,
) -> std::io::Result<()> {
    before(Step::Create)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(tmp)?;
    before(Step::Write)?;
    file.write_all(bytes)?;
    before(Step::Flush)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    before(Step::Rename)?;
    std::fs::rename(tmp, path)
}

fn temporary_path(path: &Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no file name")
    })?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".tmp");
    Ok(path.with_file_name(tmp_name))
}

fn remove_leftover(tmp: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(tmp) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Best effort: a directory cannot be opened on every platform, and a missing
/// sync only risks the old file reappearing after a crash, never a torn file.
fn sync_directory_of(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("atomic-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn entries(d: &Path) -> Vec<String> {
        let mut v: Vec<_> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn writes_new_and_replaces_existing() {
        let d = dir();
        let target = d.join("a.sshbackup");
        write_atomically(&target, b"one").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"one");
        write_atomically(&target, b"two").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"two");
        assert_eq!(entries(&d), vec!["a.sshbackup"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn failure_at_every_step_keeps_old_file_and_removes_temp() {
        for failing in [Step::Create, Step::Write, Step::Flush, Step::Rename] {
            let d = dir();
            let target = d.join("a.sshbackup");
            std::fs::write(&target, b"old").unwrap();
            let err = write_atomically_with(&target, b"new content", &mut |s| {
                if s == failing {
                    Err(std::io::Error::other("injected"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(err.to_string(), "injected", "{failing:?}");
            assert_eq!(std::fs::read(&target).unwrap(), b"old", "{failing:?}");
            assert_eq!(entries(&d), vec!["a.sshbackup"], "{failing:?}");
        }
    }

    #[test]
    fn failure_without_old_file_leaves_nothing() {
        let d = dir();
        let target = d.join("a.sshbackup");
        let _ = write_atomically_with(&target, b"x", &mut |s| {
            if s == Step::Rename {
                Err(std::io::Error::other("injected"))
            } else {
                Ok(())
            }
        });
        assert!(entries(&d).is_empty());
    }

    #[test]
    fn real_rename_failure_removes_temp() {
        let d = dir();
        let target = d.join("target");
        std::fs::create_dir_all(target.join("child")).unwrap();
        assert!(write_atomically(&target, b"x").is_err());
        assert_eq!(entries(&d), vec!["target"]);
    }

    #[test]
    fn leftover_of_an_interrupted_run_is_removed() {
        let d = dir();
        let target = d.join("a.sshbackup");
        std::fs::write(d.join(".a.sshbackup.tmp"), b"half").unwrap();
        write_atomically(&target, b"full").unwrap();
        assert_eq!(entries(&d), vec!["a.sshbackup"]);
        assert_eq!(std::fs::read(&target).unwrap(), b"full");
    }
}
