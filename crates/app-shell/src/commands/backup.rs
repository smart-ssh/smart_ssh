//! Issue #664/#666, Spec 0109: encrypted backup — backup key, export and
//! import. The commands only translate between Tauri and `app_logic::backup`.

use serde::Serialize;
use tauri::{AppHandle, State};

use app_logic::backup::{
    export_backup, start_import, BackupKey, BackupKeyStatus, BackupOpError, ExportSummary,
    ImportContext, ImportPreview, ImportStart, ImportSummary, OpenSessions, PendingImport,
    SnapshotCounts,
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

// ---------------------------------------------------------------- import

/// The file chosen in the backend's own dialog (the path never comes from the
/// frontend) and the validated import waiting for the user's confirmation.
#[derive(Default)]
pub struct BackupImportSlot {
    file: std::sync::Mutex<Option<std::path::PathBuf>>,
    pending: tokio::sync::Mutex<Option<PendingImport>>,
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

/// I8: closes the sessions an import replaces, like the close command does.
struct ShellSessions {
    app: AppHandle,
}

#[async_trait::async_trait]
impl OpenSessions for ShellSessions {
    fn affected(&self) -> usize {
        use tauri::Manager;
        self.app.state::<AppState>().sessions.affected()
    }
    async fn close_affected(&self) {
        use tauri::Manager;
        let state = self.app.state::<AppState>();
        let elevated = self
            .app
            .state::<crate::elevated_sftp::ElevatedSftpRegistry>();
        for entry in state.sessions.snapshot() {
            if entry.server_id.0.is_nil() {
                continue;
            }
            let Some(session) = elevated.remove_session(&state.sessions, entry.session_id) else {
                continue;
            };
            session.auto_reconnect.stop();
            app_logic::session::reject_pending_confirmation_on_close(
                entry.session_id,
                Some(session.as_ref()),
                &state.mcp.sessions,
                &state.pending_action_confirmations,
            );
            session.transport.mark_closed();
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                session.transport.lock().await.disconnect(),
            )
            .await;
            *session.terminal.lock().unwrap() = None;
            app_logic::events::emit_connection_status_changed(
                &crate::event_emitter::TauriEventEmitter(self.app.clone()),
                entry.session_id,
                app_logic::events::ConnectionStatus::Disconnected,
                None,
            );
        }
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
    app.dialog()
        .file()
        .set_title(&title)
        .add_filter("Smart SSH backup", &["sshbackup"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    *slot.file.lock().unwrap() = Some(path);
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
    let Some(file) = slot.file.lock().unwrap().clone() else {
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
    let sessions = ShellSessions { app: app.clone() };
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
            *slot.file.lock().unwrap() = None;
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
    *slot.file.lock().unwrap() = None;
    let safety_dir = default_safety_dir(&app);
    let sessions = ShellSessions { app: app.clone() };
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
    *slot.file.lock().unwrap() = None;
    Ok(())
}
