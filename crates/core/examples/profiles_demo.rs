//! Eigenständig lauffähiges Beispiel für `core::profiles` — ohne UI, ohne DB.
//!
//! Legt eine kleine Beispielhierarchie an (Gruppe "Kunde A" → Untergruppe
//! "Produktion" → Server "web-01"), setzt auf jeder Ebene Notizen und gibt
//! den über `effective_notes()` zusammengesetzten Kontext auf stdout aus.
//!
//! Der Store ist `InMemoryProfileStore` aus `ssh_manager_core::testing` (Issue
//! #909); das Modul gibt es nur mit dem Feature `test-support`, deshalb
//! verlangt das Beispiel es per `required-features`.
//!
//! `#[tokio::main]`, weil `ProfileStore` seit der Umstellung auf
//! `async-trait` (Teil 1 der SQLite-Persistenz-Anbindung, Spec 0004) async
//! ist und `tokio` als Dev-Dependency ohnehin für die Testsuite gebraucht
//! wird (auch Examples dürfen Dev-Dependencies nutzen).
//!
//! Führe aus mit: `cargo run -p ssh-manager-core --example profiles_demo`

use chrono::Utc;
use ssh_manager_core::profiles::{effective_notes, AuthMethod, Group, GroupId, ProfileStore};
use ssh_manager_core::testing::{InMemoryProfileStore, ServerBuilder};

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
    let store = InMemoryProfileStore::new();

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

    let web01 = ServerBuilder::new()
        .name("web-01")
        .host("web-01.kunde-a.internal")
        .group_id(Some(produktion.id))
        .tags(vec!["production".to_string()])
        .auth(AuthMethod::Agent)
        .notes(
            "web-01: PHP 8.2 und MySQL 8 sind unter /opt/lamp installiert, \
             Config liegt in /opt/lamp/conf.",
        )
        .build();

    store.create_group(&kunde_a).await.expect("create group");
    store.create_group(&produktion).await.expect("create group");
    store.create_server(&web01).await.expect("create server");

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
