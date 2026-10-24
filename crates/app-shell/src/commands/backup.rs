//! Issue #664, Spec 0109: encrypted backup — backup key and export. The
//! commands only translate between Tauri and `app_logic::backup`; there is no
//! UI for them in this change.

use serde::Serialize;
use tauri::{AppHandle, State};

use app_logic::backup::{export_backup, BackupKey, BackupKeyStatus, BackupOpError, ExportSummary};
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
    let path = path.into_path()?;

    let ExportSummary { counts } = export_backup(
        &state.backup_store,
        &state.file_host_keys,
        &path,
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .map_err(command_error)?;
    Ok(Some(BackupExportDto {
        path: path.to_string_lossy().into_owned(),
        servers: counts.servers,
        groups: counts.groups,
        rules: counts.rules,
        notes: counts.notes,
        ai_providers: counts.ai_providers,
        secrets: counts.secrets,
        host_keys: counts.host_keys,
    }))
}
