//! Reine Orchestrierungs-Logik für den `delete_server`-Command (Spec 0046,
//! Fund 1) — als eigene, von `tauri::State` unabhängige Funktion gehalten,
//! analog zu `crate::groups::compute_delete_group_result`, damit sie sich
//! isoliert gegen einen `ProfileStore`/`CredentialStore` testen lässt.

use ssh_manager_core::profiles::{CredentialStore, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::dto::{DeleteServerResult, ServerDto};
use crate::error::CommandResult;
use crate::server_credentials::{clear_sudo_password, delete_auth_method_secrets};

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

/// Der volle `delete_server`-Ablauf (Spec 0046, Fund 1), losgelöst von
/// `tauri::State` — `commands::delete_server` ist nur noch ein dünner
/// Wrapper darum (den lokalen-Pseudo-Server-Ausschluss ausgenommen, der
/// vor jedem Store-Zugriff greift). spec-reviewer-Fund (Review dieses
/// Schritts): ohne diese Extraktion lief die eigentliche
/// "ohne Bestätigung wird nichts gelöscht"-Garantie nur im `#[tauri::
/// command]`-Handler selbst, der `State<'_, AppState>` braucht und daher
/// in keinem Unit-Test erreichbar war — getestet wurde bislang nur
/// `compute_delete_server_result`, das per Konstruktion nie löscht. Jetzt
/// läuft genau dieselbe Funktion in Produktion UND im Test.
pub async fn delete_server(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    id: ServerId,
    confirm: bool,
) -> CommandResult<DeleteServerResult> {
    let server = store.get_server(&id).await?;
    let result = compute_delete_server_result(store, credential_store, &server, confirm).await?;
    if confirm {
        delete_auth_method_secrets(credential_store, &server.auth);
        clear_sudo_password(credential_store, id);
        store.delete_server(&id).await?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use ssh_manager_core::profiles::{AuthMethod, CredentialRef, PostIngestPolicy};
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

    fn server_with_password(
        name: &str,
        jump_host: Option<ServerId>,
        credential_ref: &CredentialRef,
    ) -> Server {
        Server {
            auth: AuthMethod::Password {
                credential_ref: credential_ref.clone(),
            },
            ..server(name, jump_host)
        }
    }

    /// spec-reviewer-Fund (Review dieses Schritts): ruft die tatsächliche
    /// `delete_server`-Funktion auf (denselben Code, den `commands::
    /// delete_server` in Produktion aufruft), nicht nur `compute_
    /// delete_server_result` (das per Konstruktion nie löscht) — belegt
    /// damit die eigentliche, in der Spec geforderte Garantie: "ohne
    /// Bestätigung wird nichts gelöscht" gilt für den echten Lösch-Pfad
    /// inklusive Keychain, nicht nur für die Vorschau-Berechnung.
    #[tokio::test]
    async fn test_delete_server_confirm_false_deletes_neither_row_nor_secret() {
        let credential_ref = CredentialRef::new("test:server-password");
        let target = server_with_password("target", None, &credential_ref);
        let store = InMemoryProfileStore::new().with_server(target.clone());
        let credentials = InMemoryCredentialStore::new().with_secret(&credential_ref, "hunter2");

        let preview = delete_server(&store, &credentials, target.id, false)
            .await
            .unwrap();

        assert!(!preview.executed);
        assert!(
            store.get_server(&target.id).await.is_ok(),
            "confirm: false darf die DB-Zeile nicht löschen"
        );
        assert!(
            credentials.get(&credential_ref).is_ok(),
            "confirm: false darf das Keychain-Secret nicht löschen"
        );
    }

    /// Gegenstück: `confirm: true` löscht tatsächlich — Keychain-Secret,
    /// DB-Zeile, und ein abhängiger Server verliert nur die
    /// Jump-Host-Referenz (`ON DELETE SET NULL`), wird nicht mitgelöscht.
    #[tokio::test]
    async fn test_delete_server_confirm_true_deletes_row_secret_and_nulls_dependent_jump_host() {
        let credential_ref = CredentialRef::new("test:server-password");
        let target = server_with_password("target", None, &credential_ref);
        let dependent = server("dependent", Some(target.id));
        let store = InMemoryProfileStore::new()
            .with_server(target.clone())
            .with_server(dependent.clone());
        let credentials = InMemoryCredentialStore::new().with_secret(&credential_ref, "hunter2");

        let result = delete_server(&store, &credentials, target.id, true)
            .await
            .unwrap();

        assert!(result.executed);
        assert!(
            store.get_server(&target.id).await.is_err(),
            "confirm: true muss die DB-Zeile löschen"
        );
        assert!(
            credentials.get(&credential_ref).is_err(),
            "confirm: true muss das Keychain-Secret löschen"
        );
        let orphaned = store.get_server(&dependent.id).await.unwrap();
        assert_eq!(
            orphaned.jump_host, None,
            "abhängiger Server verliert nur die Jump-Host-Referenz, wird nicht mitgelöscht"
        );
    }
}
