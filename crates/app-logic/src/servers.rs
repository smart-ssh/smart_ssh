//! Reine Orchestrierungs-Logik für die Server-Befehle — Anlegen (Spec
//! 0047, Fund A2), Bearbeiten (Spec 0082) und Löschen (Spec 0046, Fund 1),
//! jeweils als eigene, von `tauri::State` unabhängige Funktion gehalten,
//! analog zu `crate::groups::compute_delete_group_result`, damit sie sich
//! isoliert gegen einen `ProfileStore`/`CredentialStore` testen lassen.
//!
//! Alle drei haben dieselbe heikle Stelle: Zwischen Schlüsselbund und
//! Datenbank liegt ein Moment, in dem eines von beiden schon geändert ist
//! und das andere noch nicht. Was dort bei einem Fehler passieren muss,
//! ist je Befehl verschieden und steht bei der jeweiligen Funktion:
//!
//! - [`create_server`] räumt **alle** Slots der frischen `ServerId` ab —
//!   unter ihr kann nichts Legitimes stehen.
//! - [`update_server`] darf das gerade **nicht**: Der Server existiert
//!   weiter, mit seiner bisherigen Anmeldeart. Die Reihenfolge und ihre
//!   Begründung stehen im Moduldoc von [`crate::server_credentials`].
//! - [`delete_server`] läuft durch und meldet, was im Schlüsselbund
//!   zurückblieb (Spec 0071, A17).

use chrono::Utc;

