use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};

use super::*;
use crate::shared::ServerId;
use crate::testing::{InMemoryCredentialStore, InMemoryProfileStore, ServerBuilder};

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
    let srv = ServerBuilder::new()
        .name("web-01")
        .group_id(Some(leaf.id))
        .notes("web-01: PHP 8.2, MySQL 8 unter /opt/lamp.")
        .build();

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
    let srv = ServerBuilder::new()
        .name("web-01")
        .group_id(Some(mid.id))
        .notes("web-01: relevant.")
        .build();

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
    let srv = ServerBuilder::new()
        .name("standalone")
        .group_id(None)
        .notes("standalone: eigenständig, keine Gruppe.")
        .build();
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
    let srv = ServerBuilder::new()
        .name("srv")
        .group_id(Some(a.id))
        .notes("srv-notes")
        .build();

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
