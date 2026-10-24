//! Spec 0008: Gruppen-Verwaltung — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use chrono::Utc;
use tauri::State;

use ssh_manager_core::profiles::{Group, GroupId};

use app_logic::dto::{DeleteGroupResult, GroupDto, KeepGroupResult};
use app_logic::error::CommandResult;
use app_logic::groups::{move_group as move_group_impl, validate_no_cycle};
use app_logic::state::AppState;

// --- Spec 0008: Gruppen --------------------------------------------------

#[tauri::command]
pub async fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<GroupDto>> {
    let groups = state.profile_store.list_groups().await?;
    let marked = copy_marked(&state).await?;
    Ok(groups
        .iter()
        .map(|g| {
            let mut dto = GroupDto::from(g);
            dto.is_copy = marked.groups.contains(&g.id.0);
            dto
        })
        .collect())
}

/// The ids carrying the copy marker (Spec 0103).
pub(super) async fn copy_marked(state: &AppState) -> CommandResult<persistence_sqlite::CopyMarked> {
    Ok(state.backup_store.copy_marked().await?)
}

/// Spec 0103: "Keep as regular entry" for a group. Without `confirm` only
/// reports how many groups and servers would be cleared; with it clears
/// the group, its sub-groups and every server inside.
#[tauri::command]
pub async fn keep_group_as_regular_entry(
    state: State<'_, AppState>,
    id: GroupId,
    confirm: bool,
) -> CommandResult<KeepGroupResult> {
    let scope = if confirm {
        state.backup_store.clear_group_copy_marker(id.0).await
    } else {
        state.backup_store.group_copy_marker_scope(id.0).await
    }?;
    Ok(KeepGroupResult {
        applied: confirm,
        groups: scope.groups,
        servers: scope.servers,
    })
}

#[tauri::command]
pub async fn create_group(
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<GroupId>,
) -> CommandResult<GroupId> {
    validate_no_cycle(state.profile_store.as_ref(), None, parent_id).await?;

    let now = Utc::now();
    let group = Group {
        id: GroupId::new(),
        name,
        parent_id,
        notes: String::new(),
        created_at: now,
        updated_at: now,
    };
    state.profile_store.create_group(&group).await?;
    Ok(group.id)
}

#[tauri::command]
pub async fn update_group(
    state: State<'_, AppState>,
    id: GroupId,
    name: String,
    parent_id: Option<GroupId>,
) -> CommandResult<()> {
    validate_no_cycle(state.profile_store.as_ref(), Some(id), parent_id).await?;

    let mut group = state.profile_store.get_group(&id).await?;
    group.name = name;
    group.parent_id = parent_id;
    group.updated_at = Utc::now();
    state.profile_store.update_group(&group).await?;
    Ok(())
}

/// Issue #48: Drag-and-drop in der Server-Liste — schmaler Befehl, der nur
/// `parent_id` ändert (Name/Notizen bleiben), Zyklusprüfung wie
/// `update_group`. Logik in `app_logic::groups::move_group`.
#[tauri::command]
pub async fn move_group(
    state: State<'_, AppState>,
    id: GroupId,
    parent_id: Option<GroupId>,
) -> CommandResult<()> {
    move_group_impl(state.profile_store.as_ref(), id, parent_id).await
}

/// Spec 0008, Abschnitt 3: `confirm_cascade: false` liefert nur die
/// Vorschau (nichts wird gelöscht), `confirm_cascade: true` löscht
/// tatsächlich — ein zweiter, expliziter Aufruf, kein Query-Parameter, der
/// versehentlich beim ersten Aufruf schon `true` sein könnte.
#[tauri::command]
pub async fn delete_group(
    state: State<'_, AppState>,
    id: GroupId,
    confirm_cascade: bool,
) -> CommandResult<DeleteGroupResult> {
    app_logic::groups::delete_group(
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        id,
        confirm_cascade,
        state.session_extension.as_deref(),
    )
    .await
}
