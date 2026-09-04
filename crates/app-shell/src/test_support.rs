//! Gemeinsame Test-Doubles für die Spec-0008-Unit-Tests (`groups`,
//! `server_credentials`, `test_connection`) — nur unter `#[cfg(test)]`
//! eingebunden (s. `lib.rs`). Ein geteiltes Modul statt einer
//! Neuimplementierung von `ProfileStore`/`CredentialStore` pro Testdatei:
//! alle drei brauchen dieselbe vollständige In-Memory-Semantik (u. a.
//! einen echten `get_group`-Lookup, damit `ProfileStore::group_chain`s
//! Default-Implementierung tatsächlich funktioniert), Duplikation hätte
//! hier keinen Isolationsvorteil gebracht wie bei den bewusst getrennten
//! Mocks in anderen Crates.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use secrecy::SecretString;

use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore, Group, GroupId,
    NoteRevision, NoteTarget, ProfileError, ProfileResult, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

#[derive(Default)]
pub struct InMemoryProfileStore {
    pub groups: Mutex<HashMap<GroupId, Group>>,
    pub servers: Mutex<HashMap<ServerId, Server>>,
    pub note_revisions: Mutex<Vec<NoteRevision>>,
}

impl InMemoryProfileStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_group(self, group: Group) -> Self {
        self.groups.lock().unwrap().insert(group.id, group);
        self
    }

    pub fn with_server(self, server: Server) -> Self {
        self.servers.lock().unwrap().insert(server.id, server);
        self
    }
}

#[async_trait]
impl ProfileStore for InMemoryProfileStore {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        self.servers
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(ProfileError::ServerNotFound(*id))
    }

    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        self.groups
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(ProfileError::GroupNotFound(*id))
    }

    async fn list_servers(&self) -> ProfileResult<Vec<Server>> {
        Ok(self.servers.lock().unwrap().values().cloned().collect())
    }

    async fn list_groups(&self) -> ProfileResult<Vec<Group>> {
        Ok(self.groups.lock().unwrap().values().cloned().collect())
    }

    async fn create_group(&self, group: &Group) -> ProfileResult<()> {
        self.groups.lock().unwrap().insert(group.id, group.clone());
        Ok(())
    }

    async fn update_group(&self, group: &Group) -> ProfileResult<()> {
        let mut groups = self.groups.lock().unwrap();
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
        let mut groups = self.groups.lock().unwrap();
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

        let mut servers = self.servers.lock().unwrap();
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
        self.servers
            .lock()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }

    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        let mut servers = self.servers.lock().unwrap();
        if !servers.contains_key(&server.id) {
            return Err(ProfileError::ServerNotFound(server.id));
        }
        servers.insert(server.id, server.clone());
        Ok(())
    }

    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()> {
        self.servers
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or(ProfileError::ServerNotFound(*id))
    }

    async fn record_note_revision(&self, revision: &NoteRevision) -> ProfileResult<()> {
        match revision.target {
            NoteTarget::Server(id) => {
                let mut servers = self.servers.lock().unwrap();
                let server = servers
                    .get_mut(&id)
                    .ok_or(ProfileError::ServerNotFound(id))?;
                server.notes = revision.content.clone();
                server.updated_at = revision.created_at;
            }
            NoteTarget::Group(id) => {
                let mut groups = self.groups.lock().unwrap();
                let group = groups.get_mut(&id).ok_or(ProfileError::GroupNotFound(id))?;
                group.notes = revision.content.clone();
                group.updated_at = revision.created_at;
            }
        }
        self.note_revisions.lock().unwrap().push(revision.clone());
        Ok(())
    }

    async fn list_note_revisions(&self, target: NoteTarget) -> ProfileResult<Vec<NoteRevision>> {
        Ok(self
            .note_revisions
            .lock()
            .unwrap()
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
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_secret(self, r: &CredentialRef, value: &str) -> Self {
        self.secrets.lock().unwrap().insert(
            r.as_str().to_string(),
            SecretString::from(value.to_string()),
        );
        self
    }

    pub fn get_calls(&self) -> usize {
        *self.get_calls.lock().unwrap()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        *self.get_calls.lock().unwrap() += 1;
        self.secrets
            .lock()
            .unwrap()
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        self.secrets
            .lock()
            .unwrap()
            .insert(r.as_str().to_string(), value);
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        self.secrets.lock().unwrap().remove(r.as_str());
        Ok(())
    }
}
