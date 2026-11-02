use super::*;

/// Spec 0054, Teil 3: chmod. `recursive` gilt nur für Ordner (bei einer
/// Datei ignoriert der Aufrufer das Frontend-seitig ohnehin, s. dortiger
/// Dialog) — läuft denselben Verzeichnisbaum wie `sftp_delete` ab und setzt
/// dieselben Rechte auf **jeden** gefundenen Eintrag (Dateien UND
/// Verzeichnisse selbst), nicht nur auf Blätter.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // one argument per IPC parameter
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
        elevated_user.as_deref(),
        "chmod",
        path,
        matches!(result, Ok(Some(_))),
    );
    result?.ok_or_else(stopped_error)
}

/// Issue #624: the mode one entry gets during a recursive apply (like
/// `chmod -R` with capital `X`). Files get exactly the chosen rwx bits and
/// never a special bit. Folders get the chosen rwx bits plus execute for
/// every class that has read, so they stay enterable; their special bits
/// stay as they are unless the user changed them (`apply_special_bits`),
/// in which case the chosen ones apply to folders only.
pub(super) fn recursive_entry_mode(
    entry: &RemoteEntry,
    mode: u32,
    apply_special_bits: bool,
) -> u32 {
    let rwx = mode & 0o777;
    if !entry.is_dir {
        return rwx;
    }
    let rwx = rwx | ((rwx & 0o444) >> 2);
    let special = if apply_special_bits {
        mode & 0o7000
    } else {
        entry.permissions & 0o7000
    };
    rwx | special
}

/// Eigentliche Rekursions-Logik hinter `sftp_chmod` — s. `delete_recursive`s
/// Doc-Kommentar zum selben Testbarkeits-Muster. Liefert die Anzahl der
/// geänderten Einträge (Spec 0067, Teil B: Sammelmeldung).
pub(super) async fn chmod_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
    op: &OpHandle,
) -> Result<Option<u64>, SshError> {
    if !recursive {
        sftp.set_permissions(path, mode).await?;
        return Ok(Some(1));
    }
    // `lstat` statt `stat` — dieselbe Symlink-Begründung wie in
    // `delete_recursive`: ein Symlink auf ein Verzeichnis darf ein
    // rekursives chmod nicht in dessen Ziel hinein eskalieren lassen.
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.set_permissions(path, mode).await?;
        return Ok(Some(1));
    }

    // Erst den ganzen Baum LESEND ablaufen (mit den unveränderten
    // Original-Rechten), ALLE Pfade sammeln, und die Rechte erst danach in
    // einem zweiten Durchlauf setzen. Ohne diese Trennung würde ein bereits
    // umgesetztes Verzeichnis — z. B. `mode` ohne Owner-Execute-Bit — die
    // eigene weitere Traversierung blockieren (ein Verzeichnis ohne `x` für
    // den eigenen Owner lässt sich unter Unix selbst vom Owner-Prozess
    // nicht mehr auflisten), sobald es als Nächstes an der Reihe wäre.
    op.begin_entries();
    let mut all_paths = vec![(
        path.to_string(),
        recursive_entry_mode(&root_entry, mode, apply_special_bits),
    )];
    let mut queue = vec![path.to_string()];
    while let Some(dir) = queue.pop() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        let entries = sftp.list_dir(&dir).await?;
        for entry in entries {
            all_paths.push((
                entry.path.clone(),
                recursive_entry_mode(&entry, mode, apply_special_bits),
            ));
            if entry.is_dir {
                queue.push(entry.path);
            }
        }
    }
    // Rückwärts (tiefste zuerst, Wurzel zuletzt) — dieselbe Begründung wie
    // oben: sobald die Wurzel selbst ihr Execute-Bit verliert, lässt sich
    // kein Pfad *unter* ihr mehr auflösen, auch nicht nur für ein weiteres
    // `set_permissions` (Pfadauflösung braucht `x` auf jedem Vorfahren).
    let count = all_paths.len() as u64;
    op.set_files(0, Some(count));
    for (entry_path, entry_mode) in all_paths.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.set_permissions(&entry_path, entry_mode).await?;
        op.add_files_done();
    }
    Ok(Some(count))
}

/// Issue #654: change owner and/or group. `owner`/`group` are a name (looked
/// up in the server's `/etc/passwd`/`/etc/group`) or a number; `None` or an
/// empty text leaves that side unchanged. Names are resolved **before**
/// anything is changed: an unknown name fails with nothing touched.
/// `recursive` applies to folders only and never follows symlinks.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // one argument per IPC parameter
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

