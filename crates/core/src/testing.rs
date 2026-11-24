//! Shared test doubles and fixtures (issue #909): `ServerBuilder`,
//! `InMemoryProfileStore` and `InMemoryCredentialStore`.
//!
//! Compiled only for this crate's own tests and for dependants that enable the
//! `test-support` feature; production builds never include it. The doubles
//! lock through [`crate::sync`], so a panicking test thread cannot poison them
//! for the rest of a test (issue #262).
//!
//! Specialised doubles that inject a failure the stores below do not offer
//! stay next to the test that needs them and are named for what they inject.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use async_trait::async_trait;
use secrecy::SecretString;

use crate::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore, Group, GroupId,
    NoteRevision, NoteTarget, ProfileError, ProfileResult, ProfileStore, Server, ServerListing,
    UnusableServer,
};
use crate::shared::ServerId;

/// Poison-tolerant `lock()` shorthand for the doubles in this module.
trait LockExt<T> {
    fn lk(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lk(&self) -> MutexGuard<'_, T> {
        crate::sync::lock_tolerating_poison(self)
    }
}

/// Builds a [`Server`] for tests. Defaults: random id, name `test-server`,
/// host `example.invalid`, port 22, user `deploy`, agent auth, no group, no
/// tags, empty notes, no jump host, default post-ingest policy, both
/// timestamps "now". Use a `Server { .. }` literal only where a test needs to
/// show every field.
#[derive(Debug, Clone)]
pub struct ServerBuilder {
    server: Server,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerBuilder {
    pub fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            server: Server {
                id: ServerId::new(),
                name: "test-server".to_string(),
                host: "example.invalid".to_string(),
                port: 22,
                username: "deploy".to_string(),
                group_id: None,
                tags: Vec::new(),
                auth: AuthMethod::Agent,
                notes: String::new(),
                jump_host: None,
                post_ingest_policy: Default::default(),
                ai_injection_check_enabled: false,
                sftp_server_path: None,
                elevation_allowed: true,
                start_directory: None,
                created_at: now,
                updated_at: now,
            },
        }
    }

    pub fn id(mut self, id: ServerId) -> Self {
        self.server.id = id;
        self
    }

    pub fn name(mut self, name: &str) -> Self {
        self.server.name = name.to_string();
        self
    }

    pub fn host(mut self, host: &str) -> Self {
        self.server.host = host.to_string();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.server.port = port;
        self
    }

    pub fn username(mut self, username: &str) -> Self {
        self.server.username = username.to_string();
        self
    }

    pub fn group_id(mut self, group_id: Option<GroupId>) -> Self {
        self.server.group_id = group_id;
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.server.tags = tags;
        self
    }

    pub fn auth(mut self, auth: AuthMethod) -> Self {
        self.server.auth = auth;
        self
    }

    pub fn notes(mut self, notes: &str) -> Self {
        self.server.notes = notes.to_string();
        self
    }

    pub fn jump_host(mut self, jump_host: Option<ServerId>) -> Self {
        self.server.jump_host = jump_host;
        self
    }

    pub fn post_ingest_policy(mut self, policy: crate::profiles::PostIngestPolicy) -> Self {
        self.server.post_ingest_policy = policy;
        self
    }

    pub fn ai_injection_check_enabled(mut self, enabled: bool) -> Self {
        self.server.ai_injection_check_enabled = enabled;
        self
    }

    pub fn sftp_server_path(mut self, path: Option<&str>) -> Self {
        self.server.sftp_server_path = path.map(str::to_string);
        self
    }

    pub fn elevation_allowed(mut self, allowed: bool) -> Self {
        self.server.elevation_allowed = allowed;
        self
    }

    pub fn start_directory(mut self, dir: Option<&str>) -> Self {
        self.server.start_directory = dir.map(str::to_string);
        self
    }

    /// Sets `created_at` and `updated_at` to the same instant.
    pub fn timestamps(mut self, at: chrono::DateTime<chrono::Utc>) -> Self {
        self.server.created_at = at;
        self.server.updated_at = at;
        self
    }

    pub fn build(self) -> Server {
        self.server
    }
}

