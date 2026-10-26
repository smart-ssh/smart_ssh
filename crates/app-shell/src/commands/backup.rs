//! Issue #664/#666, Spec 0109: encrypted backup — backup key, export and
//! import. The commands only translate between Tauri and `app_logic::backup`.

use serde::Serialize;
use tauri::{AppHandle, State};

use app_logic::backup::{
    export_backup, start_import, BackupKey, BackupKeyStatus, BackupOpError, ExportSummary,
    ImportContext, ImportPreview, ImportStart, ImportSummary, PendingImport, SnapshotCounts,
};
use app_logic::error::{CommandError, CommandResult};
use app_logic::state::AppState;
use ssh_manager_core::backup::BackupError;

/// Counts of an export, never content (S-7).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportDto {
    pub path: String,
    pub servers: usize,
    pub groups: usize,
    pub rules: usize,
    pub notes: usize,
    pub ai_providers: usize,
    pub secrets: usize,
    pub host_keys: usize,
}

fn command_error(err: BackupOpError) -> CommandError {
    let code = match &err {
        BackupOpError::KeyRequired => "BACKUP_KEY_REQUIRED",
        BackupOpError::KeyAlreadyExists => "BACKUP_KEY_EXISTS",
        BackupOpError::Format(BackupError::PassphraseTooShort) => "BACKUP_PASSPHRASE_TOO_SHORT",
        BackupOpError::Format(BackupError::CodeTypo) => "BACKUP_CODE_TYPO",
        BackupOpError::Format(BackupError::CannotOpen) => "BACKUP_CANNOT_OPEN",
        BackupOpError::Format(BackupError::NotABackup) => "BACKUP_NOT_A_BACKUP",
        BackupOpError::Format(BackupError::NewerVersion { .. }) => "BACKUP_NEWER_VERSION",
        BackupOpError::VerificationFailed => "BACKUP_VERIFICATION_FAILED",
        _ => "BACKUP_FAILED",
    };
    CommandError::with_code(err.to_string(), code)
}

/// K4: which slot types exist. No key material.
#[tauri::command]
pub async fn backup_key_status(state: State<'_, AppState>) -> CommandResult<BackupKeyStatus> {
    BackupKey::status(&state.backup_store)
        .await
        .map_err(command_error)
}

/// K2: creates the backup key with a recovery code and returns the code. It
/// is shown exactly once; it cannot be retrieved later.
#[tauri::command]
pub async fn create_backup_key_with_code(state: State<'_, AppState>) -> CommandResult<String> {
    let code = BackupKey::create_with_code(&state.backup_store)
        .await
        .map_err(command_error)?;
    Ok(code.display_string().to_string())
}

/// K5 (and K2's alternative): sets or replaces the passphrase slot.
#[tauri::command]
pub async fn set_backup_passphrase(
    state: State<'_, AppState>,
    passphrase: String,
) -> CommandResult<()> {
    BackupKey::set_passphrase(&state.backup_store, secrecy::SecretString::from(passphrase))
        .await
        .map_err(command_error)
}

/// K6: replaces the recovery code; returns the new code once.
#[tauri::command]
pub async fn new_backup_code(state: State<'_, AppState>) -> CommandResult<String> {
    let code = BackupKey::new_code(&state.backup_store)
        .await
        .map_err(command_error)?;
    Ok(code.display_string().to_string())
}

