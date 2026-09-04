//! Kleiner CLI-Helfer zum Anlegen eines Testservers in der **echten**
//! App-Datenbank (`default_db_path()`) — es gibt noch keine
//! Server-Anlege-UI (Spec 0007, Abschnitt 7: "Testserver werden vorerst
//! direkt über das `profiles_demo`-Beispiel oder einen kleinen
//! CLI-Helfer angelegt"; `profiles_demo` selbst legt aber nur einen
//! In-Memory-Server an, nicht in der echten SQLite-Datenbank, die die
//! Tauri-App tatsächlich liest).
//!
//! Nutzt `AuthMethod::Agent` (SSH-Agent-Auth) — kein Eintrag im
//! `CredentialStore` nötig, solange der Zielserver einen Public Key kennt,
//! der im lokal laufenden `ssh-agent` geladen ist.
//!
//! Aufruf:
//! ```text
//! cargo run -p persistence-sqlite --example add_test_server -- <name> <host> <port> <username>
//! ```

use chrono::Utc;

use persistence_sqlite::{default_db_path, SqliteProfileStore};
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, name, host, port, username] = args.as_slice() else {
        eprintln!("Aufruf: add_test_server <name> <host> <port> <username>");
        std::process::exit(1);
    };
    let port: u16 = port
        .parse()
        .expect("port muss eine Zahl zwischen 0 und 65535 sein");

    let db_path = default_db_path();
    let store = SqliteProfileStore::connect(&db_path)
        .await
        .expect("Verbindung zur App-Datenbank fehlgeschlagen");

    let now = Utc::now();
    let server = Server {
        id: ServerId::new(),
        name: name.clone(),
        host: host.clone(),
        port,
        username: username.clone(),
        group_id: None,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        created_at: now,
        updated_at: now,
    };

    store
        .create_server(&server)
        .await
        .expect("Server anlegen fehlgeschlagen");

    println!(
        "Server '{}' ({}@{}:{}) angelegt, id={}",
        server.name, server.username, server.host, server.port, server.id.0
    );
    println!("Datenbank: {}", db_path.display());
}