#[derive(Default)]
pub struct InMemoryProfileStore {
    pub groups: Mutex<HashMap<GroupId, Group>>,
    pub servers: Mutex<HashMap<ServerId, Server>>,
    /// Issue #100: gespeicherte Server, deren Anmeldeart unlesbar ist —
    /// nur über `list_server_entries` sichtbar, `get_server` findet sie
    /// nicht (wie beim SQLite-Store, der dort sichtbar scheitert).
    pub unusable: Mutex<Vec<UnusableServer>>,
    pub note_revisions: Mutex<Vec<NoteRevision>>,
    /// Spec 0047, Fund A2: lässt `create_server` deterministisch
    /// fehlschlagen (simuliert einen DB-Fehler NACH bereits geschriebenen
    /// Keychain-Secrets), damit der Keychain-Rollback-Pfad in
    /// `servers::create_server` gegen einen echten Fehler getestet werden
    /// kann, statt nur den Erfolgsfall abzudecken.
    pub fail_create_server: bool,
    /// Spec 0076, C-6 (zweite Richtung): lässt `update_server`
    /// deterministisch fehlschlagen — simuliert einen DB-Fehler **nach**
    /// dem bereits geschriebenen Schlüsselbund-Eintrag, damit der Rollback
    /// der Überführung gegen einen echten Fehler geprüft werden kann.
    pub fail_update_server: bool,
    /// Spec 0075, §6.3.9: lässt `create_server` **ab dem n-ten** Aufruf
    /// scheitern. `fail_create_server` allein reicht dort nicht: Ein Fehler
    /// schon beim ersten Insert erreicht den interessanten Zustand nie —
    /// geprüft werden soll die Rücknahme, wenn bereits Profile **und** eine
    /// Gruppe angelegt sind (§3.1.11).
    pub fail_create_server_after: Mutex<Option<usize>>,
    create_server_calls: Mutex<usize>,
    /// Issue #843: `get_server` fails with a backend error.
    pub fail_get_server: std::sync::atomic::AtomicBool,
    /// Issue #843: `get_group` (and with it `group_chain`) fails with a
    /// backend error.
    pub fail_get_group: std::sync::atomic::AtomicBool,
}

impl InMemoryProfileStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_group(self, group: Group) -> Self {
        self.groups.lk().insert(group.id, group);
        self
    }

    pub fn with_server(self, server: Server) -> Self {
        self.servers.lk().insert(server.id, server);
        self
    }

    /// Issue #100: ein nicht nutzbarer Server (Anmeldeart unlesbar).
    pub fn with_unusable_server(self, server: UnusableServer) -> Self {
        self.unusable.lk().push(server);
        self
    }

    pub fn with_failing_create_server(mut self) -> Self {
        self.fail_create_server = true;
        self
    }

    pub fn with_failing_update_server(mut self) -> Self {
        self.fail_update_server = true;
        self
    }

    /// Spec 0075, §6.3.9: ab dem `n`-ten `create_server`-Aufruf (1-basiert)
    /// scheitert jeder weitere.
    pub fn fail_create_server_after(&self, n: usize) {
        *self.fail_create_server_after.lk() = Some(n);
    }
}

