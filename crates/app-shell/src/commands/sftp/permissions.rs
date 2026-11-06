use super::*;

pub(super) use app_logic::sftp_ops::{chmod_recursive, chown_recursive, resolve_chown_ids};

/// Spec 0054, Teil 3: chmod. `recursive` gilt nur für Ordner (bei einer
/// Datei ignoriert der Aufrufer das Frontend-seitig ohnehin, s. dortiger
/// Dialog) — läuft denselben Verzeichnisbaum wie `sftp_delete` ab und setzt
/// dieselben Rechte auf **jeden** gefundenen Eintrag (Dateien UND
/// Verzeichnisse selbst), nicht nur auf Blätter.
#[tauri::command]
#[expect(clippy::too_many_arguments, reason = "one argument per input")] // one argument per IPC parameter
pub async fn sftp_chmod(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    mode: u32,
    recursive: bool,
    apply_special_bits: Option<bool>,
    elevated_user: Option<String>,
) -> CommandResult<u64> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let emitter = TauriEventEmitter(app);
            chmod_impl_reporting(
                Some((&emitter, session_id)),
                &session,
                &channel,
                &path,
                mode,
                recursive,
                apply_special_bits.unwrap_or(false),
            )
            .await
        },
    )
    .await
}

#[cfg(test)]
pub(super) async fn chmod_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
) -> CommandResult<u64> {
    chmod_impl_reporting(
        None,
        session,
        channel,
        path,
        mode,
        recursive,
        apply_special_bits,
    )
    .await
}

/// Issue #658: s. `delete_impl_reporting` (kind `chmod`).
pub(super) async fn chmod_impl_reporting(
    progress: Option<(&dyn EventEmitter, SessionId)>,
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
) -> CommandResult<u64> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4 — s. `delete_impl`.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let op = channel.register_op(Uuid::new_v4(), OpKind::Chmod, path);
    let mut rows = EntryRows::new(progress, SftpTransferKind::Chmod, op.handle(), path);
    let result = rows
        .run(chmod_recursive(
            &mut sftp,
            path,
            mode,
            recursive,
            apply_special_bits,
            op.handle(),
        ))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    audit_elevated_change(
        elevated_user.as_ref(),
        "chmod",
        path,
        matches!(result, Ok(Some(_))),
    );
    result?.ok_or_else(stopped_error)
}

/// Issue #654: change owner and/or group. `owner`/`group` are a name (looked
/// up in the server's `/etc/passwd`/`/etc/group`) or a number; `None` or an
/// empty text leaves that side unchanged. Names are resolved **before**
/// anything is changed: an unknown name fails with nothing touched.
/// `recursive` applies to folders only and never follows symlinks.
#[tauri::command]
#[expect(clippy::too_many_arguments, reason = "one argument per input")] // one argument per IPC parameter
pub async fn sftp_chown(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    owner: Option<String>,
    group: Option<String>,
    recursive: bool,
    elevated_user: Option<String>,
) -> CommandResult<u64> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let emitter = TauriEventEmitter(app);
            chown_impl_reporting(
                Some((&emitter, session_id)),
                &session,
                &channel,
                &path,
                owner.as_deref(),
                group.as_deref(),
                recursive,
            )
            .await
        },
    )
    .await
}

#[cfg(test)]
pub(super) async fn chown_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    owner: Option<&str>,
    group: Option<&str>,
    recursive: bool,
) -> CommandResult<u64> {
    chown_impl_reporting(None, session, channel, path, owner, group, recursive).await
}

pub(super) async fn chown_impl_reporting(
    progress: Option<(&dyn EventEmitter, SessionId)>,
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    owner: Option<&str>,
    group: Option<&str>,
    recursive: bool,
) -> CommandResult<u64> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4 — s. `delete_impl`.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let (uid, gid) = resolve_chown_ids(&mut sftp, owner, group).await?;
    let op = channel.register_op(Uuid::new_v4(), OpKind::Chown, path);
    let mut rows = EntryRows::new(progress, SftpTransferKind::Chown, op.handle(), path);
    let result = rows
        .run(chown_recursive(
            &mut sftp,
            path,
            uid,
            gid,
            recursive,
            op.handle(),
        ))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    audit_elevated_change(
        elevated_user.as_ref(),
        "chown",
        path,
        matches!(result, Ok(Some(_))),
    );
    result?.ok_or_else(stopped_error)
}
