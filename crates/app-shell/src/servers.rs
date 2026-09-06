//! Reine Orchestrierungs-Logik für den `delete_server`-Command (Spec 0046,
//! Fund 1) — als eigene, von `tauri::State` unabhängige Funktion gehalten,
//! analog zu `crate::groups::compute_delete_group_result`, damit sie sich
//! isoliert gegen einen `ProfileStore`/`CredentialStore` testen lässt.

use ssh_manager_core::profiles::{CredentialStore, ProfileStore, Server};

use crate::dto::{DeleteServerResult, ServerDto};
use crate::error::CommandResult;

/// Baut die Vorschau/das Ergebnis von `delete_server` (Spec 0046, Fund 1):
/// der zu löschende Server selbst (dessen `authKind`/`hasSudoPassword` dem
/// Frontend sagen, welche Keychain-Secrets entfernt würden) sowie alle
/// anderen Server, die `id` als `jump_host` referenzieren und diese
/// Referenz beim tatsächlichen Löschen still auf `NULL` verlieren würden
/// (`ON DELETE SET NULL`, Spec 0008). `executed` spiegelt nur, ob der
/// Aufrufer tatsächlich gelöscht hat — diese Funktion selbst löscht
/// nichts (analog zu `compute_delete_group_result`).
pub async fn compute_delete_server_result(
    store: &dyn ProfileStore,
    // `+ Send + Sync` explizit ergänzt — s. identischer Kommentar bei
    // `compute_delete_group_result`.
    credential_store: &(dyn CredentialStore + Send + Sync),
    server: &Server,
    executed: bool,
) -> CommandResult<DeleteServerResult> {
    let all_servers = store.list_servers().await?;
    let servers_losing_jump_host: Vec<ServerDto> = all_servers
        .iter()
        .filter(|s| s.jump_host == Some(server.id))
        .map(|s| ServerDto::from_server(s, credential_store))
        .collect();

    Ok(DeleteServerResult {
        server: ServerDto::from_server(server, credential_store),
        servers_losing_jump_host,
        executed,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy};
    use ssh_manager_core::shared::ServerId;

    use super::*;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};

    fn server(name: &str, jump_host: Option<ServerId>) -> Server {
        let now = Utc::now();
        Server {
            id: ServerId::new(),
            name: name.to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
            notes: String::new(),
            jump_host,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn test_compute_delete_server_result_preview_does_not_delete() {
        let target = server("target", None);
        let dependent = server("dependent", Some(target.id));
        let store = InMemoryProfileStore::new()
            .with_server(target.clone())
            .with_server(dependent.clone());

        let result =
            compute_delete_server_result(&store, &InMemoryCredentialStore::new(), &target, false)
                .await
                .unwrap();

        assert!(!result.executed);
        assert_eq!(result.server.id, target.id.0.to_string());
        assert_eq!(result.servers_losing_jump_host.len(), 1);
        assert_eq!(
            result.servers_losing_jump_host[0].id,
            dependent.id.0.to_string()
        );

        // Vorschau darf nichts verändern.
        assert!(store.get_server(&target.id).await.is_ok());
        let unchanged = store.get_server(&dependent.id).await.unwrap();
        assert_eq!(unchanged.jump_host, Some(target.id));
    }

    #[tokio::test]
    async fn test_compute_delete_server_result_lists_no_dependents_when_none_reference_it() {
        let target = server("target", None);
        let unrelated = server("unrelated", None);
        let store = InMemoryProfileStore::new()
            .with_server(target.clone())
            .with_server(unrelated.clone());

        let result =
            compute_delete_server_result(&store, &InMemoryCredentialStore::new(), &target, false)
                .await
                .unwrap();

        assert!(result.servers_losing_jump_host.is_empty());
    }

    /// Simuliert den vollen Zweischritt aus `commands::delete_server`:
    /// Vorschau (nichts ändert sich), dann — erst bei `confirm` — das
    /// tatsächliche Löschen über `ProfileStore::delete_server` samt
    /// `ON DELETE SET NULL` auf abhängige Jump-Host-Referenzen.
    #[tokio::test]
    async fn test_delete_server_two_step_preview_then_execute() {
        let target = server("target", None);
        let dependent = server("dependent", Some(target.id));
        let store = InMemoryProfileStore::new()
            .with_server(target.clone())
            .with_server(dependent.clone());

        let preview =
            compute_delete_server_result(&store, &InMemoryCredentialStore::new(), &target, false)
                .await
                .unwrap();
        assert!(!preview.executed);
        assert!(
            store.get_server(&target.id).await.is_ok(),
            "Vorschau löscht nicht"
        );

        let confirmed =
            compute_delete_server_result(&store, &InMemoryCredentialStore::new(), &target, true)
                .await
                .unwrap();
        assert!(confirmed.executed);
        store.delete_server(&target.id).await.unwrap();

        assert!(store.get_server(&target.id).await.is_err());
        let orphaned = store.get_server(&dependent.id).await.unwrap();
        assert_eq!(
            orphaned.jump_host, None,
            "abhängiger Server verliert nur die Jump-Host-Referenz, wird nicht mitgelöscht"
        );
    }
}