#[async_trait]
impl ProfileStore for InMemoryProfileStore {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        if self
            .fail_get_server
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ProfileError::Backend("disk I/O error".to_string()));
        }
        self.servers
            .lk()
            .get(id)
            .cloned()
            .ok_or(ProfileError::ServerNotFound(*id))
    }

    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        if self
            .fail_get_group
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ProfileError::Backend("disk I/O error".to_string()));
        }
        self.groups
            .lk()
            .get(id)
            .cloned()
            .ok_or(ProfileError::GroupNotFound(*id))
    }

    async fn list_servers(&self) -> ProfileResult<Vec<Server>> {
        Ok(self.servers.lk().values().cloned().collect())
    }

    async fn list_server_entries(&self) -> ProfileResult<ServerListing> {
        Ok(ServerListing {
            servers: self.list_servers().await?,
            unusable: self.unusable.lk().clone(),
        })
    }

    async fn list_groups(&self) -> ProfileResult<Vec<Group>> {
        Ok(self.groups.lk().values().cloned().collect())
    }

    async fn create_group(&self, group: &Group) -> ProfileResult<()> {
        self.groups.lk().insert(group.id, group.clone());
        Ok(())
    }

    async fn update_group(&self, group: &Group) -> ProfileResult<()> {
        let mut groups = self.groups.lk();
        if !groups.contains_key(&group.id) {
            return Err(ProfileError::GroupNotFound(group.id));
        }
        groups.insert(group.id, group.clone());
        Ok(())
    }

    async fn delete_group(&self, id: &GroupId) -> ProfileResult<()> {
        // Bildet CASCADE/SET NULL grob nach, damit Tests gegen
        // `compute_delete_group_result` auch das tatsächliche Löschen
        // verifizieren können (nicht nur die Vorschau).
        let mut groups = self.groups.lk();
        if groups.remove(id).is_none() {
            return Err(ProfileError::GroupNotFound(*id));
        }
        let descendant_ids: Vec<GroupId> = {
            let mut affected = vec![*id];
            let mut result = Vec::new();
            let mut i = 0;
            while i < affected.len() {
                let current = affected[i];
                i += 1;
                for g in groups.values() {
                    if g.parent_id == Some(current) {
                        result.push(g.id);
                        affected.push(g.id);
                    }
                }
            }
            result
        };
        for gid in &descendant_ids {
            groups.remove(gid);
        }
        drop(groups);

        let mut servers = self.servers.lk();
        for server in servers.values_mut() {
            if server.group_id == Some(*id)
                || server.group_id.is_some_and(|g| descendant_ids.contains(&g))
            {
                server.group_id = None;
            }
        }
        Ok(())
    }

    async fn create_server(&self, server: &Server) -> ProfileResult<()> {
        if self.fail_create_server {
            return Err(ProfileError::Backend(
                "simulierter DB-Fehler (Test)".to_string(),
            ));
        }
        {
            let mut calls = self.create_server_calls.lk();
            *calls += 1;
            if let Some(n) = *self.fail_create_server_after.lk() {
                if *calls >= n {
                    return Err(ProfileError::Backend(
                        "simulierter DB-Fehler ab Aufruf n (Test)".to_string(),
                    ));
                }
            }
        }
        self.servers.lk().insert(server.id, server.clone());
        Ok(())
    }

    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        if self.fail_update_server {
            return Err(ProfileError::Backend(
                "simulierter DB-Fehler beim Speichern des Servers".to_string(),
            ));
        }
        let mut servers = self.servers.lk();
        if !servers.contains_key(&server.id) {
            return Err(ProfileError::ServerNotFound(server.id));
        }
        servers.insert(server.id, server.clone());
        Ok(())
    }

    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()> {
        // Spec 0046, Fund 1: bildet `ON DELETE SET NULL` auf `jump_host`
        // grob nach, damit Tests gegen `compute_delete_server_result` auch
        // das tatsächliche Löschen verifizieren können (nicht nur die
        // Vorschau) — analog zum `group_id`-Nachbau in `delete_group` oben.
        let mut servers = self.servers.lk();
        if servers.remove(id).is_none() {
            let mut unusable = self.unusable.lk();
            let before = unusable.len();
            unusable.retain(|u| u.id != *id);
            if unusable.len() == before {
                return Err(ProfileError::ServerNotFound(*id));
            }
        }
        for server in servers.values_mut() {
            if server.jump_host == Some(*id) {
                server.jump_host = None;
            }
        }
        Ok(())
    }

    async fn record_note_revision(&self, revision: &NoteRevision) -> ProfileResult<()> {
        match revision.target {
            NoteTarget::Server(id) => {
                let mut servers = self.servers.lk();
                let server = servers
                    .get_mut(&id)
                    .ok_or(ProfileError::ServerNotFound(id))?;
                server.notes = revision.content.clone();
                server.updated_at = revision.created_at;
            }
            NoteTarget::Group(id) => {
                let mut groups = self.groups.lk();
                let group = groups.get_mut(&id).ok_or(ProfileError::GroupNotFound(id))?;
                group.notes = revision.content.clone();
                group.updated_at = revision.created_at;
            }
        }
        self.note_revisions.lk().push(revision.clone());
        Ok(())
    }

    async fn list_note_revisions(&self, target: NoteTarget) -> ProfileResult<Vec<NoteRevision>> {
        Ok(self
            .note_revisions
            .lk()
            .iter()
            .filter(|r| r.target == target)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
pub struct InMemoryCredentialStore {
    pub secrets: Mutex<HashMap<String, SecretString>>,
    /// Spec 0022, Abschnitt 3: Anzahl der `get()`-Aufrufe, für Tests, die
    /// nachweisen sollen, dass ein Credential über mehrere Operationen
    /// hinweg (mehrere `send_chat_message`-/`execute()`-Aufrufe) nur
    /// einmalig abgerufen und danach aus dem In-Memory-Cache (`Session`-
    /// Feld/`AiProvider`-Instanz) bedient wird, statt erneut den Store zu
    /// befragen.
    get_calls: Mutex<usize>,
    /// Spec 0047, Fund A2: lässt `set()` für jeden Ref fehlschlagen, dessen
    /// Slot-Suffix (z. B. `:sudo_password`) hierauf passt — Suffix statt
    /// exaktem Ref, weil `create_server` seine `ServerId` intern frisch
    /// erzeugt (Test kennt die konkrete ID vorab nicht). Simuliert einen
    /// Keychain-Fehler NACH bereits erfolgreich geschriebenen anderen
    /// Slots (z. B. das Sudo-Passwort nach einer bereits gespeicherten
    /// Auth-Methode) — für den Rollback-Test in `servers::create_server`.
    fail_set_for_slot_suffix: Option<String>,
    /// Spec 0071, X4: lässt `get()` mit `CredentialError::Backend`
    /// fehlschlagen, **obwohl** der Wert hinterlegt ist — der Fall "der
    /// Schlüsselbund kann nicht antworten", der sich von "kein Eintrag
    /// vorhanden" unterscheiden muss.
    fail_get_with_backend: bool,
    /// Spec 0071, A17: lässt `delete()` mit `CredentialError::Backend`
    /// fehlschlagen — der Fall „der Nutzer fordert das Entfernen an, der
    /// Schlüsselbund kann es nicht ausführen". Der Wert bleibt dabei
    /// absichtlich gespeichert, damit ein Test nachweisen kann, dass das
    /// Secret tatsächlich zurückbleibt.
    fail_delete_with_backend: bool,
    /// Spec 0098, §7: einstellbare Nutzlast für **jeden** der
    /// `Backend`-Fehler dieses Stores. Tests setzen sie auf den Marker
    /// `LIBTEXT-0098 Geheim-0098` und suchen danach in allem, was ans
    /// Frontend geht — fest verdrahtete Texte je Methode taugen dafür
    /// nicht, weil der Test dann drei verschiedene Nadeln bräuchte.
    backend_payload: Option<String>,
    /// Spec 0098, T4 (A3): lässt `get()`/`set()`/`delete()` nur so oft
    /// scheitern, wie hier eingetragen ist, und danach normal
    /// weiterarbeiten — der Fall „ein einzelner Zugriff ging schief, der
    /// Schlüsselbund ist deswegen nicht kaputt".
    ///
    /// `Mutex`, nicht `Cell`: Der Store wird als `&dyn CredentialStore`
    /// benutzt, also über eine geteilte Referenz.
    remaining_failures: Mutex<Option<usize>>,
    /// Spec 0098, §7: lässt `get()` nur für Refs mit diesem Slot-Suffix
    /// scheitern — das Gegenstück zu [`Self::with_failing_set_for_slot`]
    /// für die lesenden Wege. Ohne das trifft `with_failing_get` auch
    /// Lesezugriffe, die der Test gar nicht prüfen will (etwa die
    /// Passphrase, während es um das Passwort geht).
    fail_get_for_slot_suffix: Option<String>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_secret(self, r: &CredentialRef, value: &str) -> Self {
        self.secrets.lk().insert(
            r.as_str().to_string(),
            SecretString::from(value.to_string()),
        );
        self
    }

    /// `slot`, z. B. `"sudo_password"` — matcht jeden Ref, dessen letztes
    /// `:`-getrenntes Segment gleich `slot` ist (s. `credential_ref` in
    /// `server_credentials.rs`: `"server:{id}:{slot}"`).
    pub fn with_failing_set_for_slot(mut self, slot: &str) -> Self {
        self.fail_set_for_slot_suffix = Some(format!(":{slot}"));
        self
    }

    /// Spec 0071, X4: simuliert einen nicht erreichbaren Schlüsselbund beim
    /// **Lesen**. Bewusst unabhängig von `with_secret`, damit sich der
    /// kritische Fall bauen lässt: Wert ist hinterlegt, der Store kann es
    /// aber nicht sagen.
    pub fn with_failing_get(mut self) -> Self {
        self.fail_get_with_backend = true;
        self
    }

    /// Spec 0098, §7: wie [`Self::with_failing_get`], aber nur für den
    /// genannten Slot (z. B. `"password"`). `slot` matcht das letzte
    /// `:`-getrennte Segment, genau wie bei
    /// [`Self::with_failing_set_for_slot`].
    pub fn with_failing_get_for_slot(mut self, slot: &str) -> Self {
        self.fail_get_with_backend = true;
        self.fail_get_for_slot_suffix = Some(format!(":{slot}"));
        self
    }

    /// Spec 0071, A17: simuliert einen nicht erreichbaren Schlüsselbund
    /// beim **Löschen**.
    pub fn with_failing_delete(mut self) -> Self {
        self.fail_delete_with_backend = true;
        self
    }

    /// Spec 0098, §7: die Nutzlast, mit der jeder `Backend`-Fehler dieses
    /// Stores scheitert. Tests setzen hier den Marker, nach dem sie
    /// anschließend in `message`, `code` und allem anderen suchen, was ans
    /// Frontend geht.
    pub fn with_backend_payload(mut self, payload: &str) -> Self {
        self.backend_payload = Some(payload.to_string());
        self
    }

    /// Spec 0098, T4 (A3): scheitert genau `times` Mal und arbeitet danach
    /// normal weiter. Nur sinnvoll zusammen mit einem der
    /// `with_failing_*`-Schalter — dieser hier begrenzt, **wie lange** der
    /// eingeschaltete Fehler anhält.
    pub fn failing_only_times(self, times: usize) -> Self {
        *self.remaining_failures.lk() = Some(times);
        self
    }

    pub fn get_calls(&self) -> usize {
        *self.get_calls.lk()
    }

    /// Spec 0098, §7: der Fehler, mit dem dieser Store scheitert — mit der
    /// eingestellten Nutzlast, sonst mit `fallback`.
    fn backend_error(&self, fallback: &str) -> CredentialError {
        CredentialError::Backend(
            self.backend_payload
                .clone()
                .unwrap_or_else(|| fallback.to_string()),
        )
    }

    /// Spec 0098, T4: `true`, solange der eingeschaltete Fehler noch
    /// auftreten darf. Zählt dabei herunter — ohne
    /// [`Self::failing_only_times`] (also `None`) gilt er unbegrenzt, wie
    /// bisher.
    fn may_fail_now(&self) -> bool {
        let mut remaining = self.remaining_failures.lk();
        match remaining.as_mut() {
            None => true,
            Some(0) => false,
            Some(left) => {
                *left -= 1;
                true
            }
        }
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        *self.get_calls.lk() += 1;
        let slot_matches = self
            .fail_get_for_slot_suffix
            .as_deref()
            .is_none_or(|suffix| r.as_str().ends_with(suffix));
        if self.fail_get_with_backend && slot_matches && self.may_fail_now() {
            return Err(self.backend_error("simulierter Keychain-Lesefehler (Test)"));
        }
        self.secrets
            .lk()
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        if self
            .fail_set_for_slot_suffix
            .as_deref()
            .is_some_and(|suffix| r.as_str().ends_with(suffix))
            && self.may_fail_now()
        {
            return Err(self.backend_error("simulierter Keychain-Fehler (Test)"));
        }
        self.secrets.lk().insert(r.as_str().to_string(), value);
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        if self.fail_delete_with_backend && self.may_fail_now() {
            // Absichtlich **ohne** `remove`: Das Secret bleibt stehen, so
            // wie es ein echter Schlüsselbund täte, der den Löschauftrag
            // nicht ausführen konnte.
            return Err(self.backend_error("simulierter Keychain-Löschfehler (Test)"));
        }
        self.secrets.lk().remove(r.as_str());
        Ok(())
    }
}
