//! Reine Orchestrierungs-Logik für den `delete_server`-Command (Spec 0046,
//! Fund 1) — als eigene, von `tauri::State` unabhängige Funktion gehalten,
//! analog zu `crate::groups::compute_delete_group_result`, damit sie sich
//! isoliert gegen einen `ProfileStore`/`CredentialStore` testen lässt.

use chrono::Utc;

use ssh_manager_core::profiles::{CredentialStore, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::dto::{DeleteServerResult, ServerDto, ServerInput};
use crate::error::{CommandError, CommandResult};
use crate::server_credentials::{
    cleanup_replaced_auth_method_secrets, delete_all_possible_server_secrets,
    delete_auth_method_secrets, delete_sudo_password_on_server_delete, resolve_auth_method,
    resolve_sudo_password, roll_back_failed_edit, RecordingCredentialStore,
};

/// Spec 0032, Abschnitt 6: der lokale Pseudo-Server ist explizit als
/// Jump-Host ausgeschlossen — vor dieser Prüfung fiel das erst implizit,
/// tief in `resolve_connection_target`, mit einer generischen "nicht
/// auflösbar"-Meldung auf (unabhängiger Review-Pass, s. docs/adr/0026).
///
/// Spec 0082, A6/A7: lag bis dahin als private Funktion im
/// `app-shell`-Command-Modul. Sie zieht hierher, weil [`update_server`]
/// sie **innerhalb** der Tauri-freien Fassung aufrufen muss — die
/// Ablehnung gehört vor jedes Lesen und Schreiben im Schlüsselbund und
/// darf deshalb nicht in einer Schicht sitzen, die kein Unit-Test
/// erreicht. `commands::create_server` ruft unverändert dieselbe Prüfung,
/// nur über diesen Pfad.
pub fn reject_local_jump_host(jump_host: Option<ServerId>) -> CommandResult<()> {
    if jump_host.is_some_and(crate::dto::is_local) {
        return Err(CommandError::with_code(
            "Der lokale Pseudo-Server kann nicht als Jump-Host verwendet werden",
            "SERVER_JUMP_HOST_LOCAL",
        ));
    }
    Ok(())
}

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
    keychain: credentials_keyring::KeychainAvailability,
    input: ServerInput,
) -> CommandResult<ServerId> {
    // Vor jedem Schlüsselbund-Zugriff prüfen — ein ungültiger Pfad soll
    // keine Secrets anlegen, die danach wieder abgeräumt werden müssten.
    let sftp_server_path = crate::dto::normalize_sftp_server_path(input.sftp_server_path.clone())?;
    let id = ServerId::new();

    let auth = match resolve_auth_method(credential_store, keychain, id, input.auth, None) {
        Ok(auth) => auth,
        Err(err) => {
            delete_all_possible_server_secrets(credential_store, id);
            return Err(err);
        }
    };
    if let Err(err) = resolve_sudo_password(credential_store, keychain, id, input.sudo_password) {
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
        sftp_server_path,
        created_at: now,
        updated_at: now,
    };

    if let Err(err) = store.create_server(&server).await {
        delete_all_possible_server_secrets(credential_store, id);
        return Err(err.into());
    }
    Ok(id)
}

