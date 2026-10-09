//! Issue #128 / Spec 0054, part 3: uploading a whole local folder.
//!
//! The walk and its grant checks live in `app_logic::folder_upload`; this
//! file drives the transfer: it creates the remote folders, uploads each
//! file through [`upload_impl`] (one `sftp-transfer-*` event pair per file,
//! like the recursive download) and builds the summary.
//!
//! Nothing existing is overwritten without confirmation: the user confirms
//! a list of remote files (see [`sftp_upload_folder_preview`]); the upload
//! re-checks every target and writes over an existing file only if its path
//! is in that confirmed list. Existing remote folders are reused (merge).

use std::collections::HashSet;

use tauri::{AppHandle, State};

use app_logic::error::{CommandError, CommandResult};
use app_logic::events::EventEmitter;
use app_logic::folder_upload::{
    join_remote, plan_folder, FailedEntry, FolderUploadPlan, FolderUploadPreview,
    FolderUploadSummary, SkipReason, SkippedEntry,
};
use app_logic::local_path_grants::{GrantSnapshot, GrantedLocalPath, LocalPathGrants};
use app_logic::session::{Session, SessionManager};
use app_logic::state::{AppState, SessionId};
use ssh_manager_core::ssh::SftpSession;

use super::{authorize_local_path, grant_for_live_session, upload_impl};
use crate::commands::elevation::{
    audit_elevated_change, lock_browser_sftp, run_browser_command, BrowserChannel,
};
use crate::elevated_sftp::ElevatedSftpRegistry;
use crate::event_emitter::TauriEventEmitter;

/// The upload button's "Upload folder" entry: native folder dialog in the
/// backend (same pattern as `pick_upload_files`); the picked folder is
/// granted to this session. `None` = cancelled.
#[tauri::command]
pub async fn pick_upload_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    title: String,
) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    if state.sessions.get(session_id).is_none() {
        return Err(CommandError::from("Session nicht gefunden"));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path()?;
    Ok(
        grant_for_live_session(&state.sessions, grants.inner(), session_id, vec![path])
            .into_iter()
            .next(),
    )
}

/// Tells the frontend which of the granted `local_paths` are folders, so a
/// drop of mixed files and folders can be routed. A path that is not
/// granted or cannot be read counts as "not a folder" and fails later at
/// the actual upload with the normal refusal.
#[tauri::command]
pub async fn sftp_local_paths_are_folders(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_paths: Vec<String>,
) -> CommandResult<Vec<bool>> {
    let edit_root = crate::edit_copies::instance_root();
    Ok(local_paths_are_folders(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
        &local_paths,
    ))
}

pub(super) fn local_paths_are_folders(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_paths: &[String],
) -> Vec<bool> {
    local_paths
        .iter()
        .map(|p| {
            authorize_local_path(sessions, grants, edit_root, session_id, p)
                .map(|g| g.as_path().is_dir())
                .unwrap_or(false)
        })
        .collect()
}

fn snapshot_for(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
) -> CommandResult<GrantSnapshot> {
    if sessions.get(session_id).is_none() {
        return Err(CommandError::from(
            app_logic::local_path_grants::LOCAL_PATH_NOT_GRANTED,
        ));
    }
    let edit_dir = super::edit_session_dir_in(edit_root, session_id).ok();
    Ok(grants.snapshot(session_id, edit_dir.as_deref()))
}

async fn plan_blocking(
    root: &GrantedLocalPath,
    snapshot: &GrantSnapshot,
) -> CommandResult<FolderUploadPlan> {
    let (root, snapshot) = (root.clone(), snapshot.clone());
    tokio::task::spawn_blocking(move || plan_folder(&root, &snapshot))
        .await
        .map_err(|e| format!("Hintergrund-Task für Ordner-Upload fehlgeschlagen: {e}"))?
}

/// `Some(is_dir)` if the remote path exists, `None` otherwise.
async fn remote_kind(sftp: &mut impl SftpSession, path: &str) -> Option<bool> {
    sftp.stat(path).await.ok().map(|e| e.is_dir)
}

/// First step of a folder upload: what would be uploaded, and which
/// existing remote files it would overwrite. Writes nothing.
#[tauri::command]
pub async fn sftp_upload_folder_preview(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
    remote_dir: String,
    elevated_user: Option<String>,
) -> CommandResult<FolderUploadPreview> {
    let edit_root = crate::edit_copies::instance_root();
    let root = authorize_local_path(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
        &local_path,
    )?;
    let snapshot = snapshot_for(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
    )?;
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            preview_impl(&session, &channel, &snapshot, &root, &remote_dir).await
        },
    )
    .await
}

