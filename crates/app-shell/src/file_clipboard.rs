//! Issue #358 / Spec 0054: "Copy file" puts a remote file on the system
//! clipboard as a file reference.
//!
//! The file is first downloaded into an app-private staging folder
//! (`<cache>/smart-ssh/clipboard-files/<scope>/<uuid>/<file name>`, owner-only
//! on Unix) and the clipboard then points at that path; the OS file manager
//! copies from there when the user pastes. The app never opens or executes
//! the staged file. The folder is emptied at exit and at the next start (a
//! crash leaves files behind), and a new "Copy file" replaces the previous
//! staged file — but only after the new one is on the clipboard, so a
//! cancelled or failed copy neither leaves a file nor changes the clipboard.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clipboard_rs::{Clipboard, ClipboardContext};

use crate::edit_copies::{clipboard_root, sweep_dir};

/// Managed Tauri state. Keeps the clipboard context alive: on X11 the
/// process that set the clipboard serves its content, so dropping the
/// context would drop the file reference with it.
#[derive(Default)]
pub struct FileClipboard {
    context: Mutex<Option<ClipboardContext>>,
}

#[cfg(unix)]
fn make_private(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn make_private(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Writes `bytes` as `<root>/<uuid>/<file_name>` (owner-only) and returns
/// the file path. `file_name` must already be a single safe path segment.
pub(crate) fn stage_file(root: &Path, file_name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    make_private(root)?;
    let slot = root.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&slot)?;
    make_private(&slot)?;
    let file = slot.join(file_name);
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = options.open(&file)?;
        std::io::Write::write_all(&mut handle, bytes)
    })();
    if let Err(err) = result {
        let _ = std::fs::remove_dir_all(&slot);
        return Err(err);
    }
    Ok(file)
}

/// Removes every staged slot below `root` except the one holding `keep`.
pub(crate) fn retire_others(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.starts_with(&path) {
            continue;
        }
        if let Err(err) = std::fs::remove_dir_all(&path) {
            tracing::warn!(path = %path.display(), error = %err, "could not remove replaced clipboard file");
        }
    }
}

/// Removes the slot of a staged file (cancel / failure path).
pub(crate) fn discard(file: &Path) {
    if let Some(slot) = file.parent() {
        let _ = std::fs::remove_dir_all(slot);
    }
}

/// Startup step: removes what a previous process of this instance left in
/// the staging folder. Best effort, never blocks the start. Runs after
/// `edit_copies::sweep_at_startup` has fixed the per-instance scope.
pub(crate) fn sweep_at_startup() {
    let Some(root) = clipboard_root() else {
        return;
    };
    let report = sweep_dir(&root);
    tracing::info!(
        removed = report.removed,
        failed = report.failed,
        "swept leftover clipboard files of this instance (issue #358)"
    );
}

/// Exit step: empties the staging folder.
pub(crate) fn cleanup_at_exit() {
    if let Some(root) = clipboard_root() {
        let _ = sweep_dir(&root);
    }
}

impl FileClipboard {
    /// Puts `file` on the system clipboard as the only file reference.
    pub(crate) fn set_file(&self, file: &Path) -> Result<(), String> {
        let mut guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(ClipboardContext::new().map_err(|e| e.to_string())?);
        }
        let context = guard.as_ref().expect("context was just created");
        context
            .set_files(vec![file.to_string_lossy().into_owned()])
            .map_err(|e| e.to_string())
    }

    /// Puts plain `text` on the system clipboard (issue #626). Used for text
    /// that only exists after an async step, where the webview no longer
    /// has the user gesture it needs for its own clipboard write.
    pub(crate) fn set_text(&self, text: &str) -> Result<(), String> {
        let mut guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(ClipboardContext::new().map_err(|e| e.to_string())?);
        }
        guard
            .as_ref()
            .expect("context was just created")
            .set_text(text.to_owned())
            .map_err(|e| e.to_string())
    }

    /// Reads the file references currently on the system clipboard (issue
    /// #571). Creates the clipboard context on first use. Only absolute
    /// paths of entries that exist come back; `file://` URIs (X11/Wayland)
    /// are turned into plain paths. An unreadable clipboard is an `Err`.
    pub(crate) fn get_files(&self) -> Result<Vec<PathBuf>, String> {
        let mut guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(ClipboardContext::new().map_err(|e| e.to_string())?);
        }
        let raw = guard
            .as_ref()
            .expect("context was just created")
            .get_files()
            .map_err(|e| e.to_string())?;
        Ok(raw
            .iter()
            .filter_map(|r| clipboard_ref_to_path(r))
            .filter(|p| p.is_absolute() && p.exists())
            .collect())
    }
}

