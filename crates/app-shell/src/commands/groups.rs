//! Spec 0008: Gruppen-Verwaltung — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use chrono::Utc;
use tauri::State;

use ssh_manager_core::profiles::{Group, GroupId};

use app_logic::dto::{DeleteGroupResult, GroupDto};
use app_logic::error::CommandResult;
use app_logic::groups::{compute_delete_group_result, validate_no_cycle};
use app_logic::state::AppState;

// --- Spec 0008: Gruppen --------------------------------------------------

#[tauri::command]
pub async fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<GroupDto>> {
    let groups = state.profile_store.list_groups().await?;
    Ok(groups.iter().map(GroupDto::from).collect())
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
    let result = compute_delete_group_result(
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        id,
        confirm_cascade,
    )
    .await?;
    if confirm_cascade {
        state.profile_store.delete_group(&id).await?;
    }
    Ok(result)
}