use ssh_manager_core::profiles::{AuthMethod, CredentialStore, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::dto::{
    DeleteServerResult, DeleteUnusableServerResult, ServerDto, ServerInput, UnusableServerDto,
};
use crate::error::{CommandError, CommandResult, StoreResultExt};
use crate::server_credentials::{
    cleanup_replaced_auth_method_secrets, delete_all_possible_server_secrets,
    delete_all_server_secrets_on_unusable_delete, delete_auth_method_secrets,
    delete_sudo_password_on_server_delete, resolve_auth_method, resolve_sudo_password,
    roll_back_failed_edit, RecordingCredentialStore,
};
pub use crate::server_guards::reject_local_jump_host;
use crate::server_paths::{checked_sftp_server_path, checked_start_directory};

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
    // Vor jedem Schlüsselbund-Zugriff prüfen — ein ungültiger Pfad soll
    // keine Secrets anlegen, die danach wieder abgeräumt werden müssten.
    let sftp_server_path = checked_sftp_server_path(input.sftp_server_path.clone())?;
    let start_directory = checked_start_directory(input.start_directory.clone())?;
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
        sftp_server_path,
        start_directory,
        elevation_allowed: input.elevation_allowed,
        created_at: now,
        updated_at: now,
    };

    if let Err(err) = store.create_server(&server).await {
        delete_all_possible_server_secrets(credential_store, id);
        return Err(crate::error::CommandError::from(err));
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
///
/// **Warum der schreibende Teil in einer eigenen Funktion steckt**
/// (spec-reviewer-Fund, Runde 1): Der Rückweg aus A3 war zuvor dreimal von
/// Hand hingeschrieben, einmal je fehlbarem Schritt. Das hält, solange
/// niemand etwas ergänzt — ein künftiges `?` zwischen Schlüsselbund und
/// Datenbank (etwa eine neue Prüfung) umginge ihn lautlos, und kein Test
/// fiele darauf. Jetzt gibt es genau **einen** Rückweg, am Ergebnis von
/// [`write_edited_server`]: Jeder Fehler darin, auch ein künftig
/// hinzukommender, läuft über ihn. Das ist dieselbe Absicherung, die
/// [`RecordingCredentialStore`] für die Schreibseite leistet — der Rückweg
/// war die letzte Stelle des Entwurfs, die noch von Aufmerksamkeit abhing
/// statt von der Struktur.
pub async fn update_server(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    id: ServerId,
    input: ServerInput,
) -> CommandResult<()> {
    if crate::dto::is_local(id) {
        // Spec 0032, Abschnitt 3: existiert nicht als `servers`-Zeile — nur
        // Notizen/Tags sind editierbar, über die dedizierten
        // `update_local_server_notes`/`update_local_server_tags`-Befehle.
        return Err(CommandError::with_code(
            "Der lokale Pseudo-Server kann nicht auf diesem Weg bearbeitet werden",
            "SERVER_LOCAL_NOT_EDITABLE",
        ));
    }
    reject_local_jump_host(input.jump_host)?;
    // Bis hierher hat der Aufruf nichts geschrieben, ein `?` ist also
    // gefahrlos: Es gibt noch nichts zurückzunehmen. Ab der Hülle unten
    // gilt das nicht mehr — deshalb hört das `?` hier auf.
    let sftp_server_path = checked_sftp_server_path(input.sftp_server_path.clone())?;
    // Spec 0102: bereinigt zurück in die Eingabe, damit `write_edited_server`
    // nur noch den geprüften Wert sieht.
    let mut input = input;
    input.start_directory = checked_start_directory(input.start_directory)?;
    let existing = store.get_server(&id).await.store_err()?;
    let previous_auth = existing.auth.clone();

    // Jedes Schreiben läuft über die aufzeichnende Hülle — nur so weiß der
    // Rückweg, welche Einträge dieser Aufruf angelegt hat (A3). Gelöscht
    // wird bewusst am echten Store, nicht über die Hülle: Die Aufzeichnung
    // soll das Schreiben dieses Aufrufs festhalten, nicht sein Aufräumen.
    let recording = RecordingCredentialStore::new(credential_store);

    match write_edited_server(store, &recording, id, input, existing, sftp_server_path).await {
        Ok(saved_auth) => {
            // **Erst hier** (A2): Die Datenbank trägt jetzt die neue
            // Anmeldeart — was von der bisherigen übrig ist und die neue
            // nicht weiterbenutzt, verweist auf nichts mehr und darf weg.
            // Jede frühere Stelle wäre genau der Fehler, den diese Spec
            // behebt.
            cleanup_replaced_auth_method_secrets(credential_store, id, &previous_auth, &saved_auth);
            Ok(())
        }
        Err(err) => {
            roll_back_failed_edit(credential_store, id, &previous_auth, &recording.written());
            Err(err)
        }
    }
}

/// Der schreibende Teil von [`update_server`]: Schlüsselbund, dann
/// Datenbank. Gibt die gespeicherte [`AuthMethod`] zurück, damit der
/// Aufrufer weiß, welche Einträge die neue Art weiterbenutzt.
///
/// Diese Funktion räumt selbst **nichts** auf — weder nach Erfolg noch
/// nach einem Fehler. Genau das ist ihr Zweck: Sie darf `?` benutzen, weil
/// jeder Ausgang über den Aufrufer läuft und dort behandelt wird. Wer sie
/// erweitert, braucht an den Rückweg nicht zu denken.
async fn write_edited_server(
    store: &dyn ProfileStore,
    // `+ Send + Sync` ist hier nicht Schmuck: Die Referenz wird über das
    // `.await` des DB-Schreibens hinweg gehalten, und ohne die beiden
    // Auto-Traits ist die entstehende Future nicht `Send` — was Tauri für
    // einen Command verlangt. Derselbe Grund wie bei
    // `compute_delete_server_result`.
    credential_store: &(dyn CredentialStore + Send + Sync),
    id: ServerId,
    input: ServerInput,
    existing: Server,
    sftp_server_path: Option<String>,
) -> CommandResult<AuthMethod> {
    let auth = resolve_auth_method(credential_store, id, input.auth, Some(&existing.auth))?;
    resolve_sudo_password(credential_store, id, input.sudo_password)?;

    let server = Server {
        id,
        name: input.name,
        host: input.host,
        port: input.port,
        username: input.username,
        group_id: input.group_id,
        tags: input.tags,
        auth: auth.clone(),
        notes: existing.notes,
        jump_host: input.jump_host,
        post_ingest_policy: input.post_ingest_policy,
        ai_injection_check_enabled: input.ai_injection_check_enabled,
        sftp_server_path,
        start_directory: input.start_directory,
        elevation_allowed: input.elevation_allowed,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    store.update_server(&server).await.store_err()?;
    Ok(auth)
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
    let all_servers = store.list_servers().await.store_err()?;
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
    extension: Option<&dyn crate::session_extension::SessionExtension>,
) -> CommandResult<DeleteServerResult> {
    let server = store.get_server(&id).await.store_err()?;
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
        store.delete_server(&id).await.store_err()?;
        // Spec 0038, section 2a: after the core deleted the server.
        if let Some(extension) = extension {
            extension.server_removed(id);
        }
        result.secrets_left_behind = left_behind
            .into_iter()
            .map(|r| r.as_str().to_string())
            .collect();
    }
    Ok(result)
}

/// Issue #100: die gespeicherten Server, deren Anmeldeart diese Version
/// nicht lesen kann — für den Serverbaum, der sie als „nicht nutzbar"
/// zeigt. Alle übrigen Wege (Verbinden, Bearbeiten, MCP, Export) sehen sie
/// nicht, weil `list_servers` sie nicht enthält und `get_server` für sie
/// scheitert.
pub async fn list_unusable_servers(
    store: &dyn ProfileStore,
) -> CommandResult<Vec<UnusableServerDto>> {
    Ok(store
        .list_server_entries()
        .await?
        .unusable
        .iter()
        .map(UnusableServerDto::from)
        .collect())
}

/// Issue #100: Löschen eines nicht nutzbaren Servers — der einzige Weg,
/// der für ihn offensteht. Wie [`delete_server`] zweistufig: Ohne
/// `confirm` wird nichts gelöscht, nur die Vorschau geliefert.
///
/// Gelöscht werden die Zeile und alle Secrets seines festen Schemas
/// `server:<id>:<slot>` (welche er belegt, ist mit unlesbarer Anmeldeart
/// unbekannt). Was nicht entfernt werden konnte, steht wie bei
/// [`delete_server`] in `secrets_left_behind` (Spec 0071, A17).
///
/// Ein **nutzbarer** Server wird hier abgelehnt: Für ihn gilt
/// [`delete_server`], das seine Secrets aus der gelesenen Anmeldeart
/// bestimmt. So kann dieser Weg nie zu einem lesbaren Server führen.
pub async fn delete_unusable_server(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    id: ServerId,
    confirm: bool,
    extension: Option<&dyn crate::session_extension::SessionExtension>,
) -> CommandResult<DeleteUnusableServerResult> {
    let listing = store.list_server_entries().await.store_err()?;
    let Some(unusable) = listing.unusable.iter().find(|s| s.id == id) else {
        if listing.servers.iter().any(|s| s.id == id) {
            return Err(CommandError::with_code(
                "Der Server ist nutzbar und wird über das normale Löschen entfernt",
                "SERVER_USABLE_NOT_UNUSABLE",
            ));
        }
        return Err(ssh_manager_core::profiles::ProfileError::ServerNotFound(id).into());
    };
    let servers_losing_jump_host = listing
        .servers
        .iter()
        .filter(|s| s.jump_host == Some(id))
        .map(|s| ServerDto::from_server(s, credential_store))
        .collect();
    let mut result = DeleteUnusableServerResult {
        server: UnusableServerDto::from(unusable),
        servers_losing_jump_host,
        executed: confirm,
        secrets_left_behind: Vec::new(),
    };
    if confirm {
        // Dieselbe Reihenfolge wie `delete_server`: erst die Secrets, dann
        // die Zeile — und durchlaufen, auch wenn ein Secret stehen bleibt.
        let left_behind = delete_all_server_secrets_on_unusable_delete(credential_store, id);
        store.delete_server(&id).await.store_err()?;
        if let Some(extension) = extension {
            extension.server_removed(id);
        }
        result.secrets_left_behind = left_behind
            .into_iter()
            .map(|r| r.as_str().to_string())
            .collect();
    }
    Ok(result)
}

/// Issue #48: Verschieben eines Servers in eine andere Gruppe (oder mit
/// `group_id: None` auf die Wurzelebene) per Drag-and-drop — ein eigener,
/// schmaler Weg statt [`update_server`] mit vollständigem `ServerInput`.
///
/// Bewusst **ohne** `CredentialStore`-Parameter: Ein Verschieben ändert nur
/// `group_id`, die Anmeldeart samt ihrer Schlüsselbund-Verweise wird
/// unverändert aus der gespeicherten Zeile übernommen. Damit gibt es hier
/// strukturell keinen Weg, der den Schlüsselbund liest oder beschreibt
/// (Spec 0082 / ADR 0081 betreffen dieses Kommando nicht).
///
/// Die Zielgruppe muss existieren (`get_group` schlägt sonst sichtbar
/// fehl), der lokale Pseudo-Server (Spec 0032) hat keine `servers`-Zeile
/// und ist nicht verschiebbar.
pub async fn move_server_to_group(
    store: &dyn ProfileStore,
    id: ServerId,
    group_id: Option<ssh_manager_core::profiles::GroupId>,
) -> CommandResult<()> {
    if crate::dto::is_local(id) {
        return Err(CommandError::with_code(
            "Der lokale Pseudo-Server kann keiner Gruppe zugeordnet werden",
            "SERVER_LOCAL_NO_GROUP",
        ));
    }
    if let Some(group_id) = group_id {
        store.get_group(&group_id).await.store_err()?;
    }
    let mut server = store.get_server(&id).await.store_err()?;
    if server.group_id == group_id {
        return Ok(());
    }
    server.group_id = group_id;
    server.updated_at = Utc::now();
    store.update_server(&server).await.store_err()?;
    Ok(())
}

#[cfg(test)]
mod tests;