/// X1–X6: asks for the target with the system save dialog (which asks before
/// overwriting) and writes the backup. Cancelling the dialog is not an error
/// and returns `None`. Without a backup key the command fails with
/// `BACKUP_KEY_REQUIRED` **before** the dialog opens, and creates nothing.
#[tauri::command]
pub async fn export_encrypted_backup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<BackupExportDto>> {
    use tauri_plugin_dialog::DialogExt;

    if !BackupKey::status(&state.backup_store)
        .await
        .map_err(command_error)?
        .exists
    {
        return Err(command_error(BackupOpError::KeyRequired));
    }

    let file_name = format!(
        "smart-ssh-backup-{}.sshbackup",
        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&file_name)
        .add_filter("Smart SSH backup", &["sshbackup"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let handle = crate::local_file::LocalFileHandle::from_dialog(path)?;

    let ExportSummary { counts } = match handle.as_path() {
        Some(path) => export_backup(
            &state.backup_store,
            &state.file_host_keys,
            path,
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(command_error)?,
        None => {
            // `content://` target (Android): write and verify in an
            // app-private directory, then copy through the handle.
            let dir = crate::edit_copies::picked_root()
                .ok_or_else(|| CommandError::from("No cache directory available"))?
                .join(uuid::Uuid::new_v4().to_string());
            let result = export_via_staging(&state, &handle, &app, &dir, &file_name).await;
            remove_staging(&dir);
            result?
        }
    };
    Ok(Some(BackupExportDto {
        path: handle.display(),
        servers: counts.servers,
        groups: counts.groups,
        rules: counts.rules,
        notes: counts.notes,
        ai_providers: counts.ai_providers,
        secrets: counts.secrets,
        host_keys: counts.host_keys,
    }))
}

async fn export_via_staging(
    state: &AppState,
    handle: &crate::local_file::LocalFileHandle,
    app: &AppHandle,
    dir: &std::path::Path,
    file_name: &str,
) -> CommandResult<ExportSummary> {
    std::fs::create_dir_all(dir).map_err(CommandError::from)?;
    let staged = dir.join(file_name);
    let summary = export_backup(
        &state.backup_store,
        &state.file_host_keys,
        &staged,
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .map_err(command_error)?;
    let opener = crate::local_file::platform_opener(app);
    handle.copy_from(opener.as_ref(), &staged).map_err(|err| {
        tracing::warn!(io_kind = ?err.kind(), "backup file could not be written to the chosen location");
        command_error(BackupOpError::Write)
    })?;
    Ok(summary)
}

// ---------------------------------------------------------------- import

/// The file chosen in the backend's own dialog (the path never comes from the
/// frontend) and the validated import waiting for the user's confirmation.
#[derive(Default)]
pub struct BackupImportSlot {
    file: std::sync::Mutex<Option<ChosenFile>>,
    pending: tokio::sync::Mutex<Option<PendingImport>>,
}

/// The file to import and, when it is an app-private copy of a `content://`
/// document (Android), the directory holding that copy.
struct ChosenFile {
    path: std::path::PathBuf,
    staging: Option<std::path::PathBuf>,
}

impl BackupImportSlot {
    /// Forgets the chosen file and deletes its staged copy, if any (the copy
    /// holds encrypted credentials and must not outlive the import).
    fn clear_file(&self) {
        let taken = self.file.lock().unwrap().take();
        if let Some(ChosenFile {
            staging: Some(dir), ..
        }) = taken
        {
            remove_staging(&dir);
        }
    }
}

fn remove_staging(dir: &std::path::Path) {
    if let Err(err) = std::fs::remove_dir_all(dir) {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %err, "could not remove the staged backup copy");
        }
    }
}

/// I3 numbers, never content.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCountsDto {
    pub servers: usize,
    pub groups: usize,
    pub rules: usize,
    pub notes: usize,
    pub ai_providers: usize,
}

impl From<SnapshotCounts> for BackupCountsDto {
    fn from(c: SnapshotCounts) -> Self {
        Self {
            servers: c.servers,
            groups: c.groups,
            rules: c.rules,
            notes: c.notes,
            ai_providers: c.ai_providers,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportPreviewDto {
    pub local: BackupCountsDto,
    pub file: BackupCountsDto,
    pub open_sessions: usize,
    pub safety_backup_dir: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportSummaryDto {
    pub counts: BackupCountsDto,
    pub safety_backup: Option<String>,
    pub previous_data_folder: Option<String>,
    pub host_keys_pending: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportStartDto {
    /// Local data was empty (I2): already imported.
    pub imported: Option<BackupImportSummaryDto>,
    /// Local data exists: confirm first (I3).
    pub preview: Option<BackupImportPreviewDto>,
}

fn preview_dto(p: &ImportPreview) -> BackupImportPreviewDto {
    BackupImportPreviewDto {
        local: p.local.into(),
        file: p.file.into(),
        open_sessions: p.open_sessions,
        safety_backup_dir: p.safety_backup_dir.to_string_lossy().into_owned(),
    }
}

fn summary_dto(s: ImportSummary) -> BackupImportSummaryDto {
    BackupImportSummaryDto {
        counts: s.counts.into(),
        safety_backup: s.safety_backup.map(|p| p.to_string_lossy().into_owned()),
        previous_data_folder: s.previous_data_folder,
        host_keys_pending: s.host_keys_pending,
    }
}

fn default_safety_dir(app: &AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("Smart SSH backups")
}

/// Opens the system file picker. Returns the chosen file's name (for display)
/// or `None` when cancelled; the path stays in the backend.
#[tauri::command]
pub async fn pick_backup_import_file(
    app: AppHandle,
    slot: State<'_, BackupImportSlot>,
    title: String,
) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let builder = app.dialog().file().set_title(&title);
    // Android maps a custom extension to no MIME type, which greys the file
    // out in the system picker; there the file is validated after picking.
    #[cfg(not(target_os = "android"))]
    let builder = builder.add_filter("Smart SSH backup", &["sshbackup"]);
    builder.pick_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let handle = crate::local_file::LocalFileHandle::from_dialog(picked)?;
    let name = handle.file_name();
    // A `content://` URI is copied into an app-private directory (removed
    // after the import or on cancel); a path is used as it is.
    let chosen = match handle.as_path() {
        Some(p) => ChosenFile {
            path: p.to_path_buf(),
            staging: None,
        },
        None => {
            let dir = crate::edit_copies::picked_root()
                .ok_or_else(|| CommandError::from("No cache directory available"))?
                .join(uuid::Uuid::new_v4().to_string());
            let opener = crate::local_file::platform_opener(&app);
            match handle.stage_into(opener.as_ref(), &dir) {
                Ok(path) => ChosenFile {
                    path,
                    staging: Some(dir),
                },
                Err(err) => {
                    remove_staging(&dir);
                    return Err(CommandError::from(err));
                }
            }
        }
    };
    slot.clear_file();
    *slot.file.lock().unwrap() = Some(chosen);
    *slot.pending.lock().await = None;
    Ok(Some(name))
}

/// I1–I3: opens the chosen file with a recovery code (`kind` = `code`) or a
/// passphrase (`passphrase`). Changes nothing unless local data is empty.
#[tauri::command]
pub async fn start_backup_import(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, BackupImportSlot>,
    kind: String,
    secret: String,
) -> CommandResult<BackupImportStartDto> {
    use ssh_manager_core::backup::{BackupSecret, RecoveryCode};
    let Some(file) = slot.file.lock().unwrap().as_ref().map(|c| c.path.clone()) else {
        return Err(CommandError::with_code(
            "no backup file chosen",
            "BACKUP_FILE_REQUIRED",
        ));
    };
    let secret = secrecy::SecretString::from(secret);
    let secret = if kind == "code" {
        use secrecy::ExposeSecret;
        BackupSecret::Code(
            RecoveryCode::parse(secret.expose_secret())
                .map_err(|e| command_error(BackupOpError::Format(e)))?,
        )
    } else {
        BackupSecret::Passphrase(secret)
    };
    let safety_dir = default_safety_dir(&app);
    let sessions = crate::sessions::AppOpenSessions::new(app.clone());
    let ctx = ImportContext {
        store: &state.backup_store,
        host_keys: &state.file_host_keys,
        sessions: &sessions,
        safety_dir: &safety_dir,
        folder_label: "Previous data",
        writer_version: env!("CARGO_PKG_VERSION"),
    };
    match start_import(&ctx, &file, secret)
        .await
        .map_err(command_error)?
    {
        ImportStart::Imported(summary) => {
            slot.clear_file();
            Ok(BackupImportStartDto {
                imported: Some(summary_dto(summary)),
                preview: None,
            })
        }
        ImportStart::NeedsConfirmation(pending) => {
            let preview = preview_dto(pending.preview());
            *slot.pending.lock().await = Some(*pending);
            Ok(BackupImportStartDto {
                imported: None,
                preview: Some(preview),
            })
        }
    }
}

/// I3/I4: after the user confirmed the counts.
#[tauri::command]
pub async fn confirm_backup_import(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, BackupImportSlot>,
) -> CommandResult<BackupImportSummaryDto> {
    // I4.1: without a backup key the safety backup cannot be written; the
    // pending import stays so the user can set the key up and retry.
    if !BackupKey::status(&state.backup_store)
        .await
        .map_err(command_error)?
        .exists
    {
        return Err(command_error(BackupOpError::KeyRequired));
    }
    let Some(pending) = slot.pending.lock().await.take() else {
        return Err(CommandError::with_code(
            "no import is waiting for confirmation",
            "BACKUP_FAILED",
        ));
    };
    // The pending import holds the decrypted content; the staged file is done.
    slot.clear_file();
    let safety_dir = default_safety_dir(&app);
    let sessions = crate::sessions::AppOpenSessions::new(app.clone());
    let ctx = ImportContext {
        store: &state.backup_store,
        host_keys: &state.file_host_keys,
        sessions: &sessions,
        safety_dir: &safety_dir,
        folder_label: "Previous data",
        writer_version: env!("CARGO_PKG_VERSION"),
    };
    let summary = pending.confirm(&ctx).await.map_err(command_error)?;
    Ok(summary_dto(summary))
}

/// Dropping the pending import changes nothing.
#[tauri::command]
pub async fn cancel_backup_import(slot: State<'_, BackupImportSlot>) -> CommandResult<()> {
    *slot.pending.lock().await = None;
    slot.clear_file();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_with(dir: &std::path::Path, staged: bool) -> (BackupImportSlot, std::path::PathBuf) {
        let file = dir.join("a.sshbackup");
        std::fs::write(&file, b"x").unwrap();
        let slot = BackupImportSlot::default();
        *slot.file.lock().unwrap() = Some(ChosenFile {
            path: file.clone(),
            staging: staged.then(|| dir.to_path_buf()),
        });
        (slot, file)
    }

    #[test]
    fn clearing_removes_the_staged_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("stage");
        std::fs::create_dir_all(&staging).unwrap();
        let (slot, file) = slot_with(&staging, true);
        slot.clear_file();
        assert!(!file.exists());
        assert!(!staging.exists());
        assert!(slot.file.lock().unwrap().is_none());
    }

    #[test]
    fn clearing_never_deletes_a_picked_path() {
        let tmp = tempfile::tempdir().unwrap();
        let (slot, file) = slot_with(tmp.path(), false);
        slot.clear_file();
        assert!(file.exists());
        assert!(slot.file.lock().unwrap().is_none());
    }

    #[test]
    fn staging_a_uri_copies_and_clearing_removes_it() {
        // `content://` handle → private copy → removed with the slot.
        struct Opener(std::path::PathBuf);
        impl crate::local_file::UriOpener for Opener {
            fn open(
                &self,
                _uri: &str,
                _a: crate::local_file::Access,
            ) -> std::io::Result<std::fs::File> {
                std::fs::File::open(&self.0)
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::write(&src, b"data").unwrap();
        let handle = crate::local_file::LocalFileHandle::Uri(
            "content://x/document/primary%3ADownload%2Fb.sshbackup".into(),
        );
        let dir = tmp.path().join("stage");
        let path = handle.stage_into(&Opener(src), &dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
        assert_eq!(path.file_name().unwrap(), "b.sshbackup");
        let slot = BackupImportSlot::default();
        *slot.file.lock().unwrap() = Some(ChosenFile {
            path,
            staging: Some(dir.clone()),
        });
        slot.clear_file();
        assert!(!dir.exists());
    }
}
