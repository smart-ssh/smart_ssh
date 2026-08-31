use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};

use super::*;
use crate::shared::ServerId;

// --- Test-Only Stores ---------------------------------------------------

struct InMemoryCredentialStore {
    secrets: Mutex<HashMap<String, SecretString>>,
}

impl InMemoryCredentialStore {
    fn new() -> Self {
        Self {
            secrets: Mutex::new(HashMap::new()),
        }
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
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

#[derive(Default)]
struct InMemoryProfileStore {
    // Mutex statt einfachem HashMap, weil die schreibenden `ProfileStore`-
    // Methoden `&self` nehmen (nicht `&mut self`) — analog zu
    // `InMemoryCredentialStore` oben und zur echten `SqliteProfileStore`
    // (Connection-Pool ist auch nur über `&self` erreichbar).
    groups: Mutex<HashMap<GroupId, Group>>,
    servers: Mutex<HashMap<ServerId, Server>>,
    note_revisions: Mutex<Vec<NoteRevision>>,
}

impl InMemoryProfileStore {
    fn new() -> Self {
        Self::default()
    }

    fn with_group(self, group: Group) -> Self {
        self.groups.lock().unwrap().insert(group.id, group);
        self
    }

    fn with_server(self, server: Server) -> Self {
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

    // Bewusst ohne Nachbildung von ON DELETE CASCADE/SET NULL: die
    // Referenzielle-Integritäts-Semantik aus der SQLite-Migration (Spec
    // 0004) wird gegen `SqliteProfileStore` getestet
    // (`persistence-sqlite`-Crate), nicht hier — diese Implementierung dient
    // nur den reinen `profiles`-Unit-Tests (effective_notes, group_chain,
    // record_revision), die keine Kaskaden brauchen.

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

// --- Test-Fixtures --------------------------------------------------------

fn group(name: &str, parent: Option<GroupId>, notes: &str) -> Group {
    let now = Utc::now();
    Group {
        id: GroupId::new(),
        name: name.to_string(),
        parent_id: parent,
        notes: notes.to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn server(name: &str, group_id: Option<GroupId>, notes: &str) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: name.to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        notes: notes.to_string(),
        jump_host: None,
        created_at: now,
        updated_at: now,
    }
}

// --- effective_notes() ----------------------------------------------------

#[tokio::test]
async fn test_effective_notes_orders_root_first_server_last() {
    let root = group("Kunde A", None, "Kunde A: Ansprechpartner ist Team X.");
    let mid = group(
        "Produktion",
        Some(root.id),
        "Produktion: nur read-only ausserhalb Wartungsfenster.",
    );
    let leaf = group(
        "Web-Cluster",
        Some(mid.id),
        "Web-Cluster: nginx als Reverse Proxy.",
    );
    let srv = server(
        "web-01",
        Some(leaf.id),
        "web-01: PHP 8.2, MySQL 8 unter /opt/lamp.",
    );

    let store = InMemoryProfileStore::new()
        .with_group(root.clone())
        .with_group(mid.clone())
        .with_group(leaf.clone())
        .with_server(srv.clone());

    let notes = effective_notes(&srv, &store)
        .await
        .expect("keine zyklische Kette in diesem Test");

    let idx_root = notes
        .find("## Kontext: Kunde A")
        .expect("Kunde A Abschnitt fehlt");
    let idx_mid = notes
        .find("## Kontext: Produktion")
        .expect("Produktion Abschnitt fehlt");
    let idx_leaf = notes
        .find("## Kontext: Web-Cluster")
        .expect("Web-Cluster Abschnitt fehlt");
    let idx_srv = notes
        .find("## Kontext: Server \"web-01\"")
        .expect("Server Abschnitt fehlt");

    assert!(
        idx_root < idx_mid,
        "Wurzel muss vor der mittleren Gruppe stehen"
    );
    assert!(
        idx_mid < idx_leaf,
        "mittlere Gruppe muss vor der Blatt-Gruppe stehen"
    );
    assert!(
        idx_leaf < idx_srv,
        "Server-Kontext muss ganz am Ende stehen"
    );

    assert!(notes.contains("Ansprechpartner ist Team X"));
    assert!(notes.contains("PHP 8.2"));
}

#[tokio::test]
async fn test_effective_notes_skips_empty_notes() {
    let root = group("Kunde A", None, "");
    let mid = group("Produktion", Some(root.id), "   ");
    let srv = server("web-01", Some(mid.id), "web-01: relevant.");

    let store = InMemoryProfileStore::new()
        .with_group(root.clone())
        .with_group(mid.clone())
        .with_server(srv.clone());

    let notes = effective_notes(&srv, &store)
        .await
        .expect("keine zyklische Kette in diesem Test");

    assert!(
        !notes.contains("Kunde A"),
        "leere Notiz darf keinen Abschnitt erzeugen"
    );
    assert!(
        !notes.contains("Produktion"),
        "reine Whitespace-Notiz darf keinen Abschnitt erzeugen"
    );
    assert!(notes.contains("## Kontext: Server \"web-01\""));
    assert!(notes.contains("relevant"));
}

#[tokio::test]
async fn test_effective_notes_server_without_group_yields_only_server_context() {
    let srv = server(
        "standalone",
        None,
        "standalone: eigenständig, keine Gruppe.",
    );
    let store = InMemoryProfileStore::new().with_server(srv.clone());

    let notes = effective_notes(&srv, &store)
        .await
        .expect("keine zyklische Kette in diesem Test");

    assert_eq!(
        notes,
        "## Kontext: Server \"standalone\"\nstandalone: eigenständig, keine Gruppe."
    );
}

#[tokio::test]
async fn test_group_chain_detects_cycle_without_infinite_loop() {
    let mut a = group("A", None, "a-notes");
    let mut b = group("B", None, "b-notes");
    // Simuliert einen Store-Fehler: A -> B -> A.
    a.parent_id = Some(b.id);
    b.parent_id = Some(a.id);

    let store = InMemoryProfileStore::new()
        .with_group(a.clone())
        .with_group(b.clone());

    let result = store.group_chain(&a.id).await;
    assert_eq!(result, Err(ProfileError::CycleDetected));
}

#[tokio::test]
async fn test_effective_notes_propagates_cycle_error_instead_of_hanging() {
    let mut a = group("A", None, "a-notes");
    let mut b = group("B", None, "b-notes");
    a.parent_id = Some(b.id);
    b.parent_id = Some(a.id);
    let srv = server("srv", Some(a.id), "srv-notes");

    let store = InMemoryProfileStore::new()
        .with_group(a)
        .with_group(b)
        .with_server(srv.clone());

    let result = effective_notes(&srv, &store).await;
    assert_eq!(result, Err(ProfileError::CycleDetected));
}

// --- record_revision() -----------------------------------------------------

#[test]
fn test_record_revision_creates_user_variant() {
    let target = NoteTarget::Server(ServerId::new());
    let revision = record_revision(target, "neuer Inhalt".to_string(), NoteEditor::User);

    assert_eq!(revision.content, "neuer Inhalt");
    assert_eq!(revision.target, target);
    assert!(matches!(revision.edited_by, NoteEditor::User));
}

#[test]
fn test_record_revision_creates_ai_variant_with_provider_and_model() {
    let target = NoteTarget::Group(GroupId::new());
    let editor = NoteEditor::Ai {
        provider: "anthropic".to_string(),
        model: "claude-sonnet-5".to_string(),
    };
    let revision = record_revision(target, "KI-Vorschlag".to_string(), editor);

    assert_eq!(revision.content, "KI-Vorschlag");
    assert_eq!(revision.target, target);
    match revision.edited_by {
        NoteEditor::Ai { provider, model } => {
            assert_eq!(provider, "anthropic");
            assert_eq!(model, "claude-sonnet-5");
        }
        NoteEditor::User => panic!("expected NoteEditor::Ai, got NoteEditor::User"),
    }
}

// --- CredentialStore --------------------------------------------------------
// (CredentialStore bleibt in Teil 1 bewusst synchron — nur ProfileStore wird
// umgestellt, s. Aufgabenstellung. Test bleibt daher #[test], nicht
// #[tokio::test].)

#[test]
fn test_in_memory_credential_store_roundtrip_and_not_found() {
    let store = InMemoryCredentialStore::new();
    let r = CredentialRef::new("keychain-entry-1");

    match store.get(&r) {
        Err(CredentialError::NotFound(missing)) => assert_eq!(missing, r),
        other => panic!("expected NotFound, got {other:?}"),
    }

    store
        .set(&r, SecretString::from("s3cr3t".to_string()))
        .unwrap();
    let fetched = store
        .get(&r)
        .expect("Credential sollte jetzt vorhanden sein");
    assert_eq!(fetched.expose_secret(), "s3cr3t");

    store.delete(&r).unwrap();
    assert!(store.get(&r).is_err());
}
