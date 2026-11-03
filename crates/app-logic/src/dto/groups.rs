//! DTOs: groups.

use serde::Serialize;

use ssh_manager_core::profiles::Group;

use super::{ServerDto, UnusableServerDto};

/// Flache Sicht auf eine [`Group`] (Spec 0008, Abschnitt 3 — "Baum wird im
/// Frontend gebaut"). `notes` ist dort nicht ausdrücklich erwähnt, aber
/// auch nicht ausgeschlossen — ohne dieses Feld bräuchte das
/// Gruppen-Formular einen eigenen `get_group`-Befehl nur für die
/// Notiz-Vorbefüllung, obwohl `list_groups()` die Daten ohnehin schon aus
/// derselben `Group`-Struktur liest. Kein zusätzlicher DB-Zugriff, nur ein
/// zusätzliches Feld auf einem bereits geladenen Wert.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub notes: String,
    /// Spec 0103: see [`ServerDto::is_copy`].
    pub is_copy: bool,
}

/// Result of "Keep as regular entry" on a group (Spec 0103): without
/// `confirm` only the counts of what would be cleared, with it the counts
/// of what was cleared.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepGroupResult {
    pub applied: bool,
    pub groups: usize,
    pub servers: usize,
}

impl From<&Group> for GroupDto {
    fn from(group: &Group) -> Self {
        Self {
            id: group.id.0.to_string(),
            name: group.name.clone(),
            parent_id: group.parent_id.map(|p| p.0.to_string()),
            notes: group.notes.clone(),
            is_copy: false,
        }
    }
}

/// Vorschau bzw. Ergebnis von `delete_group` (Spec 0008, Abschnitt 3).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteGroupResult {
    pub child_groups_to_delete: Vec<GroupDto>,
    pub servers_to_unassign: Vec<ServerDto>,
    /// Issue #177 (Spec 0008 §6a): nicht nutzbare Server in der Gruppe oder
    /// einer Nachfahre-Gruppe — sie verlieren die Gruppenzuordnung ebenso.
    pub unusable_servers_to_unassign: Vec<UnusableServerDto>,
    pub executed: bool,
}
