//! Reine Orchestrierungs-Logik für den `delete_server`-Command (Spec 0046,
//! Fund 1) — als eigene, von `tauri::State` unabhängige Funktion gehalten,
//! analog zu `crate::groups::compute_delete_group_result`, damit sie sich
//! isoliert gegen einen `ProfileStore`/`CredentialStore` testen lässt.

use chrono::Utc;

use ssh_manager_core::profiles::{CredentialStore, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::dto::{DeleteServerResult, ServerDto, ServerInput};
use crate::error::CommandResult;
use crate::server_credentials::{
    clear_sudo_password, delete_all_possible_server_secrets, delete_auth_method_secrets,
    resolve_auth_method, resolve_sudo_password,
};

/// Der volle `create_server`-Ablauf (Spec 0047, Fund A2), losgelöst von
/// `tauri::State` — analog zu [`delete_server`] unten (das schon dem
/// Spec-0046-Muster folgt, das diese Spec explizit als Vorbild nennt).
/// `commands::create_server` ist nur noch ein dünner Wrapper (den
/// lokalen-Jump-Host-Ausschluss ausgenommen, der vor jedem Store-Zugriff
/// greift und nichts in den Keychain schreibt).
///
/// **Rollback-Garantie**: schlägt irgendein Schritt fehl — `resolve_auth_
/// method` selbst (kann bei `PrivateKey`/`Certificate` bereits den ersten
/// von zwei Slots geschrieben haben, bevor der zweite scheitert, s.
/// dortiger Kommentar), `resolve_sudo_password`, oder der abschließende
/// `ProfileStore::create_server`-DB-Insert — werden ALLE Keychain-Slots
/// abgeräumt, die dieser Aufruf potenziell beschrieben haben könnte
/// (`delete_all_possible_server_secrets`), bevor der Fehler zurückgeht.
/// Kein verwaister Eintrag für eine `ServerId`, die es nicht (mehr) gibt.
pub async fn create_server(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    input: ServerInput,
) -> CommandResult<ServerId> {
    let id = ServerId::new();

    let auth = match resolve_auth_method(credential_store, id, input.auth, None) {
        Ok(auth) => auth,
        Err(err) => {
            delete_all_possible_server_secrets(credential_store, id);
            return Err(err);
        }
    };
    if let Err(err) = resolve_sudo_password(credential_store, id, input.sudo_password) {
        delete_all_possible_server_secrets(credential_store, id);
        return Err(err);
    }

    let now = Utc::now();
    let server = Server {
        id,
        name: input.name,
        host: input.host,
        port: input.port,
        username: input.username,
        group_id: input.group_id,
        tags: input.tags,
        auth,
        notes: String::new(),
        jump_host: input.jump_host,
        post_ingest_policy: input.post_ingest_policy,
        ai_injection_check_enabled: input.ai_injection_check_enabled,
        created_at: now,
        updated_at: now,
    };

    if let Err(err) = store.create_server(&server).await {
        delete_all_possible_server_secrets(credential_store, id);
        return Err(err.into());
    }
    Ok(id)
}

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

    fn password_input(password_value: &str, sudo_password: &str) -> ServerInput {
        ServerInput {
            name: "target".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: crate::dto::AuthMethodInput::Password {
                value: Some(password_value.to_string()),
            },
            jump_host: None,
            sudo_password: Some(sudo_password.to_string()),
            post_ingest_policy: Default::default(),
            ai_injection_check_enabled: false,
        }
    }

    /// Spec 0047, Fund A2: DB-Insert schlägt fehl, NACHDEM sowohl die
    /// Auth-Methode (Passwort) als auch das Sudo-Passwort bereits im
    /// Keychain standen — beide müssen zurückgerollt werden, nicht nur die
    /// Auth-Methode (der ursprüngliche `commands::create_server`-Code rief
    /// nur `delete_auth_method_secrets` im Fehlerfall auf, nie
    /// `clear_sudo_password` — das Sudo-Passwort blieb verwaist).
    #[tokio::test]
    async fn test_create_server_rolls_back_all_secrets_on_db_insert_failure() {
        let store = InMemoryProfileStore::new().with_failing_create_server();
        let credentials = InMemoryCredentialStore::new();

        let result = create_server(&store, &credentials, password_input("secret", "hunter2")).await;

        assert!(result.is_err());
        assert!(
            credentials.secrets.lock().unwrap().is_empty(),
            "nach einem fehlgeschlagenen create_server darf kein Keychain-Eintrag \
             übrig bleiben (weder Passwort noch Sudo-Passwort), war aber: {:?}",
            credentials
                .secrets
                .lock()
                .unwrap()
                .keys()
                .collect::<Vec<_>>()
        );
    }

    /// Gegenprobe zum selben Fund: das Sudo-Passwort-`set()` schlägt fehl,
    /// NACHDEM die Auth-Methode (Passwort) bereits erfolgreich geschrieben
    /// wurde — dieser Pfad erreicht den DB-Insert gar nicht erst
    /// (`resolve_sudo_password` gibt vorher `Err` zurück), trotzdem darf
    /// das bereits geschriebene Passwort-Secret nicht verwaist zurück-
    /// bleiben. Deckt den zweiten, subtileren Leck-Pfad ab, den die reine
    /// "räum bei DB-Fehler die Auth-Methode auf"-Fassung nicht sah.
    #[tokio::test]
    async fn test_create_server_rolls_back_auth_secret_when_sudo_password_write_fails() {
        let store = InMemoryProfileStore::new();
        let credentials = InMemoryCredentialStore::new().with_failing_set_for_slot("sudo_password");

        let result = create_server(&store, &credentials, password_input("secret", "hunter2")).await;

        assert!(result.is_err());
        assert!(
            credentials.secrets.lock().unwrap().is_empty(),
            "das bereits geschriebene Passwort-Secret darf nach dem fehlgeschlagenen \
             Sudo-Passwort-Write nicht übrig bleiben, war aber: {:?}",
            credentials
                .secrets
                .lock()
                .unwrap()
                .keys()
                .collect::<Vec<_>>()
        );
        assert!(
            store.servers.lock().unwrap().is_empty(),
            "bei einem Fehler vor dem DB-Insert darf keine Server-Zeile entstehen"
        );
    }

    fn certificate_input(cert_value: &str, key_value: &str) -> ServerInput {
        ServerInput {
            name: "target".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: crate::dto::AuthMethodInput::Certificate {
                cert_content: Some(cert_value.to_string()),
                key_content: Some(key_value.to_string()),
            },
            jump_host: None,
            sudo_password: None,
            post_ingest_policy: Default::default(),
            ai_injection_check_enabled: false,
        }
    }

    /// spec-reviewer-Fund (Review dieses Schritts, ERHÖHT): deckt den in
    /// `server_credentials.rs`s eigenem Doc-Kommentar genannten, aber bis
    /// dahin ungetesteten zweiten Teil-Write-Pfad ab — `Certificate`
    /// schreibt zuerst den `certificate`-Slot, dann erst den
    /// `certificate_key`-Slot (s. `resolve_auth_method`). Schlägt der
    /// zweite Write fehl, muss der bereits geschriebene `certificate`-Slot
    /// zurückgerollt werden, nicht nur der (hier gar nicht erst
    /// geschriebene) `certificate_key`-Slot.
    #[tokio::test]
    async fn test_create_server_rolls_back_certificate_secret_when_certificate_key_write_fails() {
        let store = InMemoryProfileStore::new();
        let credentials =
            InMemoryCredentialStore::new().with_failing_set_for_slot("certificate_key");

        let result = create_server(
            &store,
            &credentials,
            certificate_input("cert-pem", "key-pem"),
        )
        .await;

        assert!(result.is_err());
        assert!(
            credentials.secrets.lock().unwrap().is_empty(),
            "das bereits geschriebene Zertifikat-Secret darf nach dem fehlgeschlagenen \
             Zertifikats-Key-Write nicht übrig bleiben, war aber: {:?}",
            credentials
                .secrets
                .lock()
                .unwrap()
                .keys()
                .collect::<Vec<_>>()
        );
        assert!(
            store.servers.lock().unwrap().is_empty(),
            "bei einem Fehler vor dem DB-Insert darf keine Server-Zeile entstehen"
        );
    }
}