/// `file:///a%20b` -> `/a b`; a plain path stays as it is.
fn clipboard_ref_to_path(raw: &str) -> Option<PathBuf> {
    let Some(rest) = raw.strip_prefix("file://") else {
        return (!raw.is_empty()).then(|| PathBuf::from(raw));
    };
    // Drop an authority such as `localhost`; the path starts at the next `/`.
    let rest = if rest.starts_with('/') {
        rest
    } else {
        &rest[rest.find('/')?..]
    };
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
    }
    #[cfg(not(unix))]
    {
        let text = String::from_utf8(out).ok()?;
        // `/C:/dir/file` -> `C:/dir/file`
        let text = if text.as_bytes().get(2) == Some(&b':') {
            text[1..].to_string()
        } else {
            text
        };
        Some(PathBuf::from(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn test_clipboard_references_become_plain_paths() {
        assert_eq!(
            clipboard_ref_to_path("file:///tmp/a%20b/c%C3%A4.txt"),
            Some(PathBuf::from("/tmp/a b/cä.txt"))
        );
        #[cfg(unix)]
        {
            assert_eq!(
                clipboard_ref_to_path("file://localhost/tmp/x"),
                Some(PathBuf::from("/tmp/x"))
            );
            assert_eq!(
                clipboard_ref_to_path("/tmp/plain"),
                Some(PathBuf::from("/tmp/plain"))
            );
        }
        assert_eq!(clipboard_ref_to_path(""), None);
        // A stray `%` stays literal.
        assert_eq!(
            clipboard_ref_to_path("file:///tmp/100%"),
            Some(PathBuf::from("/tmp/100%"))
        );
    }

    #[test]
    fn test_staged_file_and_folders_are_owner_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("clipboard-files").join("data-1");
        let file = stage_file(&root, "a.bin", b"abc").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"abc");
        assert_eq!(file.file_name().unwrap(), "a.bin");
        #[cfg(unix)]
        {
            assert_eq!(mode(&root), 0o700);
            assert_eq!(mode(file.parent().unwrap()), 0o700);
            assert_eq!(mode(&file), 0o600);
        }
    }

    #[test]
    fn test_new_file_replaces_the_previous_one_only_after_retire() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let first = stage_file(&root, "a.txt", b"1").unwrap();
        let second = stage_file(&root, "a.txt", b"2").unwrap();
        // Both exist until the new one is on the clipboard.
        assert!(first.exists() && second.exists());
        retire_others(&root, &second);
        assert!(!first.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"2");
    }

    #[test]
    fn test_discard_removes_the_slot_and_keeps_the_previous_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let kept = stage_file(&root, "a.txt", b"1").unwrap();
        let cancelled = stage_file(&root, "b.txt", b"2").unwrap();
        discard(&cancelled);
        assert!(!cancelled.exists() && !cancelled.parent().unwrap().exists());
        assert!(kept.exists());
    }

    #[test]
    fn test_sweep_empties_the_folder_for_leftovers_and_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        stage_file(&root, "left.txt", b"x").unwrap();
        stage_file(&root, "over.txt", b"y").unwrap();
        let report = sweep_dir(&root);
        assert_eq!(report.removed, 2);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        // A missing folder is not an error.
        assert_eq!(sweep_dir(&tmp.path().join("none")).failed, 0);
    }

    /// Acceptance: after putting a 5 MB binary file on the clipboard, the
    /// clipboard API returns exactly one file reference with the same
    /// content. Needs a display (Linux: a virtual X server such as
    /// `xvfb-run`). Without one the test is skipped unless
    /// `SMART_SSH_REQUIRE_CLIPBOARD_TESTS=1` (set in CI), where a missing
    /// display is a failure.
    #[test]
    fn test_file_reference_round_trips_through_the_clipboard() {
        let require = std::env::var("SMART_SSH_REQUIRE_CLIPBOARD_TESTS").as_deref() == Ok("1");
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let bytes: Vec<u8> = (0..5 * 1024 * 1024u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let file = stage_file(&root, "blob.bin", &bytes).unwrap();

        let clipboard = FileClipboard::default();
        if let Err(err) = clipboard.set_file(&file) {
            if require {
                panic!("clipboard unavailable but required: {err}");
            }
            eprintln!("skipped: no usable clipboard ({err})");
            return;
        }
        let files = clipboard.get_files().expect("read back");
        assert_eq!(files.len(), 1, "exactly one file reference: {files:?}");
        let got = files[0].clone();
        assert_eq!(
            std::fs::canonicalize(&got).unwrap(),
            std::fs::canonicalize(&file).unwrap()
        );
        assert_eq!(std::fs::read(&got).unwrap(), bytes);
    }
}
