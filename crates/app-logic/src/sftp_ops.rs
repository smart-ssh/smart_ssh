//! Issue #849: the pure SFTP logic of the file browser — recursive walk,
//! delete, chmod/chown, mode-bit calculation, owner-name resolution and
//! overwrite-rename. Everything here works on a `&mut dyn SftpSession` and
//! a small progress/stop trait ([`OpProgress`]); the elevated channel and
//! its types stay in `app-shell` and are never visible here (the compiler
//! and a source-scan test in `app-shell` enforce that).

use async_trait::async_trait;
use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

use crate::error::{CommandError, CommandResult};

pub const CHOWN_UNKNOWN_USER: &str = "CHOWN_UNKNOWN_USER";
pub const CHOWN_UNKNOWN_GROUP: &str = "CHOWN_UNKNOWN_GROUP";
pub const CHOWN_NOTHING_TO_CHANGE: &str = "CHOWN_NOTHING_TO_CHANGE";

/// Progress and stop handling of one running file operation.
pub trait OpProgress: Send + Sync {
    /// Stopped by the user (button or confirmed dialog). Checked before each
    /// entry; what was already changed stays changed.
    fn is_user_stopped(&self) -> bool;
    /// The recursion found a folder and now counts entries.
    fn begin_entries(&self);
    fn set_files(&self, done: u64, total: Option<u64>);
    fn add_files_done(&self);
}

/// Lists one remote directory. Separate from [`SftpSession`] so a caller can
/// take (and release) its channel lock per listing.
#[async_trait]
pub trait DirLister: Send {
    async fn list_dir(&mut self, path: &str) -> CommandResult<Vec<RemoteEntry>>;
}

/// Collects all directory paths under `root` (including `root`) and counts
/// the files. Directories are in discovery order (a directory appears after
/// its parent), so reversing gives a bottom-up order.
pub async fn walk_dirs_and_count_files(
    sftp: &mut dyn SftpSession,
    root: &str,
) -> Result<(Vec<String>, u64), SshError> {
    let mut dirs = vec![root.to_string()];
    let mut queue = vec![root.to_string()];
    let mut file_count = 0u64;
    while let Some(dir) = queue.pop() {
        let entries = sftp.list_dir(&dir).await?;
        for entry in entries {
            if entry.is_dir {
                queue.push(entry.path.clone());
                dirs.push(entry.path);
            } else {
                file_count += 1;
            }
        }
    }
    Ok((dirs, file_count))
}

/// Receives every entry of [`walk_tree`] right after its directory was
/// listed, before the next directory is listed. An error aborts the walk.
#[async_trait]
pub trait TreeVisitor: Send {
    async fn visit(&mut self, dir: &str, entry: RemoteEntry) -> CommandResult<()>;
}

/// Walks the tree below `root`, handing each entry together with the path of
/// the directory it was listed in to `visitor` as soon as that directory is
/// listed. A parent directory's own entry always comes before the entries
/// inside it. `Ok(None)`: stopped by the user before the walk finished.
pub async fn walk_tree(
    lister: &mut dyn DirLister,
    root: &str,
    op: &dyn OpProgress,
    visitor: &mut dyn TreeVisitor,
) -> CommandResult<Option<()>> {
    let mut queue = vec![root.to_string()];
    while let Some(dir) = queue.pop() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        for entry in lister.list_dir(&dir).await? {
            if entry.is_dir {
                queue.push(entry.path.clone());
            }
            visitor.visit(&dir, entry).await?;
        }
    }
    Ok(Some(()))
}

/// Recursive delete. `lstat` instead of `stat`: `stat` follows symlinks, so a
/// symlink to a directory (`/var/www/current -> /etc`) would be taken for a
/// folder and the tree behind it deleted. `lstat` reports the link itself,
/// which is then only removed, never entered.
///
/// `Ok(None)` = stopped by the user; checked before each entry, what was
/// already deleted stays deleted.
pub async fn delete_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    op: &dyn OpProgress,
) -> Result<Option<()>, SshError> {
    if op.is_user_stopped() {
        return Ok(None);
    }
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.remove(path).await?;
        return Ok(Some(()));
    }

    op.begin_entries();
    let (dirs, file_count) = walk_dirs_and_count_files(sftp, path).await?;
    op.set_files(0, Some(dirs.len() as u64 + file_count));
    // Walk the tree a second time for the files instead of keeping all
    // their paths from the first walk (the preview only needs the counts).
    for dir in &dirs {
        if op.is_user_stopped() {
            return Ok(None);
        }
        let entries = sftp.list_dir(dir).await?;
        for entry in entries {
            if !entry.is_dir {
                if op.is_user_stopped() {
                    return Ok(None);
                }
                sftp.remove(&entry.path).await?;
                op.add_files_done();
            }
        }
    }
    for dir in dirs.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.remove_dir(&dir).await?;
        op.add_files_done();
    }
    Ok(Some(()))
}

