//! Issue #128 / Spec 0054, part 3: planning of a recursive folder upload.
//!
//! [`plan_folder`] walks a granted local folder and returns what would be
//! uploaded. It reads only directory listings and file metadata, never file
//! contents. Rules (ADR 0126):
//!
//! - Symbolic links are never followed: they are listed as skipped.
//! - Every file and subfolder must pass the grant rule (canonicalised, inside
//!   a grant of the session) — the same rule as a single upload. An entry
//!   that fails it is skipped and listed.
//! - Special files (sockets, pipes, devices) and names that are not valid
//!   UTF-8 are skipped and listed.
//! - Depth and entry count are bounded, so a very deep or huge tree cannot
//!   hold the walk indefinitely.

use std::path::PathBuf;

use serde::Serialize;

use crate::error::CommandError;
use crate::local_path_grants::{GrantSnapshot, GrantedLocalPath};

/// Deepest folder level that is walked (the root is level 0).
pub const MAX_DEPTH: usize = 64;
/// Most entries (files + folders) one folder upload may contain.
pub const MAX_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    Symlink,
    TooDeep,
    Unsupported,
    NotGranted,
    /// The remote file exists and the user did not confirm overwriting it.
    OverwriteNotConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
    /// Path relative to the uploaded folder, `/`-separated.
    pub path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedEntry {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Relative to the uploaded folder, `/`-separated.
    pub relative: String,
    pub local: GrantedLocalPath,
}

#[derive(Debug, Clone)]
pub struct FolderUploadPlan {
    /// Name of the folder, the first segment below the remote target.
    pub folder_name: String,
    /// Subfolders, relative, parents before children.
    pub dirs: Vec<String>,
    pub files: Vec<PlannedFile>,
    pub skipped: Vec<SkippedEntry>,
}

/// Result of an executed folder upload, shown to the user as the summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderUploadSummary {
    pub folder_name: String,
    pub files_uploaded: u32,
    pub folders_created: u32,
    pub skipped: Vec<SkippedEntry>,
    pub failed: Vec<FailedEntry>,
    /// Files below a folder that could not be created; not attempted.
    pub not_attempted: u32,
}

/// What the user is asked before a folder upload starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderUploadPreview {
    pub folder_name: String,
    pub file_count: u32,
    pub folder_count: u32,
    /// Remote paths of existing files that the upload would overwrite.
    pub overwrites: Vec<String>,
    pub skipped: Vec<SkippedEntry>,
}

pub fn join_relative(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// `base` + `/` + `rel`, without doubling the slash for `/`.
pub fn join_remote(base: &str, rel: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

pub fn plan_folder(
    root: &GrantedLocalPath,
    grants: &GrantSnapshot,
) -> Result<FolderUploadPlan, CommandError> {
    let root_path = root.as_path();
    let folder_name = root_path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| CommandError::from("Der lokale Ordner hat keinen verwendbaren Namen."))?
        .to_string();
    let meta = std::fs::symlink_metadata(root_path)?;
    if !meta.is_dir() {
        return Err(CommandError::from(
            "Der gewählte lokale Pfad ist kein Ordner.",
        ));
    }

    let mut plan = FolderUploadPlan {
        folder_name,
        dirs: Vec::new(),
        files: Vec::new(),
        skipped: Vec::new(),
    };
    let mut entries_seen = 0usize;
    // Depth-first, pre-order: a folder is listed before its content.
    let mut stack: Vec<(PathBuf, String, usize)> =
        vec![(root_path.to_path_buf(), String::new(), 0)];
    while let Some((dir, rel_dir, depth)) = stack.pop() {
        let mut children: Vec<(String, PathBuf, std::fs::FileType)> = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let rel_name = entry.file_name();
            let file_type = entry.file_type()?;
            match rel_name.to_str() {
                Some(name) => children.push((name.to_string(), entry.path(), file_type)),
                None => plan.skipped.push(SkippedEntry {
                    path: join_relative(&rel_dir, &rel_name.to_string_lossy()),
                    reason: SkipReason::Unsupported,
                }),
            }
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        // Pushed in reverse so the sorted order is the pop order.
        let mut subdirs = Vec::new();
        for (name, path, file_type) in children {
            let rel = join_relative(&rel_dir, &name);
            entries_seen += 1;
            if entries_seen > MAX_ENTRIES {
                return Err(CommandError::from(format!(
                    "Der Ordner enthält mehr als {MAX_ENTRIES} Einträge und wird nicht hochgeladen."
                )));
            }
            if file_type.is_symlink() {
                plan.skipped.push(SkippedEntry {
                    path: rel,
                    reason: SkipReason::Symlink,
                });
            } else if file_type.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    plan.skipped.push(SkippedEntry {
                        path: rel,
                        reason: SkipReason::TooDeep,
                    });
                } else if grants.check(&path).is_err() {
                    plan.skipped.push(SkippedEntry {
                        path: rel,
                        reason: SkipReason::NotGranted,
                    });
                } else {
                    plan.dirs.push(rel.clone());
                    subdirs.push((path, rel, depth + 1));
                }
            } else if file_type.is_file() {
                match grants.check(&path) {
                    Ok(local) => plan.files.push(PlannedFile {
                        relative: rel,
                        local,
                    }),
                    Err(_) => plan.skipped.push(SkippedEntry {
                        path: rel,
                        reason: SkipReason::NotGranted,
                    }),
                }
            } else {
                plan.skipped.push(SkippedEntry {
                    path: rel,
                    reason: SkipReason::Unsupported,
                });
            }
        }
        stack.extend(subdirs.into_iter().rev());
    }
    // `dirs` was filled when a folder was found, i.e. before its content is
    // listed, and a folder is found only while its parent is listed: parents
    // always precede children.
    Ok(plan)
}

#[cfg(test)]
mod tests;
