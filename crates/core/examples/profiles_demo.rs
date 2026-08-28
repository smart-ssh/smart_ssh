//! Eigenständig lauffähiges Beispiel für `core::profiles` — ohne UI, ohne DB.
//!
//! Legt eine kleine Beispielhierarchie an (Gruppe "Kunde A" → Untergruppe
//! "Produktion" → Server "web-01"), setzt auf jeder Ebene Notizen und gibt
//! den über `effective_notes()` zusammengesetzten Kontext auf stdout aus.
//!
//! `InMemoryProfileStore` aus `core::profiles::tests` ist bewusst nur unter
//! `#[cfg(test)]` verfügbar (Aufgabenstellung, Punkt 3) — Cargo-Examples
//! werden aber *nicht* im Test-Modus kompiliert, `#[cfg(test)]`-Items sind
//! hier schlicht nicht vorhanden. Damit dieses Demo trotzdem eigenständig
//! (`cargo run --example profiles_demo`) läuft, definiert es unten eine
//! eigene, minimale `ProfileStore`-Implementierung — das beweist nebenbei,
//! dass der Trait wie vorgesehen auch außerhalb der Testsuite einfach zu
//! implementieren ist.
//!
//! `#[tokio::main]`, weil `ProfileStore` seit der Umstellung auf
//! `async-trait` (Teil 1 der SQLite-Persistenz-Anbindung, Spec 0004) async
//! ist und `tokio` als Dev-Dependency ohnehin für die Testsuite gebraucht
//! wird (auch Examples dürfen Dev-Dependencies nutzen).
//!
//! Führe aus mit: `cargo run -p ssh-manager-core --example profiles_demo`

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use ssh_manager_core::profiles::{
    effective_notes, AuthMethod, Group, GroupId, NoteRevision, NoteTarget, ProfileError,
    ProfileResult, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

/// Minimale, funktionierende `ProfileStore`-Implementierung fürs Demo (kein
/// Mock/Stub) — Mutex-gekapselt, weil der Trait schreibende Methoden über
/// `&self` verlangt (analog zu `SqliteProfileStore`, dessen Connection-Pool
/// intern ebenfalls nur über `&self` genutzt wird).
#[derive(Default)]
struct DemoProfileStore {
    groups: Mutex<HashMap<GroupId, Group>>,
    servers: Mutex<HashMap<ServerId, Server>>,
}

impl DemoProfileStore {
    fn new() -> Self {
        Self::default()
    }

    fn insert_group(&self, group: Group) {
        self.groups.lock().unwrap().insert(group.id, group);
    }

    fn insert_server(&self, server: Server) {
        self.servers.lock().unwrap().insert(server.id, server);
    }
}

#[async_trait]
impl ProfileStore for DemoProfileStore {
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
        self.groups
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or(ProfileError::GroupNotFound(*id))
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
        Ok(())
    }
}

fn new_group(name: &str, parent_id: Option<GroupId>, notes: &str) -> Group {
    let now = Utc::now();
    Group {
        id: GroupId::new(),
        name: name.to_string(),
        parent_id,
        notes: notes.to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::main]
async fn main() {
    let store = DemoProfileStore::new();

    let kunde_a = new_group(
        "Kunde A",
        None,
        "Kunde A ist ein Hosting-Kunde mit mehreren Umgebungen. Ansprechpartner: Team Ops.",
    );
    let produktion = new_group(
        "Produktion",
        Some(kunde_a.id),
        "Produktions-Umgebung von Kunde A. Änderungen nur im Wartungsfenster (Di 22-24 Uhr).",
    );

    let now = Utc::now();
    let web01 = Server {
        id: ServerId::new(),
        name: "web-01".to_string(),
        host: "web-01.kunde-a.internal".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: Some(produktion.id),
        tags: vec!["production".to_string()],
        auth: AuthMethod::Agent,
        notes: "web-01: PHP 8.2 und MySQL 8 sind unter /opt/lamp installiert, \
                Config liegt in /opt/lamp/conf."
            .to_string(),
        jump_host: None,
        created_at: now,
        updated_at: now,
    };

    store.insert_group(kunde_a);
    store.insert_group(produktion);
    store.insert_server(web01.clone());

    match effective_notes(&web01, &store).await {
        Ok(context) => {
            println!(
                "--- Effektiver LLM-Kontext für Session zu \"{}\" ---\n",
                web01.name
            );
            println!("{context}");
        }
        Err(err) => eprintln!("Fehler beim Zusammenbauen des Kontexts: {err}"),
    }
}
