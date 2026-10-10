//! The single write path for `settings.json` on the backend (issue #265).
//!
//! The Tauri store caches `settings.json` in the process: `set`/`delete`
//! take effect in memory at once, only `save` can fail. Every backend
//! writer therefore goes through [`update`], which
//!
//! - refuses to write when the file on disk cannot be parsed (the store
//!   plugin silently turns an unreadable file into an *empty* store, and the
//!   next save would overwrite the broken file and drop all other
//!   settings),
//! - records the previous value of every key it changes,
//! - restores those values in memory when `save` fails, so the process does
//!   not behave as if the value had been saved while the file still holds
//!   the old one,
//! - applies owner-only permissions after a successful save (issue #40).
//!
//! Reading is unchanged and keeps its fail-safe defaults.

use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::{Store, StoreExt};

use app_logic::error::{CommandError, CommandResult};

/// File name of the settings store, shared by all modules.
pub(crate) const SETTINGS_STORE_FILE: &str = "settings.json";

/// Where `settings.json` lives: via `tauri_plugin_store::resolve_store_path`,
/// the same resolution the store uses when reading and writing
/// (`BaseDirectory::AppData`). Deliberately not `app_config_dir`: on Linux
/// that is a different directory (`~/.config` instead of `~/.local/share`),
/// see ADR 0105. Single source of truth for this path — used by the
/// permission hardening and by the data paths display
/// (`commands::diagnostics_export`).
pub(crate) fn settings_store_path<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri_plugin_store::Result<std::path::PathBuf> {
    tauri_plugin_store::resolve_store_path(app, SETTINGS_STORE_FILE)
}

/// Sets `settings.json` to owner-only (0600) on Unix, best-effort: a failed
/// path resolution or `set_permissions` is ignored and never fails the
/// calling command.
pub(crate) fn harden_settings_store_permissions<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(unix)]
    {
        let Ok(path) = settings_store_path(app) else {
            return;
        };
        ssh_manager_core::fs_hardening::harden_permissions(&path, 0o600, "settings");
    }
    #[cfg(not(unix))]
    {
        let _ = app;
    }
}

/// Collects the changes of one [`update`] call and remembers the previous
/// value of each key, so they can be rolled back in memory.
pub(crate) struct Changes<R: Runtime> {
    store: Arc<Store<R>>,
    previous: Vec<(String, Option<Value>)>,
}

impl<R: Runtime> Changes<R> {
    pub(crate) fn set(&mut self, key: &str, value: Value) {
        self.previous.push((key.to_string(), self.store.get(key)));
        self.store.set(key, value);
    }

    pub(crate) fn delete(&mut self, key: &str) {
        self.previous.push((key.to_string(), self.store.get(key)));
        self.store.delete(key);
    }

    fn roll_back(self) {
        for (key, previous) in self.previous.into_iter().rev() {
            match previous {
                Some(value) => self.store.set(key, value),
                None => {
                    self.store.delete(key);
                }
            }
        }
    }
}

/// Checks that the file on disk, if present, is a JSON object (an empty file
/// counts as nothing to lose). Returns whether it holds any entries.
///
/// The store plugin writes by truncating and rewriting the file (also from a
/// debounced auto-save), so a read can catch a half-written file. A failed
/// parse is therefore retried a few times before the file counts as broken.
fn check_file_loadable(path: &std::path::Path) -> CommandResult<bool> {
    const ATTEMPTS: u32 = 5;
    for attempt in 1..=ATTEMPTS {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(false);
        }
        if let Ok(map) = serde_json::from_slice::<serde_json::Map<String, Value>>(&bytes) {
            return Ok(!map.is_empty());
        }
        if attempt < ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    Err(CommandError::from(format!(
        "{SETTINGS_STORE_FILE} could not be read and was left unchanged; \
         repair or remove the file and try again"
    )))
}