pub(super) async fn preview_impl(
    session: &Session,
    channel: &BrowserChannel,
    snapshot: &GrantSnapshot,
    root: &GrantedLocalPath,
    remote_dir: &str,
) -> CommandResult<FolderUploadPreview> {
    let plan = plan_blocking(root, snapshot).await?;
    let remote_root = join_remote(remote_dir, &plan.folder_name);
    let mut overwrites = Vec::new();
    {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        for file in &plan.files {
            let remote = join_remote(&remote_root, &file.relative);
            // An existing *folder* at a file's path is not an overwrite;
            // the upload reports it as a failure for that file.
            if remote_kind(&mut sftp, &remote).await == Some(false) {
                overwrites.push(remote);
            }
        }
    }
    Ok(FolderUploadPreview {
        folder_name: plan.folder_name,
        file_count: plan.files.len() as u32,
        folder_count: plan.dirs.len() as u32,
        overwrites,
        skipped: plan.skipped,
    })
}

/// Second step: the upload. `confirmed_overwrites` are the remote paths the
/// user saw in the preview and agreed to overwrite; any other existing file
/// is left untouched and listed as skipped.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn sftp_upload_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
    remote_dir: String,
    confirmed_overwrites: Vec<String>,
    elevated_user: Option<String>,
) -> CommandResult<FolderUploadSummary> {
    let edit_root = crate::edit_copies::instance_root();
    let root = authorize_local_path(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
        &local_path,
    )?;
    let snapshot = snapshot_for(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
    )?;
    let confirmed: HashSet<String> = confirmed_overwrites.into_iter().collect();
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            upload_folder_impl(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &snapshot,
                &root,
                &remote_dir,
                &confirmed,
            )
            .await
        },
    )
    .await
}

/// Makes sure a remote folder exists. `Ok(true)` = created now.
async fn ensure_remote_dir(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<bool> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    match remote_kind(&mut sftp, path).await {
        Some(true) => Ok(false),
        Some(false) => Err(CommandError::from(
            "Am Zielpfad existiert bereits eine Datei, kein Ordner.",
        )),
        None => {
            let result = sftp.create_dir(path).await;
            audit_elevated_change(elevated_user.as_deref(), "mkdir", path, result.is_ok());
            result?;
            Ok(true)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn upload_folder_impl(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    snapshot: &GrantSnapshot,
    root: &GrantedLocalPath,
    remote_dir: &str,
    confirmed_overwrites: &HashSet<String>,
) -> CommandResult<FolderUploadSummary> {
    let plan = plan_blocking(root, snapshot).await?;
    let remote_root = join_remote(remote_dir, &plan.folder_name);
    let mut summary = FolderUploadSummary {
        folder_name: plan.folder_name.clone(),
        skipped: plan.skipped.clone(),
        ..Default::default()
    };

    // Without the target folder there is nothing to continue with.
    if ensure_remote_dir(session, channel, &remote_root).await? {
        summary.folders_created += 1;
    }
    let mut failed_dirs: Vec<String> = Vec::new();
    for dir in &plan.dirs {
        if failed_dirs
            .iter()
            .any(|f| dir.starts_with(&format!("{f}/")))
        {
            continue;
        }
        match ensure_remote_dir(session, channel, &join_remote(&remote_root, dir)).await {
            Ok(created) => summary.folders_created += u32::from(created),
            Err(err) => {
                summary.failed.push(FailedEntry {
                    path: dir.clone(),
                    error: channel.transfer_error_message(&err),
                });
                failed_dirs.push(dir.clone());
            }
        }
    }

    for file in &plan.files {
        if failed_dirs
            .iter()
            .any(|f| file.relative.starts_with(&format!("{f}/")))
        {
            summary.not_attempted += 1;
            continue;
        }
        let remote = join_remote(&remote_root, &file.relative);
        let existing = {
            let mut guard = lock_browser_sftp(session, channel).await;
            match guard.sftp().await {
                Ok(mut sftp) => Ok(remote_kind(&mut sftp, &remote).await),
                Err(err) => Err(err),
            }
        };
        match existing {
            Err(err) => {
                summary.failed.push(FailedEntry {
                    path: file.relative.clone(),
                    error: channel.transfer_error_message(&err),
                });
                continue;
            }
            Ok(Some(true)) => {
                summary.failed.push(FailedEntry {
                    path: file.relative.clone(),
                    error: "Am Zielpfad existiert bereits ein Ordner.".to_string(),
                });
                continue;
            }
            Ok(Some(false)) if !confirmed_overwrites.contains(&remote) => {
                summary.skipped.push(SkippedEntry {
                    path: file.relative.clone(),
                    reason: SkipReason::OverwriteNotConfirmed,
                });
                continue;
            }
            Ok(_) => {}
        }
        // The grant is checked again right before the read: the file may
        // have been replaced (e.g. by a link) since the walk.
        let (snap, path) = (snapshot.clone(), file.local.as_path().to_path_buf());
        let fresh = tokio::task::spawn_blocking(move || snap.check(&path))
            .await
            .map_err(|e| format!("Hintergrund-Task für Ordner-Upload fehlgeschlagen: {e}"))?;
        let result = match fresh {
            Ok(local) => upload_impl(emitter, session, channel, session_id, &local, &remote).await,
            Err(err) => Err(err),
        };
        match result {
            Ok(()) => summary.files_uploaded += 1,
            Err(err) => summary.failed.push(FailedEntry {
                path: file.relative.clone(),
                error: channel.transfer_error_message(&err),
            }),
        }
    }
    Ok(summary)
}