/// Issue #624: the mode one entry gets during a recursive apply (like
/// `chmod -R` with capital `X`). Files get exactly the chosen rwx bits and
/// never a special bit. Folders get the chosen rwx bits plus execute for
/// every class that has read, so they stay enterable; their special bits
/// stay as they are unless the user changed them (`apply_special_bits`),
/// in which case the chosen ones apply to folders only.
pub fn recursive_entry_mode(entry: &RemoteEntry, mode: u32, apply_special_bits: bool) -> u32 {
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

/// Core of `sftp_chmod`. Returns the number of changed entries; `Ok(None)`
/// = stopped by the user.
pub async fn chmod_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
    op: &dyn OpProgress,
) -> Result<Option<u64>, SshError> {
    if !recursive {
        sftp.set_permissions(path, mode).await?;
        return Ok(Some(1));
    }
    // `lstat`: same symlink reasoning as in `delete_recursive`.
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.set_permissions(path, mode).await?;
        return Ok(Some(1));
    }

    // Read the whole tree first (with the original permissions) and change
    // afterwards: a folder that already lost the owner's execute bit could
    // not be listed any more when its turn comes.
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
    // Deepest first, root last: once the root loses its execute bit no path
    // below it can be resolved any more.
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

pub fn non_empty(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

pub fn is_numeric_id(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

/// Resolves the typed owner/group to ids. Numbers need no lookup; a name is
/// looked up on the server through the **same channel** as the change (so an
/// elevated change reads the files with the elevated rights and a revoked
/// channel is noticed).
pub async fn resolve_chown_ids(
    sftp: &mut dyn SftpSession,
    owner: Option<&str>,
    group: Option<&str>,
) -> CommandResult<(Option<u32>, Option<u32>)> {
    use crate::owner_names::{load_id_names, resolve_group, resolve_user, IdLookupError};
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
        crate::owner_names::IdNames::default()
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

/// SFTP `SETSTAT` takes uid and gid as a pair: the side that stays gets the
/// entry's current id. Without a known current id nothing is guessed.
pub fn chown_target(
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
pub async fn chown_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    uid: Option<u32>,
    gid: Option<u32>,
    recursive: bool,
    op: &dyn OpProgress,
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

/// Issue #625: rename/move onto an existing **file name** after the user
/// confirmed "overwrite". SFTP `RENAME` never replaces.
///
/// - Target is a folder: error, nothing is touched.
/// - Server with `posix-rename@openssh.com`: one atomic exchange.
/// - Otherwise: remove the target, then `rename`. If the `rename` fails, the
///   error says the target is already removed and the source is unchanged.
///
/// A failing `stat` of the target counts as "target missing": nothing
/// replaces anything, and the plain `rename` decides (it never overwrites,
/// and a revoked elevation fails there just the same).
pub async fn rename_replacing(
    sftp: &mut dyn SftpSession,
    from: &str,
    to: &str,
) -> Result<(), SshError> {
    if from == to {
        return Ok(());
    }
    match sftp.stat(to).await {
        Ok(entry) if entry.is_dir => {
            return Err(SshError::ChannelError(format!(
                "Das Ziel '{to}' ist ein Ordner und kann nicht ersetzt werden."
            )));
        }
        Ok(_) => {}
        Err(_) => return sftp.rename(from, to).await,
    }
    if sftp.posix_rename(from, to).await? {
        return Ok(());
    }
    sftp.remove(to).await?;
    sftp.rename(from, to).await.map_err(|e| {
        SshError::ChannelError(format!(
            "Das Ziel '{to}' wurde bereits entfernt, aber das Umbenennen von '{from}' ist \
             fehlgeschlagen; die Quelle ist unverändert: {e}"
        ))
    })
}

#[cfg(test)]
mod tests;
