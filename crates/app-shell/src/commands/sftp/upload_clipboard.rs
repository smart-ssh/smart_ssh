use super::*;

/// Issue #89: `local_path` comes from the webview and proves nothing on its
/// own — the webview also renders AI-generated content. It is read only if
/// it lies inside a grant of this session (native open dialog via
/// [`pick_upload_files`], native drop via [`claim_dropped_paths`], or the
/// session's own edit-session directory), see
/// `app_logic::local_path_grants`. Any other path fails before a channel
/// is opened, anything is read or a transfer starts.
// Tauri command arguments are the IPC interface; grouping them into a
// struct would change the frontend call shape.
#[expect(clippy::too_many_arguments, reason = "one argument per input")]
#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    let edit_root = crate::edit_copies::instance_root();
    let local_path = authorize_local_path(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
        &local_path,
    )?;
    let staged = local_path.as_path().to_path_buf();
    let result = run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            upload_impl(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &local_path,
                &remote_path,
            )
            .await
        },
    )
    .await;
    // Issue #679: a copy staged from the Android picker is not needed after
    // the upload (success or not); real files are never inside this root.
    if let Some(root) = crate::edit_copies::picked_root() {
        crate::edit_copies::remove_staged_pick(&staged, &root);
    }
    result
}

/// Issue #89: the one place where a frontend-supplied local path becomes a
/// [`GrantedLocalPath`]. The session must still exist (grants die with it,
/// and a leftover edit-session directory of a closed session must not count
/// either), then the path must lie inside a grant of exactly this session.
///
/// `edit_root`: the edit-copy root, see [`edit_session_dir_in`]. Every
/// command passes [`crate::edit_copies::instance_root`].
pub(super) fn authorize_local_path(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<GrantedLocalPath> {
    if sessions.get(session_id).is_none() {
        return Err(CommandError::from(LOCAL_PATH_NOT_GRANTED));
    }
    let edit_dir = edit_session_dir_in(edit_root, session_id).ok();
    grants.check(
        session_id,
        std::path::Path::new(local_path),
        edit_dir.as_deref(),
    )
}

/// Issue #89: the upload button's native open dialog, run in the backend
/// (pattern: `read_credential_file`). The frontend passes only a title;
/// the picked paths are granted to this session and returned. `None` =
/// cancelled. Files only — folders go through `pick_upload_folder` (ADR 0144).
#[tauri::command]
pub async fn pick_upload_files(
    app: AppHandle,
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    title: String,
) -> CommandResult<Option<Vec<String>>> {
    use tauri_plugin_dialog::DialogExt;

    app_logic::session::require_session(&state.sessions, session_id)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_files(move |paths| {
            let _ = tx.send(paths);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None); // Abbrechen ist kein Fehler.
    };
    // Issue #679: `content://` URIs (Android) are copied into an app-private
    // staging directory; the copies are what gets granted and uploaded.
    let staging = crate::edit_copies::picked_root().map(|r| r.join(Uuid::new_v4().to_string()));
    let mut paths = Vec::with_capacity(picked.len());
    for item in picked {
        let handle = crate::local_file::LocalFileHandle::from_dialog(item)?;
        let path = match handle.as_path() {
            Some(p) => p.to_path_buf(),
            None => {
                let dir = staging
                    .as_deref()
                    .ok_or_else(|| CommandError::from("No cache directory available"))?;
                // Same display name twice: keep both copies apart.
                let dir = dir.join(paths.len().to_string());
                let h = handle.clone();
                let o = crate::local_file::platform_opener(&app);
                tokio::task::spawn_blocking(move || h.stage_into(o.as_ref(), &dir))
                    .await
                    .map_err(|e| CommandError::from(e.to_string()))??
            }
        };
        paths.push(path);
    }
    Ok(Some(grant_for_live_session(
        &state.sessions,
        grants.inner(),
        session_id,
        paths,
    )))
}

/// Issue #89: hands the most recent native drop (captured from the window's
/// drag-and-drop event in the backend, see `crate::run`) to this session's
/// file browser and grants its paths. The frontend sends no paths — only
/// which session claims the drop. Empty list = no fresh drop to claim.
#[tauri::command]
pub async fn claim_dropped_paths(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
) -> CommandResult<Vec<String>> {
    claim_dropped_paths_impl(&state.sessions, grants.inner(), session_id)
}

pub(super) fn claim_dropped_paths_impl(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    session_id: SessionId,
) -> CommandResult<Vec<String>> {
    app_logic::session::require_session(sessions, session_id)?;
    Ok(grants
        .claim_drop(session_id)
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Issue #571: whether the OS clipboard holds at least one local file or
/// folder reference. Returns no paths — it only decides whether the empty-
/// space menu offers "Paste". An unreadable clipboard counts as "no files".
#[tauri::command]
pub async fn clipboard_has_files(
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
) -> CommandResult<bool> {
    match clipboard.get_files() {
        Ok(files) => Ok(!files.is_empty()),
        Err(err) => {
            tracing::debug!(error = %err, "clipboard could not be read for the paste check");
            Ok(false)
        }
    }
}

/// Issue #626: writes `text` to the OS clipboard from the backend. The
/// webview refuses `navigator.clipboard.writeText` (WebKit) once the user
/// gesture is gone, e.g. after awaiting file content.
#[tauri::command]
pub async fn clipboard_write_text(
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
    text: String,
) -> CommandResult<()> {
    clipboard.set_text(&text).map_err(|e| {
        tracing::debug!(error = %e, "clipboard text write failed");
        CommandError::from("The clipboard could not be written")
    })
}

/// Texts of the paste confirmation dialog; the frontend supplies the
/// localized wording, the file list and target are added by the backend.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardPasteDialogTexts {
    pub title: String,
    pub intro: String,
    pub target_label: String,
    pub more_label: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

/// Maximum number of names listed in the confirmation dialog.
pub(super) const PASTE_DIALOG_MAX_NAMES: usize = 12;

pub(super) fn paste_dialog_message(
    texts: &ClipboardPasteDialogTexts,
    files: &[std::path::PathBuf],
    target_dir: &str,
) -> String {
    let mut message = format!("{}\n", texts.intro);
    for file in files.iter().take(PASTE_DIALOG_MAX_NAMES) {
        message.push_str(&format!("\n• {}", file.display()));
    }
    if files.len() > PASTE_DIALOG_MAX_NAMES {
        message.push_str(&format!(
            "\n• {}",
            texts.more_label.replace(
                "{count}",
                &(files.len() - PASTE_DIALOG_MAX_NAMES).to_string()
            )
        ));
    }
    message.push_str(&format!("\n\n{}: {}", texts.target_label, target_dir));
    message
}

/// Core of [`claim_clipboard_files`]: nothing is granted unless `confirm`
/// returns `true` (the user clicked the native dialog's confirm button) and
/// the session still exists. `None` = nothing on the clipboard; `Some(empty)`
/// = cancelled.
pub(super) async fn claim_clipboard_files_impl<F, Fut>(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    session_id: SessionId,
    files: Vec<std::path::PathBuf>,
    confirm: F,
) -> CommandResult<Option<Vec<String>>>
where
    F: FnOnce(Vec<std::path::PathBuf>) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    app_logic::session::require_session(sessions, session_id)?;
    if files.is_empty() {
        return Ok(None);
    }
    if !confirm(files.clone()).await {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(grant_for_live_session(
        sessions, grants, session_id, files,
    )))
}

/// Issue #571: "Paste" of local files from the OS clipboard as an upload.
/// The backend reads the clipboard itself, shows a native confirmation that
/// lists the files and the target directory, and only after the user
/// confirmed grants the paths to this session and returns them (third grant
/// source next to the open dialog and the native drop, ADR 0113/0145). The
/// webview sends no local path; `target_dir` is display text only.
/// `None` = nothing on the clipboard, empty list = cancelled.
#[tauri::command]
pub async fn claim_clipboard_files(
    app: AppHandle,
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
    session_id: SessionId,
    target_dir: String,
    texts: ClipboardPasteDialogTexts,
) -> CommandResult<Option<Vec<String>>> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

    app_logic::session::require_session(&state.sessions, session_id)?;
    let files = clipboard.get_files().map_err(|err| {
        tracing::debug!(error = %err, "clipboard could not be read for paste");
        CommandError::from("The clipboard could not be read")
    })?;
    claim_clipboard_files_impl(
        &state.sessions,
        grants.inner(),
        session_id,
        files,
        |files| async move {
            let message = paste_dialog_message(&texts, &files, &target_dir);
            let (tx, rx) = tokio::sync::oneshot::channel();
            app.dialog()
                .message(message)
                .title(&texts.title)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    texts.confirm_label.clone(),
                    texts.cancel_label.clone(),
                ))
                .show(move |confirmed| {
                    let _ = tx.send(confirmed);
                });
            // A dialog that vanishes without an answer counts as cancel.
            rx.await.unwrap_or(false)
        },
    )
    .await
}

/// Grants `paths` only if the session is still there once the dialog
/// returns — a session closed while the dialog was open gets no grants.
/// A disconnect between this check and the grant leaves an entry for a
/// session id that is never used again; every reader checks that the
/// session exists ([`authorize_local_path`]).
pub(super) fn grant_for_live_session(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    session_id: SessionId,
    paths: Vec<std::path::PathBuf>,
) -> Vec<String> {
    if sessions.get(session_id).is_none() {
        return Vec::new();
    }
    grants
        .grant(session_id, paths)
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Spec 0086, T8: `emitter` statt eines `AppHandle` — s.
/// [`download_one_file`].
pub(super) async fn upload_impl(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    local_path: &GrantedLocalPath,
    remote_path: &str,
) -> CommandResult<()> {
    // Issue #622: in Stücken, mit Fortschritt und Abbruch — s. `transfer`.
    let transfer_id = Uuid::new_v4();
    let guard = channel.register_op(transfer_id, OpKind::Upload, remote_path);
    let run = FileRun {
        emitter,
        session,
        channel,
        session_id,
        op: guard.handle(),
        folder: false,
    };
    upload_file(&run, transfer_id, local_path.as_path(), remote_path, 0).await?;
    Ok(())
}
