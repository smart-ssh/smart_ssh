//! Reine Orchestrierungs-Logik für die Gruppen-Commands (Spec 0008,
//! Abschnitt 3) — als eigene, von `tauri::State` unabhängige Funktionen
//! gehalten, analog zu `crate::test_connection`, damit sie sich isoliert
//! gegen einen `ProfileStore` testen lassen.

use std::collections::HashSet;

use ssh_manager_core::profiles::{CredentialStore, GroupId, ProfileStore};

use crate::dto::{DeleteGroupResult, GroupDto, ServerDto, UnusableServerDto};
use crate::error::{CommandError, CommandResult};

/// Aufgabenstellung Teil 1, Punkt 5: eine Gruppe darf nicht sich selbst
/// oder einen ihrer eigenen Nachfahren als `parent_id` bekommen.
/// `group_id` ist `None` bei `create_group` (eine gerade erst entstehende
/// Gruppe kann strukturell noch kein Vorfahre von irgendetwas sein, der
/// Aufruf bleibt trotzdem für einen einzigen, gemeinsamen Prüfpfad da).
///
/// Nutzt [`ProfileStore::group_chain`] (Eltern-Kette von `new_parent` bis
/// zur Wurzel, `new_parent` selbst eingeschlossen) statt selbst eine
/// Nachfahren-Traversierung zu bauen: taucht `group_id` in dieser Kette
/// auf, ist `new_parent` entweder `group_id` selbst oder einer ihrer
/// Nachfahren — genau der zu verhindernde Fall.
pub async fn validate_no_cycle(
    store: &dyn ProfileStore,
    group_id: Option<GroupId>,
    new_parent: Option<GroupId>,
) -> CommandResult<()> {
    let (Some(group_id), Some(new_parent)) = (group_id, new_parent) else {
        return Ok(());
    };
    if group_id == new_parent {
        return Err(CommandError::with_code(
            "Eine Gruppe kann nicht ihre eigene übergeordnete Gruppe sein",
            "GROUP_SELF_PARENT",
        ));
    }
    let chain = store.group_chain(&new_parent).await?;
    if chain.iter().any(|g| g.id == group_id) {
        return Err(CommandError::with_code(
            "Zyklus: die gewählte übergeordnete Gruppe ist eine Untergruppe dieser Gruppe",
            "GROUP_CYCLE_DETECTED",
        ));
    }
    Ok(())
}

/// Issue #48: Verschieben einer Gruppe unter eine andere Gruppe (oder mit
/// `parent_id: None` auf die Wurzelebene) per Drag-and-drop. Ändert nur
/// `parent_id` — Name und Notizen bleiben unangetastet, anders als bei
/// `update_group`, das den Namen mitschreibt. Zyklen lehnt derselbe
/// [`validate_no_cycle`] ab wie beim Bearbeiten im Formular, mit
/// denselben Codes (`GROUP_SELF_PARENT`/`GROUP_CYCLE_DETECTED`); bei
/// einer Ablehnung wird nichts geschrieben.
pub async fn move_group(
    store: &dyn ProfileStore,
    id: GroupId,
    parent_id: Option<GroupId>,
) -> CommandResult<()> {
    validate_no_cycle(store, Some(id), parent_id).await?;
    let mut group = store.get_group(&id).await?;
    if group.parent_id == parent_id {
        return Ok(());
    }
    group.parent_id = parent_id;
    group.updated_at = chrono::Utc::now();
    store.update_group(&group).await?;
    Ok(())
}

