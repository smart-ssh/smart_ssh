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
//! Führe aus mit: `cargo run -p ssh-manager-core --example profiles_demo`

use std::collections::HashMap;

use chrono::Utc;
use ssh_manager_core::profiles::{
    effective_notes, AuthMethod, Group, GroupId, ProfileError, ProfileResult, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

struct DemoProfileStore {
    groups: HashMap<GroupId, Group>,
    servers: HashMap<ServerId, Server>,
}

impl DemoProfileStore {
    fn new() -> Self {
        Self {
            groups: HashMap::new(),
            servers: HashMap::new(),
        }
    }

    fn insert_group(&mut self, group: Group) {
        self.groups.insert(group.id, group);
    }

    fn insert_server(&mut self, server: Server) {
        self.servers.insert(server.id, server);
    }
}

impl ProfileStore for DemoProfileStore {
    fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        self.servers
            .get(id)
            .cloned()
            .ok_or(ProfileError::ServerNotFound(*id))
    }

    fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        self.groups
            .get(id)
            .cloned()
            .ok_or(ProfileError::GroupNotFound(*id))
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

fn main() {
    let mut store = DemoProfileStore::new();

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

    match effective_notes(&web01, &store) {
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
