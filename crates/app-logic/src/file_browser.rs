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