/// Baut die Vorschau/das Ergebnis von `delete_group` (Spec 0008, Abschnitt
/// 3): alle Nachfahre-Gruppen (rekursiv, da `ON DELETE CASCADE` in SQLite
/// transitiv über mehrere Ebenen greift) sowie alle Server, die direkt in
/// `id` oder einer ihrer Nachfahre-Gruppen stehen (diese verlieren die
/// Gruppenzuordnung, `ON DELETE SET NULL`). `executed` spiegelt nur, ob
/// der Aufrufer tatsächlich gelöscht hat — diese Funktion selbst löscht
/// nichts.
pub async fn compute_delete_group_result(
    store: &dyn ProfileStore,
    // `+ Send + Sync` explizit ergänzt (der Trait selbst deklariert es
    // nicht, s. `crate::state::AppState`-Doc-Kommentar zu `credential_store`)
    // — ohne diesen Zusatz hält die zurückgegebene Future eine nicht-`Sync`-
    // Referenz über die vorangehenden `.await`-Punkte hinweg und ist selbst
    // nicht mehr `Send`, was `#[tauri::command]` verlangt.
    credential_store: &(dyn CredentialStore + Send + Sync),
    id: GroupId,
    executed: bool,
) -> CommandResult<DeleteGroupResult> {
    let all_groups = store.list_groups().await?;
    let listing = store.list_server_entries().await?;
    let all_servers = listing.servers;

    let mut affected: HashSet<GroupId> = HashSet::new();
    affected.insert(id);
    let mut child_groups = Vec::new();
    let mut frontier = vec![id];
    while let Some(current) = frontier.pop() {
        for group in &all_groups {
            if group.parent_id == Some(current) && affected.insert(group.id) {
                child_groups.push(group);
                frontier.push(group.id);
            }
        }
    }

    let servers_to_unassign: Vec<ServerDto> = all_servers
        .iter()
        .filter(|s| s.group_id.is_some_and(|gid| affected.contains(&gid)))
        .map(|s| ServerDto::from_server(s, credential_store))
        .collect();

    let unusable_servers_to_unassign: Vec<UnusableServerDto> = listing
        .unusable
        .iter()
        .filter(|s| s.group_id.is_some_and(|gid| affected.contains(&gid)))
        .map(UnusableServerDto::from)
        .collect();

    Ok(DeleteGroupResult {
        child_groups_to_delete: child_groups.into_iter().map(GroupDto::from).collect(),
        servers_to_unassign,
        unusable_servers_to_unassign,
        executed,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use ssh_manager_core::profiles::{AuthMethod, Group, PostIngestPolicy, Server};
    use ssh_manager_core::shared::ServerId;

    use super::*;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};

    fn group(name: &str, parent: Option<GroupId>) -> Group {
        let now = Utc::now();
        Group {
            id: GroupId::new(),
            name: name.to_string(),
            parent_id: parent,
            notes: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn server_in(group_id: Option<GroupId>) -> Server {
        let now = Utc::now();
        Server {
            id: ServerId::new(),
            name: "srv".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            start_directory: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn test_validate_no_cycle_allows_unrelated_parent() {
        let root = group("root", None);
        let other = group("other", None);
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(other.clone());

        let result = validate_no_cycle(&store, Some(root.id), Some(other.id)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_no_cycle_rejects_self_as_parent() {
        let g = group("g", None);
        let store = InMemoryProfileStore::new().with_group(g.clone());

        let result = validate_no_cycle(&store, Some(g.id), Some(g.id)).await;

        let err = result.expect_err("erwartet: Fehler bei Selbst-Referenz");
        // Spec 0024, Abschnitt 5: stabiler Code fürs Frontend-Mapping.
        assert_eq!(err.code, Some("GROUP_SELF_PARENT"));
    }

    #[tokio::test]
    async fn test_validate_no_cycle_rejects_own_descendant_as_parent() {
        let root = group("root", None);
        let child = group("child", Some(root.id));
        let grandchild = group("grandchild", Some(child.id));
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(child.clone())
            .with_group(grandchild.clone());

        // root -> grandchild als Parent wäre ein Zyklus (grandchild ist
        // Nachfahre von root).
        let result = validate_no_cycle(&store, Some(root.id), Some(grandchild.id)).await;

        let err = result.expect_err("erwartet: Fehler bei zyklischer Elternkette");
        assert_eq!(err.code, Some("GROUP_CYCLE_DETECTED"));
    }

    #[tokio::test]
    async fn test_validate_no_cycle_allows_create_with_any_parent() {
        let root = group("root", None);
        let store = InMemoryProfileStore::new().with_group(root.clone());

        // `group_id: None` (create_group) — kein Zyklus möglich.
        let result = validate_no_cycle(&store, None, Some(root.id)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_compute_delete_group_result_preview_does_not_delete() {
        let root = group("root", None);
        let child = group("child", Some(root.id));
        let server = server_in(Some(child.id));
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(child.clone())
            .with_server(server.clone());

        let result =
            compute_delete_group_result(&store, &InMemoryCredentialStore::new(), root.id, false)
                .await
                .unwrap();

        assert!(!result.executed);
        assert_eq!(result.child_groups_to_delete.len(), 1);
        assert_eq!(result.child_groups_to_delete[0].id, child.id.0.to_string());
        assert_eq!(result.servers_to_unassign.len(), 1);
        assert_eq!(result.servers_to_unassign[0].id, server.id.0.to_string());

        // Vorschau darf nichts verändern.
        assert!(store.get_group(&root.id).await.is_ok());
        assert!(store.get_group(&child.id).await.is_ok());
    }

    #[tokio::test]
    async fn test_compute_delete_group_result_includes_multi_level_descendants() {
        let root = group("root", None);
        let child = group("child", Some(root.id));
        let grandchild = group("grandchild", Some(child.id));
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(child.clone())
            .with_group(grandchild.clone());

        let result =
            compute_delete_group_result(&store, &InMemoryCredentialStore::new(), root.id, false)
                .await
                .unwrap();

        let ids: Vec<String> = result
            .child_groups_to_delete
            .iter()
            .map(|g| g.id.clone())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&child.id.0.to_string()));
        assert!(ids.contains(&grandchild.id.0.to_string()));
    }

    fn unusable_in(
        name: &str,
        group_id: Option<GroupId>,
    ) -> ssh_manager_core::profiles::UnusableServer {
        ssh_manager_core::profiles::UnusableServer {
            id: ServerId::new(),
            name: name.to_string(),
            host: "newer.example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id,
            reason: ssh_manager_core::profiles::UnusableReason::UnknownAuthMethod,
        }
    }

    /// Issue #177: nicht nutzbare Server direkt in der Gruppe und in einer
    /// Nachfahre-Gruppe stehen in der Vorschau; einer außerhalb nicht.
    #[tokio::test]
    async fn test_delete_group_preview_names_unusable_servers_in_subtree_only() {
        let root = group("root", None);
        let child = group("child", Some(root.id));
        let other = group("other", None);
        let direct = unusable_in("direct", Some(root.id));
        let nested = unusable_in("nested", Some(child.id));
        let outside = unusable_in("outside", Some(other.id));
        let ungrouped = unusable_in("ungrouped", None);
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(child)
            .with_group(other)
            .with_unusable_server(direct.clone())
            .with_unusable_server(nested.clone())
            .with_unusable_server(outside)
            .with_unusable_server(ungrouped);

        let result =
            compute_delete_group_result(&store, &InMemoryCredentialStore::new(), root.id, false)
                .await
                .unwrap();

        let mut ids: Vec<ServerId> = result
            .unusable_servers_to_unassign
            .iter()
            .map(|s| s.id)
            .collect();
        ids.sort_by_key(|id| id.0);
        let mut expected = vec![direct.id, nested.id];
        expected.sort_by_key(|id| id.0);
        assert_eq!(ids, expected);
        assert!(result.servers_to_unassign.is_empty());
        // Vorschau ändert nichts.
        assert_eq!(store.unusable.lock().unwrap().len(), 4);
    }

    /// Simuliert den vollen Zweischritt aus `commands::delete_group`:
    /// Vorschau (nichts ändert sich), dann — erst bei `confirm_cascade` —
    /// das tatsächliche Löschen über `ProfileStore::delete_group`.
    #[tokio::test]
    async fn test_delete_group_two_step_preview_then_execute() {
        let root = group("root", None);
        let child = group("child", Some(root.id));
        let server = server_in(Some(child.id));
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(child.clone())
            .with_server(server.clone());

        let preview =
            compute_delete_group_result(&store, &InMemoryCredentialStore::new(), root.id, false)
                .await
                .unwrap();
        assert!(!preview.executed);
        assert!(
            store.get_group(&child.id).await.is_ok(),
            "Vorschau löscht nicht"
        );

        let confirmed =
            compute_delete_group_result(&store, &InMemoryCredentialStore::new(), root.id, true)
                .await
                .unwrap();
        assert!(confirmed.executed);
        store.delete_group(&root.id).await.unwrap();

        assert!(store.get_group(&root.id).await.is_err());
        assert!(
            store.get_group(&child.id).await.is_err(),
            "Kind-Gruppe wird kaskadiert"
        );
        let unassigned = store.get_server(&server.id).await.unwrap();
        assert_eq!(
            unassigned.group_id, None,
            "Server verliert nur die Zuordnung"
        );
    }

    // --- Issue #48: Verschieben per Drag-and-drop -------------------------

    #[tokio::test]
    async fn test_move_group_into_other_group_and_back_to_root() {
        let a = group("a", None);
        let b = group("b", None);
        let store = InMemoryProfileStore::new()
            .with_group(a.clone())
            .with_group(b.clone());

        move_group(&store, b.id, Some(a.id)).await.unwrap();
        let moved = store.get_group(&b.id).await.unwrap();
        assert_eq!(moved.parent_id, Some(a.id));
        assert_eq!(moved.name, "b", "Name bleibt unverändert");

        move_group(&store, b.id, None).await.unwrap();
        assert_eq!(store.get_group(&b.id).await.unwrap().parent_id, None);
    }

    #[tokio::test]
    async fn test_move_group_into_own_descendant_is_rejected_and_changes_nothing() {
        let root = group("root", None);
        let child = group("child", Some(root.id));
        let grandchild = group("grandchild", Some(child.id));
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(child.clone())
            .with_group(grandchild.clone());

        let err = move_group(&store, root.id, Some(grandchild.id))
            .await
            .expect_err("Zyklus muss abgelehnt werden");

        assert_eq!(err.code, Some("GROUP_CYCLE_DETECTED"));
        assert_eq!(store.get_group(&root.id).await.unwrap().parent_id, None);
        assert_eq!(
            store.get_group(&grandchild.id).await.unwrap().parent_id,
            Some(child.id)
        );
    }

    #[tokio::test]
    async fn test_move_group_onto_itself_is_rejected() {
        let g = group("g", None);
        let store = InMemoryProfileStore::new().with_group(g.clone());

        let err = move_group(&store, g.id, Some(g.id))
            .await
            .expect_err("Selbst-Referenz muss abgelehnt werden");

        assert_eq!(err.code, Some("GROUP_SELF_PARENT"));
        assert_eq!(store.get_group(&g.id).await.unwrap().parent_id, None);
    }

    #[tokio::test]
    async fn test_move_group_keeps_its_servers() {
        let a = group("a", None);
        let b = group("b", None);
        let srv = server_in(Some(b.id));
        let store = InMemoryProfileStore::new()
            .with_group(a.clone())
            .with_group(b.clone())
            .with_server(srv.clone());

        move_group(&store, b.id, Some(a.id)).await.unwrap();

        assert_eq!(
            store.get_server(&srv.id).await.unwrap().group_id,
            Some(b.id)
        );
    }
}
