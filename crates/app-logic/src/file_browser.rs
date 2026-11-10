//! Pure helpers for remote directory listings (Issue #868: moved out of `dto`).

use serde::Serialize;

use ssh_manager_core::ssh::RemoteEntry;

/// Sicht auf einen [`RemoteEntry`] für die Dateiliste im Dateibrowser (Spec
/// 0020, Abschnitt 5.1: "Name, Größe, Rechte, Änderungsdatum"). `permissions`
/// kommt bereits hier als lesbarer `rwxr-xr-x`-String statt als rohe Bits —
/// das Frontend braucht dafür keine eigene Unix-Rechte-Formatierungslogik.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RemoteEntryDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: String,
    pub modified: Option<String>,
    /// Spec 0054, Teil 2 (Eigenschaften-Dialog: "Rechte numerisch +
    /// symbolisch") — dieselben Bits wie `permissions`, nur numerisch
    /// (`0o644`-Stil) statt als `rwxr-xr-x`-String. Auch die Grundlage für
    /// den chmod-Dialog (Teil 3), der von einem numerischen Ausgangswert
    /// aus editiert statt den `permissions`-String zurückparsen zu müssen.
    pub permissions_octal: u32,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
    /// Issue #1291: the entry itself is a symbolic link (lstat view).
    pub is_symlink: bool,
    /// Issue #1291: `readlink` target of a link, `None` for other entries or
    /// when it could not be read. Server-controlled text: render as text only.
    pub link_target: Option<String>,
    /// Issue #1291: what a link points to (`None` for non-links). For
    /// `Dir` links `is_dir` is `true` so the UI can navigate; recursive
    /// operations never use this DTO to decide whether to descend.
    pub link_kind: Option<LinkKind>,
}

/// Issue #1291: resolved target type of a symbolic link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum LinkKind {
    Dir,
    File,
    /// Target missing or not accessible.
    Broken,
}

impl From<&RemoteEntry> for RemoteEntryDto {
    fn from(entry: &RemoteEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            size: entry.size,
            permissions: format_unix_permissions(entry.permissions),
            modified: entry.modified.map(|dt| dt.to_rfc3339()),
            permissions_octal: entry.permissions,
            uid: entry.uid,
            gid: entry.gid,
            owner: entry.owner.clone(),
            group: entry.group.clone(),
            is_symlink: entry.is_symlink,
            link_target: None,
            link_kind: None,
        }
    }
}

/// Formatiert reine Unix-Rechte-Bits (keine Typ-Bits, s.
/// `RemoteEntry::permissions`-Doc-Kommentar in `core`) als klassischen
/// `rwxr-xr-x`-String.
pub(crate) fn format_unix_permissions(bits: u32) -> String {
    let triplet = |shift: u32| -> String {
        let r = if bits & (0o4 << shift) != 0 { 'r' } else { '-' };
        let w = if bits & (0o2 << shift) != 0 { 'w' } else { '-' };
        let x = if bits & (0o1 << shift) != 0 { 'x' } else { '-' };
        [r, w, x].iter().collect()
    };
    format!("{}{}{}", triplet(6), triplet(3), triplet(0))
}

/// Issue #1291: resolves every symbolic link in `entries` with at most one
/// `stat` and one `read_link` per link entry (none for ordinary entries). A
/// failing call never fails the listing: a failing `stat` marks the link
/// broken, a failing `read_link` leaves `link_target` empty. The core
/// `RemoteEntry`s stay lstat-like; only the returned DTOs carry `is_dir ==
/// true` for links to folders.
pub async fn resolve_symlinks(
    sftp: &mut dyn ssh_manager_core::ssh::SftpSession,
    entries: &[RemoteEntry],
) -> Vec<RemoteEntryDto> {
    let mut dtos = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut dto = RemoteEntryDto::from(entry);
        if entry.is_symlink {
            dto.link_target = sftp.read_link(&entry.path).await.ok();
            match sftp.stat(&entry.path).await {
                Ok(target) if target.is_dir => {
                    dto.link_kind = Some(LinkKind::Dir);
                    dto.is_dir = true;
                }
                Ok(_) => dto.link_kind = Some(LinkKind::File),
                Err(_) => dto.link_kind = Some(LinkKind::Broken),
            }
        }
        dtos.push(dto);
    }
    dtos
}

/// Sortiert Verzeichniseinträge für die Anzeige: Verzeichnisse zuerst, dann
/// alphabetisch nach Name (case-insensitiv) — Spec 0020 macht dazu keine
/// Vorgabe, das ist die in Dateibrowsern übliche Konvention.
pub fn sort_remote_entries(entries: &mut [RemoteEntryDto]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}