pub(crate) const CHOWN_UNKNOWN_USER: &str = "CHOWN_UNKNOWN_USER";
pub(crate) const CHOWN_UNKNOWN_GROUP: &str = "CHOWN_UNKNOWN_GROUP";
pub(crate) const CHOWN_NOTHING_TO_CHANGE: &str = "CHOWN_NOTHING_TO_CHANGE";

pub(super) fn non_empty(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

pub(super) fn is_numeric_id(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

/// Resolves the typed owner/group to ids. Numbers need no lookup; a name is
/// looked up on the server through the **same channel** as the change (so an
/// elevated change reads the files with the elevated rights and a revoked
/// channel is noticed).
pub(super) async fn resolve_chown_ids(
    sftp: &mut dyn SftpSession,
    owner: Option<&str>,
    group: Option<&str>,
) -> CommandResult<(Option<u32>, Option<u32>)> {
    use app_logic::owner_names::{load_id_names, resolve_group, resolve_user, IdLookupError};
    let (owner, group) = (non_empty(owner), non_empty(group));
    if owner.is_none() && group.is_none() {
        return Err(CommandError::with_code(
            "Neither owner nor group was given.",
            CHOWN_NOTHING_TO_CHANGE,
        ));
    }
    let needs_lookup =
        owner.is_some_and(|o| !is_numeric_id(o)) || group.is_some_and(|g| !is_numeric_id(g));
    let names = if needs_lookup {
        load_id_names(sftp).await
    } else {
        app_logic::owner_names::IdNames::default()
    };
    let map_err = |e: IdLookupError| match e {
        IdLookupError::UnknownUser(n) => {
            CommandError::with_code(format!("Unknown user: {n}"), CHOWN_UNKNOWN_USER)
        }
        IdLookupError::UnknownGroup(n) => {
            CommandError::with_code(format!("Unknown group: {n}"), CHOWN_UNKNOWN_GROUP)
        }
    };
    let uid = owner
        .map(|o| resolve_user(&names, o))
        .transpose()
        .map_err(map_err)?;
    let gid = group
        .map(|g| resolve_group(&names, g))
        .transpose()
        .map_err(map_err)?;
    Ok((uid, gid))
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
        elevated_user.as_deref(),
        "chown",
        path,
        matches!(result, Ok(Some(_))),
    );
    result?.ok_or_else(stopped_error)
}

/// SFTP `SETSTAT` takes uid and gid as a pair: the side that stays gets the
/// entry's current id. Without a known current id nothing is guessed.
pub(super) fn chown_target(
    entry: &RemoteEntry,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(u32, u32), SshError> {
    let missing = || {
        SshError::ChannelError(format!(
            "{}: the server did not report the current owner/group",
            entry.path
        ))
    };
    Ok((
        uid.or(entry.uid).ok_or_else(missing)?,
        gid.or(entry.gid).ok_or_else(missing)?,
    ))
}

/// Core of `sftp_chown`; mirrors `chmod_recursive` (read the whole tree
/// first, then change deepest-first). Entries that are symlinks are skipped
/// below the root so a link can never lead the change outside the tree.
/// Returns the number of changed entries.
pub(super) async fn chown_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    uid: Option<u32>,
    gid: Option<u32>,
    recursive: bool,
    op: &OpHandle,
) -> Result<Option<u64>, SshError> {
    if op.is_user_stopped() {
        return Ok(None);
    }
    // `lstat`: same symlink reasoning as `delete_recursive`.
    let root_entry = sftp.lstat(path).await?;
    if !recursive || !root_entry.is_dir {
        let (u, g) = chown_target(&root_entry, uid, gid)?;
        sftp.set_owner(path, u, g).await?;
        return Ok(Some(1));
    }
    op.begin_entries();
    let mut targets = vec![(path.to_string(), chown_target(&root_entry, uid, gid)?)];
    let mut queue = vec![path.to_string()];
    while let Some(dir) = queue.pop() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        for entry in sftp.list_dir(&dir).await? {
            if entry.is_symlink {
                continue;
            }
            targets.push((entry.path.clone(), chown_target(&entry, uid, gid)?));
            if entry.is_dir {
                queue.push(entry.path);
            }
        }
    }
    let count = targets.len() as u64;
    op.set_files(0, Some(count));
    for (entry_path, (u, g)) in targets.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.set_owner(&entry_path, u, g).await?;
        op.add_files_done();
    }
    Ok(Some(count))
}