/// Der volle `update_server`-Ablauf (Spec 0082, A7), losgelöst von
/// `tauri::State` — `commands::update_server` reicht nur noch durch.
///
/// Vor dieser Extraktion lief der gesamte Ablauf im `#[tauri::command]`-
/// Handler und war damit in keinem Unit-Test aufrufbar: Getestet werden
/// konnten nur seine Einzelteile (`resolve_auth_method`,
/// `resolve_sudo_password`), nie ihr Zusammenspiel — und genau dort liegt
/// die Zusage, um die es in Spec 0082 geht (was bleibt im Schlüsselbund
/// stehen, wenn ein späterer Schritt scheitert).
///
/// Reihenfolge der Prüfungen (Spec 0082, A6): die Ablehnung des lokalen
/// Pseudo-Servers und eines lokalen Jump-Hosts steht **vor** jedem Lesen
/// aus Profil- oder Credential-Store. Anders als bei [`create_server`],
/// wo beides im Tauri-Command bleibt, gehören sie hier in die Tauri-freie
/// Fassung: Nur so lässt sich die Reihenfolge testen.
pub async fn update_server(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    keychain: credentials_keyring::KeychainAvailability,
    id: ServerId,
    input: ServerInput,
) -> CommandResult<()> {
    if crate::dto::is_local(id) {
        // Spec 0032, Abschnitt 3: existiert nicht als `servers`-Zeile — nur
        // Notizen/Tags sind editierbar, über die dedizierten
        // `update_local_server_notes`/`update_local_server_tags`-Befehle.
        return Err("Der lokale Pseudo-Server kann nicht auf diesem Weg bearbeitet werden".into());
    }
    reject_local_jump_host(input.jump_host)?;
    let sftp_server_path = crate::dto::normalize_sftp_server_path(input.sftp_server_path.clone())?;

    let existing = store.get_server(&id).await?;
    let previous_auth = existing.auth.clone();

    // Jedes Schreiben läuft über die aufzeichnende Hülle — nur so weiß der
    // Rückweg, welche Einträge dieser Aufruf angelegt hat (A3). Gelöscht
    // wird bewusst am echten Store, nicht über die Hülle: Die Aufzeichnung
    // soll das Schreiben dieses Aufrufs festhalten, nicht sein Aufräumen.
    let recording = RecordingCredentialStore::new(credential_store);

    let saved_auth =
        match resolve_auth_method(&recording, keychain, id, input.auth, Some(&previous_auth)) {
            Ok(auth) => auth,
            Err(err) => {
                roll_back_failed_edit(credential_store, id, &previous_auth, &recording.written());
                return Err(err);
            }
        };
    if let Err(err) = resolve_sudo_password(&recording, keychain, id, input.sudo_password) {
        roll_back_failed_edit(credential_store, id, &previous_auth, &recording.written());
        return Err(err);
    }

    let server = Server {
        id,
        name: input.name,
        host: input.host,
        port: input.port,
        username: input.username,
        group_id: input.group_id,
        tags: input.tags,
        auth: saved_auth.clone(),
        notes: existing.notes,
        jump_host: input.jump_host,
        post_ingest_policy: input.post_ingest_policy,
        ai_injection_check_enabled: input.ai_injection_check_enabled,
        sftp_server_path,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    if let Err(err) = store.update_server(&server).await {
        roll_back_failed_edit(credential_store, id, &previous_auth, &recording.written());
        return Err(err.into());
    }

    // **Erst hier** (A2): Die Datenbank trägt jetzt die neue Anmeldeart —
    // was von der bisherigen übrig ist und die neue nicht weiterbenutzt,
    // verweist auf nichts mehr und darf weg. Jede frühere Stelle wäre
    // genau der Fehler, den diese Spec behebt.
    cleanup_replaced_auth_method_secrets(credential_store, &previous_auth, &saved_auth);
    Ok(())
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
        // Spec 0071, A17: Diese Funktion löscht per Konstruktion nichts
        // (s. Doc-Kommentar oben) — es kann also auch nichts stehen
        // geblieben sein. `delete_server` unten füllt das Feld.
        secrets_left_behind: Vec::new(),
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
    let mut result =
        compute_delete_server_result(store, credential_store, &server, confirm).await?;
    if confirm {
        // Spec 0071, A17 (zweiter Punkt): Das Löschen läuft durch, auch
        // wenn ein Secret nicht entfernt werden konnte — das Profil
        // verschwindet, und das Ergebnis sagt ausdrücklich, was im
        // Schlüsselbund zurückblieb. Der Nutzer soll nicht auf einem
        // unlöschbaren Server sitzen bleiben, nur weil der Schlüsselbund
        // klemmt; verschweigen darf man den Rückstand aber auch nicht
        // (X6-Korrektur vom 2026-09-22).
        let mut left_behind = delete_auth_method_secrets(credential_store, &server.auth);
        left_behind.extend(delete_sudo_password_on_server_delete(credential_store, id));
        store.delete_server(&id).await?;
        result.secrets_left_behind = left_behind
            .into_iter()
            .map(|r| r.as_str().to_string())
            .collect();
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

    /// Spec 0071: Der In-Memory-Store dieser Tests ist per Definition
    /// verfügbar — hier geht es um das Rollback-Verhalten, nicht um die
    /// Schlüsselbund-Verfügbarkeit.
    const AVAILABLE: credentials_keyring::KeychainAvailability =
        credentials_keyring::KeychainAvailability::Available;

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
            sftp_server_path: None,
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
        assert!(
            result.secrets_left_behind.is_empty(),
            "bei funktionierendem Schlüsselbund bleibt nichts zurück"
        );
    }

    /// Spec 0071, A17 (zweiter Punkt): Klemmt der Schlüsselbund, wird der
    /// Server **trotzdem** gelöscht — niemand soll auf einem unlöschbaren
    /// Server sitzen bleiben. Das Ergebnis sagt dafür ausdrücklich, welche
    /// Einträge im Schlüsselbund zurückblieben; sie sind danach verwaist,
    /// weil die Server-ID nicht mehr existiert.
    ///
    /// Am Stand vor A17 war `secrets_left_behind` nicht vorhanden und der
    /// Rückstand nur im Log sichtbar — das Ergebnis behauptete implizit,
    /// alles sei entfernt.
    #[tokio::test]
    async fn test_delete_server_succeeds_but_reports_secrets_it_could_not_remove() {
        let credential_ref = CredentialRef::new("test:server-password");
        let target = server_with_password("target", None, &credential_ref);
        let store = InMemoryProfileStore::new().with_server(target.clone());
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&credential_ref, "hunter2")
            .with_failing_delete();

        let result = delete_server(&store, &credentials, target.id, true)
            .await
            .expect("das Löschen darf am Schlüsselbund nicht scheitern");

        assert!(result.executed);
        assert!(
            store.get_server(&target.id).await.is_err(),
            "der Server muss trotz Schlüsselbund-Fehler verschwinden"
        );
        assert!(
            result
                .secrets_left_behind
                .contains(&credential_ref.as_str().to_string()),
            "der Rückstand muss im Ergebnis stehen, nicht nur im Log: {:?}",
            result.secrets_left_behind
        );
        assert!(
            credentials.get(&credential_ref).is_ok(),
            "der Test taugt nur, wenn das Secret tatsächlich stehen bleibt"
        );
    }

    /// Gegenprobe zu A17: Ohne Bestätigung wird nichts gelöscht — und
    /// damit kann auch nichts zurückbleiben.
    #[tokio::test]
    async fn test_delete_server_preview_never_reports_leftovers() {
        let credential_ref = CredentialRef::new("test:server-password");
        let target = server_with_password("target", None, &credential_ref);
        let store = InMemoryProfileStore::new().with_server(target.clone());
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&credential_ref, "hunter2")
            .with_failing_delete();

        let preview = delete_server(&store, &credentials, target.id, false)
            .await
            .unwrap();

        assert!(!preview.executed);
        assert!(preview.secrets_left_behind.is_empty());
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
            sftp_server_path: None,
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

        let result = create_server(
            &store,
            &credentials,
            AVAILABLE,
            password_input("secret", "hunter2"),
        )
        .await;

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

        let result = create_server(
            &store,
            &credentials,
            AVAILABLE,
            password_input("secret", "hunter2"),
        )
        .await;

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
            sftp_server_path: None,
        }
    }

    fn private_key_input(key_value: &str, passphrase_value: &str) -> ServerInput {
        ServerInput {
            name: "target".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: crate::dto::AuthMethodInput::PrivateKey {
                key_content: Some(key_value.to_string()),
                passphrase: Some(passphrase_value.to_string()),
            },
            jump_host: None,
            sudo_password: None,
            post_ingest_policy: Default::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
        }
    }

    /// spec-reviewer-Fund (Review dieses Schritts, ERHÖHT): dieselbe
    /// Zwei-Slot-Teil-Write-Situation wie bei `Certificate` (s. Test unten),
    /// nur für `PrivateKey` — der Key-Slot wird zuerst geschrieben, die
    /// Passphrase erst danach; schlägt deren Write fehl, darf der bereits
    /// geschriebene `private_key`-Slot nicht verwaist zurückbleiben.
    #[tokio::test]
    async fn test_create_server_rolls_back_private_key_secret_when_passphrase_write_fails() {
        let store = InMemoryProfileStore::new();
        let credentials = InMemoryCredentialStore::new().with_failing_set_for_slot("passphrase");

        let result = create_server(
            &store,
            &credentials,
            AVAILABLE,
            private_key_input("key-pem", "hunter2"),
        )
        .await;

        assert!(result.is_err());
        assert!(
            credentials.secrets.lock().unwrap().is_empty(),
            "das bereits geschriebene Private-Key-Secret darf nach dem fehlgeschlagenen \
             Passphrase-Write nicht übrig bleiben, war aber: {:?}",
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
            AVAILABLE,
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

    // --- Spec 0082: Bearbeiten darf keine Zugangsdaten kosten -------------
    //
    // Jeder Test hier fährt den **ganzen** Bearbeiten-Ablauf
    // ([`update_server`]) — Auflösen der Anmeldeart, Sudo-Passwort,
    // Schreiben der Datenbank. Genau darin liegt die Zusage: Die
    // Einzelteile für sich waren immer schon getestet, kaputt war ihr
    // Zusammenspiel (ein Schritt räumte auf, ein späterer scheiterte, und
    // niemand nahm das Aufgeräumte zurück).
    //
    // Ausgangszustand, wo nicht anders genannt: ein gespeicherter Server
    // mit `Password` und hinterlegtem Passwort.

    use crate::dto::{AuthMethodInput, LOCAL_SERVER_ID};
    use crate::server_credentials::{credential_ref, sudo_password_credential_ref};
    use crate::test_support::log_capture;
    use secrecy::ExposeSecret;

    const IDENTITY_PATH: &str = "/home/deploy/.ssh/id_ed25519";

    fn stored_server(id: ServerId, auth: AuthMethod) -> Server {
        let mut s = server("target", None);
        s.id = id;
        s.auth = auth;
        s
    }

    fn edit_input(auth: AuthMethodInput) -> ServerInput {
        ServerInput {
            name: "target".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth,
            jump_host: None,
            sudo_password: None,
            post_ingest_policy: Default::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
        }
    }

    fn stored_secret(store: &InMemoryCredentialStore, r: &CredentialRef) -> Option<String> {
        store.get(r).ok().map(|s| s.expose_secret().to_string())
    }

    /// Die DB-Zeile des Ausgangszustands „Server mit Passwort".
    fn password_store(id: ServerId, password_ref: &CredentialRef) -> InMemoryProfileStore {
        InMemoryProfileStore::new().with_server(stored_server(
            id,
            AuthMethod::Password {
                credential_ref: password_ref.clone(),
            },
        ))
    }

    /// Ausgangszustand „Server mit Passwort": DB-Zeile plus passender Ref,
    /// beides unter derselben ID.
    fn password_server() -> (ServerId, InMemoryProfileStore, CredentialRef) {
        let id = ServerId::new();
        let password_ref = credential_ref(id, "password");
        let store = password_store(id, &password_ref);
        (id, store, password_ref)
    }

    fn auth_of(store: &InMemoryProfileStore, id: &ServerId) -> AuthMethod {
        store
            .servers
            .lock()
            .unwrap()
            .get(id)
            .expect("die Server-Zeile muss noch da sein")
            .auth
            .clone()
    }

    /// T1 (Messung M1): Wechsel auf „Zertifikat" mit beiden Feldern leer.
    /// Der Fehler ist richtig — der Preis dafür darf nicht das bisherige
    /// Passwort sein.
    #[tokio::test]
    async fn test_t1_failed_switch_to_certificate_keeps_the_previous_password() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        let err = update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Certificate {
                cert_content: None,
                key_content: None,
            }),
        )
        .await
        .expect_err("ohne Zertifikat darf nicht gespeichert werden");

        assert_eq!(err.code, Some("SERVER_CERTIFICATE_REQUIRED"));
        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password"),
            "das Passwort der bisherigen Anmeldeart muss den gescheiterten Wechsel überleben"
        );
        assert!(matches!(auth_of(&store, &id), AuthMethod::Password { .. }));
    }

    /// T2 (M2): Der Schlüsselbund verweigert das Schreiben des neuen
    /// Private Keys.
    #[tokio::test]
    async fn test_t2_failed_private_key_write_keeps_the_previous_password() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_set_for_slot("private_key");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::PrivateKey {
                key_content: Some("-----BEGIN KEY-----".to_string()),
                passphrase: None,
            }),
        )
        .await
        .expect_err("ein fehlgeschlagener Schlüsselbund-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
    }

    /// T3 (M3): Der Wechsel selbst gelingt, erst das Sudo-Passwort
    /// scheitert.
    #[tokio::test]
    async fn test_t3_failed_sudo_password_write_keeps_the_previous_password() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_set_for_slot("sudo_password");
        let mut input = edit_input(AuthMethodInput::Agent);
        input.sudo_password = Some("sudo-secret".to_string());

        update_server(&store, &credentials, AVAILABLE, id, input)
            .await
            .expect_err("ein fehlgeschlagener Sudo-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        assert!(matches!(auth_of(&store, &id), AuthMethod::Password { .. }));
    }

    /// T4: Alles im Schlüsselbund gelingt, das Schreiben der Datenbank
    /// scheitert.
    #[tokio::test]
    async fn test_t4_failed_database_write_keeps_the_previous_password() {
        let id = ServerId::new();
        let password_ref = credential_ref(id, "password");
        let store = password_store(id, &password_ref).with_failing_update_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Agent),
        )
        .await
        .expect_err("ein fehlgeschlagener DB-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
    }

    /// T5 (M4): Zertifikat angegeben, Zertifikats-Key vergessen. Der
    /// Aufruf hat den `certificate`-Slot schon geschrieben, bevor die
    /// zweite Pflichtfeld-Prüfung zuschlägt — beides muss zurück: das
    /// Passwort bleibt, der halb geschriebene neue Slot verschwindet.
    #[tokio::test]
    async fn test_t5_failed_certificate_switch_keeps_password_and_leaves_no_orphan() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        let err = update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Certificate {
                cert_content: Some("cert-pem".to_string()),
                key_content: None,
            }),
        )
        .await
        .expect_err("ohne Zertifikats-Key darf nicht gespeichert werden");

        assert_eq!(err.code, Some("SERVER_CERTIFICATE_KEY_REQUIRED"));
        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        assert!(
            stored_secret(&credentials, &credential_ref(id, "certificate")).is_none(),
            "der bereits geschriebene Zertifikat-Slot darf nicht verwaist zurückbleiben"
        );
    }

    /// T6 (M5): Private Key → Schlüsseldatei **mit neuer Passphrase**.
    /// Beide Anmeldearten legen ihre Passphrase unter demselben Ref ab —
    /// wer die Slots der alten Art pauschal abräumt, löscht hier die
    /// gerade gespeicherte Passphrase.
    #[tokio::test]
    async fn test_t6_switch_to_identity_file_with_a_new_passphrase_keeps_the_shared_slot() {
        let id = ServerId::new();
        let key_ref = credential_ref(id, "private_key");
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryProfileStore::new().with_server(stored_server(
            id,
            AuthMethod::PrivateKey {
                credential_ref: key_ref.clone(),
                passphrase_ref: Some(passphrase_ref.clone()),
            },
        ));
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "old-key")
            .with_secret(&passphrase_ref, "old-passphrase");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase: Some("new-passphrase".to_string()),
            }),
        )
        .await
        .expect("der Wechsel ist vollständig angegeben und muss gelingen");

        assert_eq!(
            stored_secret(&credentials, &passphrase_ref).as_deref(),
            Some("new-passphrase"),
            "die Passphrase der NEUEN Anmeldeart darf das Aufräumen nicht treffen"
        );
        assert!(
            stored_secret(&credentials, &key_ref).is_none(),
            "der Private-Key-Slot gehört zur alten Art und wird aufgeräumt"
        );
        let AuthMethod::IdentityFile {
            passphrase_ref: saved,
            path,
        } = auth_of(&store, &id)
        else {
            panic!("die DB muss die neue Anmeldeart tragen");
        };
        assert_eq!(path, IDENTITY_PATH);
        assert_eq!(saved.as_ref(), Some(&passphrase_ref));
    }

    /// T7: derselbe Wechsel **ohne** neue Passphrase — dann verweist die
    /// neue Anmeldeart auf gar keinen Slot, und beide alten fallen weg.
    #[tokio::test]
    async fn test_t7_switch_to_identity_file_without_a_passphrase_clears_both_old_slots() {
        let id = ServerId::new();
        let key_ref = credential_ref(id, "private_key");
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryProfileStore::new().with_server(stored_server(
            id,
            AuthMethod::PrivateKey {
                credential_ref: key_ref.clone(),
                passphrase_ref: Some(passphrase_ref.clone()),
            },
        ));
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "old-key")
            .with_secret(&passphrase_ref, "old-passphrase");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase: None,
            }),
        )
        .await
        .expect("eine Schlüsseldatei ohne Passphrase ist vollständig angegeben");

        assert!(stored_secret(&credentials, &key_ref).is_none());
        assert!(stored_secret(&credentials, &passphrase_ref).is_none());
        let AuthMethod::IdentityFile {
            passphrase_ref: saved,
            ..
        } = auth_of(&store, &id)
        else {
            panic!("die DB muss die neue Anmeldeart tragen");
        };
        assert!(saved.is_none());
    }

    /// T8 (A5): Nachfolger von `test_update_kind_change_cleans_up_
    /// abandoned_slot` — beim **erfolgreichen** Wechsel wird der Slot der
    /// alten Art sehr wohl entfernt. Ohne diesen Test ließe sich jeder
    /// Test darüber auch dadurch grün bekommen, dass man das Aufräumen
    /// ganz abschaltet.
    #[tokio::test]
    async fn test_t8_successful_switch_to_agent_removes_the_abandoned_password_slot() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Agent),
        )
        .await
        .expect("der Wechsel auf den Agenten braucht keine Eingabe und muss gelingen");

        assert!(
            stored_secret(&credentials, &password_ref).is_none(),
            "nach erfolgreichem Wechsel darf kein verwaister Passwort-Slot zurückbleiben"
        );
        assert!(matches!(auth_of(&store, &id), AuthMethod::Agent));
    }

    /// T9 (A5): gleiche Anmeldeart, leeres Feld — „leer = unverändert"
    /// gilt weiter.
    #[tokio::test]
    async fn test_t9_editing_without_changing_the_method_keeps_the_stored_password() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Password { value: None }),
        )
        .await
        .expect("ein leeres Passwortfeld beim Bearbeiten bedeutet unverändert");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
    }

    /// T9b: derselbe Fall für die Schlüsseldatei — der Fund aus Spec 0076,
    /// §7.1, jetzt am **ganzen** Ablauf statt nur an `resolve_auth_method`.
    /// Dort prüfte ihn eine Paarliste; hier kann er per Konstruktion nicht
    /// mehr auftreten (gleiche Art ⇒ leere Differenzmenge). Der Test hält
    /// das fest, statt sich darauf zu verlassen.
    #[tokio::test]
    async fn test_t9b_editing_an_identity_file_server_keeps_its_stored_passphrase() {
        let id = ServerId::new();
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryProfileStore::new().with_server(stored_server(
            id,
            AuthMethod::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase_ref: Some(passphrase_ref.clone()),
            },
        ));
        let credentials =
            InMemoryCredentialStore::new().with_secret(&passphrase_ref, "old-passphrase");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase: None,
            }),
        )
        .await
        .expect("das bloße Bearbeiten einer Schlüsseldatei muss gelingen");

        assert_eq!(
            stored_secret(&credentials, &passphrase_ref).as_deref(),
            Some("old-passphrase")
        );
    }

    /// T10 (A4): Das Speichern gelingt, nur das Aufräumen des alten Slots
    /// scheitert. Das Ergebnis bleibt Erfolg — aber der Rückstand darf
    /// nicht spurlos verschwinden. Im Log steht der Ref, **nicht** das
    /// Passwort.
    #[tokio::test]
    async fn test_t10_a_failed_cleanup_still_succeeds_but_warns_with_the_ref_only() {
        log_capture::start_recording();
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_delete();

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Agent),
        )
        .await
        .expect("ein klemmender Schlüsselbund darf das Speichern nicht scheitern lassen");

        assert!(matches!(auth_of(&store, &id), AuthMethod::Agent));
        let log = log_capture::recorded_text();
        assert!(
            log.contains(password_ref.as_str()),
            "der nicht entfernte Eintrag muss mit seinem Ref im Log auftauchen, war: {log}"
        );
        assert!(
            !log.contains("old-password"),
            "der Secret-Inhalt darf nie im Log stehen, war: {log}"
        );
    }

    /// T11 (M5, Gegenrichtung): Schlüsseldatei → Private Key, beides neu.
    /// Auch hier trägt die neue Art den Ref, den die alte als
    /// „aufzuräumen" führt.
    #[tokio::test]
    async fn test_t11_switch_from_identity_file_to_private_key_keeps_the_new_passphrase() {
        let id = ServerId::new();
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryProfileStore::new().with_server(stored_server(
            id,
            AuthMethod::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase_ref: Some(passphrase_ref.clone()),
            },
        ));
        let credentials =
            InMemoryCredentialStore::new().with_secret(&passphrase_ref, "old-passphrase");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::PrivateKey {
                key_content: Some("-----BEGIN KEY-----".to_string()),
                passphrase: Some("new-passphrase".to_string()),
            }),
        )
        .await
        .expect("der Wechsel ist vollständig angegeben und muss gelingen");

        assert_eq!(
            stored_secret(&credentials, &passphrase_ref).as_deref(),
            Some("new-passphrase")
        );
        let AuthMethod::PrivateKey {
            passphrase_ref: saved,
            ..
        } = auth_of(&store, &id)
        else {
            panic!("die DB muss die neue Anmeldeart tragen");
        };
        assert_eq!(saved.as_ref(), Some(&passphrase_ref));
    }

    /// T12 (A5): Nachfolger von `test_switching_away_from_an_identity_
    /// file_cleans_up_the_passphrase_slot` — die Gegenprobe zu T9b.
    #[tokio::test]
    async fn test_t12_switching_away_from_an_identity_file_removes_the_passphrase_slot() {
        let id = ServerId::new();
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryProfileStore::new().with_server(stored_server(
            id,
            AuthMethod::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase_ref: Some(passphrase_ref.clone()),
            },
        ));
        let credentials =
            InMemoryCredentialStore::new().with_secret(&passphrase_ref, "old-passphrase");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Agent),
        )
        .await
        .expect("der Wechsel auf den Agenten muss gelingen");

        assert!(
            stored_secret(&credentials, &passphrase_ref).is_none(),
            "verwaister Passphrase-Slot muss nach erfolgreichem Wechsel weg sein"
        );
    }

    /// T13 (A3, Sudo): Der Rückweg darf **nicht** wie beim Anlegen alle
    /// Slots des Servers abräumen. Das Sudo-Passwort ist ein eigener Slot,
    /// unabhängig von der Anmeldeart — es gehört weder der alten noch der
    /// neuen Art und wird deshalb nie angefasst (R1: der neue Wert bleibt
    /// stehen).
    #[tokio::test]
    async fn test_t13_rollback_never_touches_the_sudo_password_slot() {
        let id = ServerId::new();
        let password_ref = credential_ref(id, "password");
        let sudo_ref = sudo_password_credential_ref(id);
        let store = password_store(id, &password_ref).with_failing_update_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_secret(&sudo_ref, "old-sudo");
        let mut input = edit_input(AuthMethodInput::Agent);
        input.sudo_password = Some("new-sudo".to_string());

        update_server(&store, &credentials, AVAILABLE, id, input)
            .await
            .expect_err("ein fehlgeschlagener DB-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password"),
            "die bisherige Anmeldeart bleibt verbindbar"
        );
        assert_eq!(
            stored_secret(&credentials, &sudo_ref).as_deref(),
            Some("new-sudo"),
            "das Sudo-Passwort gehört keiner Anmeldeart und darf vom Rückweg nicht \
             abgeräumt werden (R1)"
        );
    }

    /// T14 (A4 auf dem Rückweg): wie T5, zusätzlich klemmt jedes
    /// Entfernen. Der gemeldete Fehler bleibt der **ursprüngliche** —
    /// ein per `?` durchgereichter Löschfehler würde dem Nutzer die
    /// falsche Ursache nennen.
    #[tokio::test]
    async fn test_t14_a_failed_rollback_keeps_the_original_error_and_warns() {
        log_capture::start_recording();
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_delete();

        let err = update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Certificate {
                cert_content: Some("cert-pem".to_string()),
                key_content: None,
            }),
        )
        .await
        .expect_err("ohne Zertifikats-Key darf nicht gespeichert werden");

        assert_eq!(
            err.code,
            Some("SERVER_CERTIFICATE_KEY_REQUIRED"),
            "der Löschfehler des Rückwegs darf den gemeldeten Fehler nicht verdrängen"
        );
        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        let log = log_capture::recorded_text();
        let certificate_ref = credential_ref(id, "certificate");
        assert!(
            log.contains(certificate_ref.as_str()),
            "der nicht entfernte Eintrag muss mit seinem Ref im Log auftauchen, war: {log}"
        );
        assert!(
            !log.contains("cert-pem"),
            "der Secret-Inhalt darf nie im Log stehen, war: {log}"
        );
    }

    /// T15 (A6): Ein lokaler Jump-Host wird abgelehnt, **bevor** irgendein
    /// Schlüsselbund-Zugriff passiert. Der Store lehnt jedes Löschen ab —
    /// so kann ein zu früh geschriebener Eintrag nicht vom Rückweg
    /// verwischt werden und fällt auf.
    #[tokio::test]
    async fn test_t15_a_local_jump_host_is_rejected_before_any_keychain_access() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_delete();
        let mut input = edit_input(AuthMethodInput::PrivateKey {
            key_content: Some("-----BEGIN KEY-----".to_string()),
            passphrase: None,
        });
        input.jump_host = Some(LOCAL_SERVER_ID);
        input.sudo_password = Some("new-sudo".to_string());

        let err = update_server(&store, &credentials, AVAILABLE, id, input)
            .await
            .expect_err("der lokale Pseudo-Server ist als Jump-Host ausgeschlossen");

        // **Vor** allen weiteren Zusicherungen abgegriffen: `stored_secret`
        // liest selbst über den Store und würde den Zähler sonst hochtreiben,
        // bis die Aussage nichts mehr über den Produktivcode sagt.
        let reads_during_the_call = credentials.get_calls();

        assert_eq!(err.code, Some("SERVER_JUMP_HOST_LOCAL"));
        assert_eq!(
            reads_during_the_call, 0,
            "vor der Ablehnung darf nicht einmal gelesen werden"
        );
        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        assert!(stored_secret(&credentials, &credential_ref(id, "private_key")).is_none());
        assert!(stored_secret(&credentials, &sudo_password_credential_ref(id)).is_none());
    }

    /// T15b: dasselbe für den lokalen Pseudo-Server selbst — er hat keine
    /// `servers`-Zeile, die Ablehnung muss also vor dem Profil-Lesen
    /// greifen.
    #[tokio::test]
    async fn test_t15b_the_local_pseudo_server_is_rejected_before_any_keychain_access() {
        let id = LOCAL_SERVER_ID;
        let password_ref = credential_ref(id, "password");
        let store = InMemoryProfileStore::new();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_delete();
        let mut input = edit_input(AuthMethodInput::PrivateKey {
            key_content: Some("-----BEGIN KEY-----".to_string()),
            passphrase: None,
        });
        input.sudo_password = Some("new-sudo".to_string());

        update_server(&store, &credentials, AVAILABLE, id, input)
            .await
            .expect_err("der lokale Pseudo-Server wird nicht auf diesem Weg bearbeitet");

        // S. T15: der Zähler wird vor den lesenden Zusicherungen abgegriffen.
        let reads_during_the_call = credentials.get_calls();

        assert_eq!(
            reads_during_the_call, 0,
            "vor der Ablehnung darf nicht einmal gelesen werden"
        );
        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        assert!(stored_secret(&credentials, &credential_ref(id, "private_key")).is_none());
        assert!(stored_secret(&credentials, &sudo_password_credential_ref(id)).is_none());
    }

    /// T16 (R1 festhalten): gleiche Anmeldeart mit **neuem** Wert, danach
    /// scheitert die Datenbank. Der neue Wert bleibt stehen — er gehört
    /// demselben Ref wie der alte, ein Rückweg, der ihn als „in diesem
    /// Aufruf geschrieben" entfernt, nähme dem Server seine Anmeldung.
    #[tokio::test]
    async fn test_t16_an_overwritten_shared_slot_is_never_removed_by_the_rollback() {
        let id = ServerId::new();
        let password_ref = credential_ref(id, "password");
        let store = password_store(id, &password_ref).with_failing_update_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::Password {
                value: Some("new-password".to_string()),
            }),
        )
        .await
        .expect_err("ein fehlgeschlagener DB-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("new-password"),
            "R1: der überschriebene Wert bleibt stehen — gelöscht wird er nie"
        );
    }

    /// T17 (A3): Der neue Slot steht schon im Schlüsselbund, als die
    /// Datenbank scheitert. Er gehört zu keiner gespeicherten Anmeldeart
    /// und muss weg.
    #[tokio::test]
    async fn test_t17_a_database_failure_removes_the_newly_written_private_key() {
        let id = ServerId::new();
        let password_ref = credential_ref(id, "password");
        let store = password_store(id, &password_ref).with_failing_update_server();
        let credentials = InMemoryCredentialStore::new().with_secret(&password_ref, "old-password");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::PrivateKey {
                key_content: Some("-----BEGIN KEY-----".to_string()),
                passphrase: None,
            }),
        )
        .await
        .expect_err("ein fehlgeschlagener DB-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        assert!(
            stored_secret(&credentials, &credential_ref(id, "private_key")).is_none(),
            "der Rückweg muss auch außerhalb der Credential-Auflösung greifen"
        );
    }

    /// T17b: derselbe Rückweg, nur scheitert diesmal das Sudo-Passwort —
    /// ein Schritt, der gar nicht in `resolve_auth_method` liegt.
    #[tokio::test]
    async fn test_t17b_a_failed_sudo_write_removes_the_newly_written_private_key() {
        let (id, store, password_ref) = password_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "old-password")
            .with_failing_set_for_slot("sudo_password");
        let mut input = edit_input(AuthMethodInput::PrivateKey {
            key_content: Some("-----BEGIN KEY-----".to_string()),
            passphrase: None,
        });
        input.sudo_password = Some("new-sudo".to_string());

        update_server(&store, &credentials, AVAILABLE, id, input)
            .await
            .expect_err("ein fehlgeschlagener Sudo-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &password_ref).as_deref(),
            Some("old-password")
        );
        assert!(stored_secret(&credentials, &credential_ref(id, "private_key")).is_none());
    }

    /// T18 (M5 auf dem Fehlerweg): Der Rückweg darf „in diesem Aufruf
    /// geschrieben" nicht ohne Abgleich mit den Refs der bisherigen Art
    /// lesen — sonst löscht er hier die Passphrase, auf die die **alte**,
    /// weiterhin gespeicherte Anmeldeart verweist.
    #[tokio::test]
    async fn test_t18_a_rollback_never_removes_a_slot_the_previous_method_still_uses() {
        let id = ServerId::new();
        let key_ref = credential_ref(id, "private_key");
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryProfileStore::new()
            .with_server(stored_server(
                id,
                AuthMethod::PrivateKey {
                    credential_ref: key_ref.clone(),
                    passphrase_ref: Some(passphrase_ref.clone()),
                },
            ))
            .with_failing_update_server();
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "old-key")
            .with_secret(&passphrase_ref, "old-passphrase");

        update_server(
            &store,
            &credentials,
            AVAILABLE,
            id,
            edit_input(AuthMethodInput::IdentityFile {
                path: IDENTITY_PATH.to_string(),
                passphrase: Some("new-passphrase".to_string()),
            }),
        )
        .await
        .expect_err("ein fehlgeschlagener DB-Write darf nicht als Erfolg gelten");

        assert_eq!(
            stored_secret(&credentials, &key_ref).as_deref(),
            Some("old-key"),
            "der Schlüssel der bisherigen Anmeldeart bleibt"
        );
        assert_eq!(
            stored_secret(&credentials, &passphrase_ref).as_deref(),
            Some("new-passphrase"),
            "der gemeinsame Slot bleibt stehen (R1) — gelöscht wäre die alte Anmeldung \
             nicht mehr zu entsperren"
        );
        assert!(
            matches!(auth_of(&store, &id), AuthMethod::PrivateKey { .. }),
            "die Datenbank ist unverändert"
        );
    }
}