/// Applies `change` to the settings store and saves it. On a failed save the
/// previous in-memory values are restored and the error is returned.
pub(crate) fn update<R: Runtime>(
    app: &AppHandle<R>,
    change: impl FnOnce(&mut Changes<R>),
) -> CommandResult<()> {
    let path = settings_store_path(app)?;
    let file_has_entries = check_file_loadable(&path)?;
    let store = app.store(SETTINGS_STORE_FILE)?;
    if file_has_entries && store.is_empty() {
        // The store was opened while the file was unreadable and is cached
        // empty; pick up the repaired file before writing on top of it.
        store.reload()?;
    }

    let mut changes = Changes {
        store: store.clone(),
        previous: Vec::new(),
    };
    change(&mut changes);

    // The one place that saves the settings store.
    if let Err(err) = store.save() {
        changes.roll_back();
        return Err(err.into());
    }
    harden_settings_store_permissions(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_retention::{read_retention_days, write_retention_days};
    use crate::first_run_notice::test_support::{lock, test_app};
    use crate::local_server::{save_notes, save_tags};

    /// The tests share the real `settings.json` (see
    /// `first_run_notice::test_support`). Restores its content and mode when
    /// the test ends, also on a panic.
    struct FileGuard {
        path: std::path::PathBuf,
        content: Option<Vec<u8>>,
    }

    impl FileGuard {
        fn new<R: Runtime>(app: &AppHandle<R>) -> Self {
            let path = settings_store_path(app).expect("store path must resolve");
            let content = std::fs::read(&path).ok();
            Self { path, content }
        }
    }

    impl Drop for FileGuard {
        fn drop(&mut self) {
            // `set` schedules a debounced (100 ms) auto-save in the store
            // plugin; let it fire before restoring, or it would overwrite
            // the restored file afterwards.
            std::thread::sleep(std::time::Duration::from_millis(400));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
            }
            match &self.content {
                Some(content) => {
                    let _ = std::fs::write(&self.path, content);
                }
                None => {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }
    }

    /// Measured behaviour (issue #265): with a syntactically broken
    /// `settings.json` the plugin's `store()` does **not** fail; it returns
    /// an empty store. A save on it would overwrite the broken file and
    /// drop every other setting.
    #[test]
    fn test_store_open_on_broken_file_yields_empty_store_not_an_error() {
        let _guard = lock();
        let probe = test_app();
        let _file = FileGuard::new(probe.handle());
        let path = settings_store_path(probe.handle()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"chatSessionRetentionDays": 30, "#).unwrap();

        // A fresh app has a fresh, not yet loaded store registry.
        let app = test_app();
        let store = app
            .handle()
            .store(SETTINGS_STORE_FILE)
            .expect("the plugin returns Ok for a broken file");
        assert!(store.is_empty(), "broken file is loaded as an empty store");
    }

    #[test]
    fn test_write_to_broken_file_fails_and_leaves_file_unchanged() {
        let _guard = lock();
        let probe = test_app();
        let _file = FileGuard::new(probe.handle());
        let path = settings_store_path(probe.handle()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let broken: &[u8] = br#"{"chatSessionRetentionDays": 30, "otherSetting": tru"#;
        std::fs::write(&path, broken).unwrap();

        let app = test_app();
        let handle = app.handle();

        assert!(write_retention_days(handle, Some(7)).is_err());
        assert!(save_notes(handle, "note").is_err());
        assert!(save_tags(handle, &["a".to_string()]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), broken);
    }

    #[test]
    fn test_write_after_repair_keeps_other_settings() {
        let _guard = lock();
        let probe = test_app();
        let _file = FileGuard::new(probe.handle());
        let path = settings_store_path(probe.handle()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ broken").unwrap();

        let app = test_app();
        let handle = app.handle();
        assert!(write_retention_days(handle, Some(7)).is_err());

        // The file is repaired while the empty store is cached.
        std::fs::write(&path, br#"{"keepMe": true}"#).unwrap();
        write_retention_days(handle, Some(7)).unwrap();

        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk["keepMe"], serde_json::json!(true));
        assert_eq!(on_disk["chatSessionRetentionDays"], serde_json::json!(7));
    }

    #[test]
    fn test_empty_or_missing_file_is_written() {
        let _guard = lock();
        let probe = test_app();
        let _file = FileGuard::new(probe.handle());
        let path = settings_store_path(probe.handle()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"").unwrap();

        let app = test_app();
        write_retention_days(app.handle(), Some(5)).unwrap();
        assert_eq!(read_retention_days(app.handle()), Some(5));
    }

    /// A failed save must leave the in-memory value at the previous one and
    /// return the error. Against the old writers (`set` then `save`) the
    /// cached store kept the new value.
    #[cfg(unix)]
    #[test]
    fn test_failed_save_restores_previous_values_in_memory() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = lock();
        // The guard must outlive `app`: dropping a test app saves its cached
        // stores, which would otherwise overwrite the restored file.
        let probe = test_app();
        let _file = FileGuard::new(probe.handle());
        let app = test_app();
        let handle = app.handle();

        write_retention_days(handle, Some(3)).unwrap();
        save_notes(handle, "old note").unwrap();
        save_tags(handle, &["old".to_string()]).unwrap();
        let path = settings_store_path(handle).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
            // Running as root: file modes do not stop the write.
            return;
        }

        assert!(write_retention_days(handle, Some(99)).is_err());
        assert_eq!(read_retention_days(handle), Some(3));
        assert!(write_retention_days(handle, None).is_err());
        assert_eq!(read_retention_days(handle), Some(3));
        assert!(save_notes(handle, "new note").is_err());
        assert!(save_tags(handle, &["new".to_string()]).is_err());

        let store = handle.store(SETTINGS_STORE_FILE).unwrap();
        assert_eq!(
            store.get("localServerNotes"),
            Some(serde_json::json!("old note"))
        );
        assert_eq!(
            store.get("localServerTags"),
            Some(serde_json::json!(["old"]))
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_successful_writes_leave_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = lock();
        // The guard must outlive `app`: dropping a test app saves its cached
        // stores, which would otherwise overwrite the restored file.
        let probe = test_app();
        let _file = FileGuard::new(probe.handle());
        let app = test_app();
        let handle = app.handle();
        let path = settings_store_path(handle).unwrap();
        let mode = || std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        write_retention_days(handle, Some(1)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_retention_days(handle, Some(2)).unwrap();
        assert_eq!(mode(), 0o600, "chat_retention write");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        save_notes(handle, "n").unwrap();
        assert_eq!(mode(), 0o600, "local_server notes write");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        save_tags(handle, &["t".to_string()]).unwrap();
        assert_eq!(mode(), 0o600, "local_server tags write");
    }
}
