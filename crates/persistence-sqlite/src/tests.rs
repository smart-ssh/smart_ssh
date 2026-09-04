//! Testsuite für `SqliteProfileStore` — setzt den Testfall-Katalog aus
//! `docs/specs/0004-sqlite-persistence.md`, Abschnitt 6, um. Jeder Test läuft
//! gegen eine frische In-Memory-SQLite-Instanz mit angewendeten Migrationen,
//! kein geteilter State zwischen Tests.

use chrono::Utc;
use sqlx::sqlite::SqliteConnectOptions;
use uuid::Uuid;

use ssh_manager_core::profiles::{
    AuthMethod, Group, GroupId, NoteEditor, NoteRevision, NoteTarget, PostIngestPolicy,
    ProfileError, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::SqliteProfileStore;

async fn in_memory_store() -> SqliteProfileStore {
    let options = SqliteConnectOptions::new().filename(":memory:");
    SqliteProfileStore::connect_with(options)
        .await
        .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein")
}

fn make_group(name: &str, parent: Option<GroupId>) -> Group {
    let now = Utc::now();
    Group {
        id: GroupId::new(),
        name: name.to_string(),
        parent_id: parent,
        notes: String::new(),
        created_at: now,
        updated_at: now,
    }
}

fn make_server(name: &str, group_id: Option<GroupId>, tags: Vec<String>) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: name.to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id,
        tags,
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        created_at: now,
        updated_at: now,
    }
}

/// Spec Abschnitt 6, Testfall 1: "Gruppe anlegen, Server anlegen, wieder
/// abrufen → Felder identisch".
#[tokio::test]
async fn test_create_and_get_group_and_server_roundtrip() {
    let store = in_memory_store().await;

    let group = make_group("Kunde A", None);
    store.create_group(&group).await.unwrap();

    let fetched_group = store.get_group(&group.id).await.unwrap();
    assert_eq!(fetched_group, group);

    // Tags alphabetisch sortiert angelegt, da `get_server` sie sortiert
    // zurückgibt (s. `SqliteProfileStore::fetch_tags`) — sonst würde der
    // Vec<String>-Vergleich an der Reihenfolge scheitern, nicht am Inhalt.
    let server = make_server(
        "web-01",
        Some(group.id),
        vec!["production".to_string(), "web".to_string()],
    );
    store.create_server(&server).await.unwrap();

    let fetched_server = store.get_server(&server.id).await.unwrap();
    assert_eq!(fetched_server, server);
}

/// Spec Abschnitt 6, Testfall 2: "Gruppenkette über 3 Ebenen korrekt von
/// Wurzel bis Blatt zurückgegeben".
#[tokio::test]
async fn test_group_chain_across_three_levels() {
    let store = in_memory_store().await;

    let root = make_group("Kunde A", None);
    store.create_group(&root).await.unwrap();
    let mid = make_group("Produktion", Some(root.id));
    store.create_group(&mid).await.unwrap();
    let leaf = make_group("Web-Cluster", Some(mid.id));
    store.create_group(&leaf).await.unwrap();

    let chain = store.group_chain(&leaf.id).await.unwrap();
    let names: Vec<&str> = chain.iter().map(|g| g.name.as_str()).collect();

    assert_eq!(names, vec!["Kunde A", "Produktion", "Web-Cluster"]);
}

/// Spec Abschnitt 6, Testfall 3: "Server-Löschung entfernt zugehörige
/// server_tags, aber nicht die Gruppe".
#[tokio::test]
async fn test_delete_server_removes_tags_but_not_group() {
    let store = in_memory_store().await;

    let group = make_group("Kunde A", None);
    store.create_group(&group).await.unwrap();
    let server = make_server("web-01", Some(group.id), vec!["production".to_string()]);
    store.create_server(&server).await.unwrap();

    store.delete_server(&server.id).await.unwrap();

    assert!(matches!(
        store.get_server(&server.id).await,
        Err(ProfileError::ServerNotFound(id)) if id == server.id
    ));

    let group_still_there = store.get_group(&group.id).await.unwrap();
    assert_eq!(group_still_there, group);

    let tag_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM server_tags WHERE server_id = ?")
        .bind(server.id.0.to_string())
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(
        tag_count, 0,
        "server_tags-Zeilen müssen mit dem Server kaskadieren"
    );
}

/// Spec Abschnitt 6, Testfall 4: "Gruppen-Löschung setzt group_id
/// betroffener Server auf NULL, löscht sie nicht".
#[tokio::test]
async fn test_delete_group_sets_server_group_id_null_and_keeps_server() {
    let store = in_memory_store().await;

    let group = make_group("Kunde A", None);
    store.create_group(&group).await.unwrap();
    let server = make_server("web-01", Some(group.id), vec![]);
    store.create_server(&server).await.unwrap();

    store.delete_group(&group.id).await.unwrap();

    assert!(matches!(
        store.get_group(&group.id).await,
        Err(ProfileError::GroupNotFound(id)) if id == group.id
    ));

    let fetched_server = store.get_server(&server.id).await.unwrap();
    assert_eq!(fetched_server.group_id, None);
    assert_eq!(
        fetched_server.id, server.id,
        "Server selbst darf nicht gelöscht werden"
    );
}

/// Spec Abschnitt 6, Testfall 5: "note_revisions werden beim Schreiben einer
/// neuen Notiz-Version zusätzlich zum aktuellen notes-Feld persistiert
/// (nicht ersetzt)".
#[tokio::test]
async fn test_record_note_revision_persists_history_and_updates_notes_field() {
    let store = in_memory_store().await;

    let server = make_server("web-01", None, vec![]);
    store.create_server(&server).await.unwrap();

    let revision1 = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Server(server.id),
        content: "erste Notiz".to_string(),
        edited_by: NoteEditor::User,
        created_at: Utc::now(),
    };
    store.record_note_revision(&revision1).await.unwrap();

    let after_first = store.get_server(&server.id).await.unwrap();
    assert_eq!(after_first.notes, "erste Notiz");

    let revision2 = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Server(server.id),
        content: "zweite Notiz, von KI vorgeschlagen".to_string(),
        edited_by: NoteEditor::Ai {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-5".to_string(),
        },
        created_at: Utc::now(),
    };
    store.record_note_revision(&revision2).await.unwrap();

    let after_second = store.get_server(&server.id).await.unwrap();
    assert_eq!(after_second.notes, "zweite Notiz, von KI vorgeschlagen");

    // Historie: beide Revisionen bleiben erhalten (Insert, kein Replace).
    let history_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM note_revisions WHERE target_type = 'server' AND target_id = ?",
    )
    .bind(server.id.0.to_string())
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(history_count, 2);
}

/// Regressionstest für `ProfileStore::list_note_revisions` selbst — bislang
/// nur indirekt über eine rohe `SELECT COUNT(*)`-Abfrage gegen `store.pool`
/// abgedeckt (s. Test oben), nie über die tatsächliche Trait-Methode, die
/// `list_note_revisions` (der Tauri-Command und damit die Notiz-Historie im
/// Server-/Gruppen-Formular) wirklich aufruft. Deckt außerdem ab, dass zwei
/// verschiedene Ziele (zwei Server) einander nicht ins Gehege kommen — ein
/// falsch gebundener `target_id`-Parameter würde hier entweder leere oder
/// vermischte Ergebnisse liefern.
#[tokio::test]
async fn test_list_note_revisions_returns_persisted_revisions_for_correct_target() {
    let store = in_memory_store().await;

    let server_a = make_server("server-a", None, vec![]);
    let server_b = make_server("server-b", None, vec![]);
    store.create_server(&server_a).await.unwrap();
    store.create_server(&server_b).await.unwrap();

    let revision_a1 = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Server(server_a.id),
        content: "A: erste Notiz".to_string(),
        edited_by: NoteEditor::User,
        created_at: Utc::now(),
    };
    let revision_a2 = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Server(server_a.id),
        content: "A: zweite Notiz".to_string(),
        edited_by: NoteEditor::Ai {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-5".to_string(),
        },
        created_at: Utc::now(),
    };
    let revision_b1 = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Server(server_b.id),
        content: "B: einzige Notiz".to_string(),
        edited_by: NoteEditor::User,
        created_at: Utc::now(),
    };
    store.record_note_revision(&revision_a1).await.unwrap();
    store.record_note_revision(&revision_a2).await.unwrap();
    store.record_note_revision(&revision_b1).await.unwrap();

    let history_a = store
        .list_note_revisions(NoteTarget::Server(server_a.id))
        .await
        .unwrap();
    assert_eq!(
        history_a.len(),
        2,
        "server-a sollte genau seine 2 Revisionen sehen"
    );
    assert_eq!(history_a[0].content, "A: erste Notiz");
    assert_eq!(history_a[1].content, "A: zweite Notiz");
    assert_eq!(history_a[1].edited_by, revision_a2.edited_by);

    let history_b = store
        .list_note_revisions(NoteTarget::Server(server_b.id))
        .await
        .unwrap();
    assert_eq!(
        history_b.len(),
        1,
        "server-b darf server-as Revisionen nicht sehen"
    );
    assert_eq!(history_b[0].content, "B: einzige Notiz");

    let history_unrelated = store
        .list_note_revisions(NoteTarget::Server(ServerId::new()))
        .await
        .unwrap();
    assert!(
        history_unrelated.is_empty(),
        "ein Server ganz ohne Revisionen liefert eine leere Liste, keinen Fehler"
    );
}

/// Klärt eine plausible Verwechslung, die genau wie der gemeldete Bug
/// aussehen kann, aber keiner ist: `preview_effective_notes`
/// (`effective_notes()`) fasst Gruppen- **und** Server-Notizen zusammen,
/// während das Notiz-Textfeld/die Historie im Server-Formular bewusst nur
/// auf den Server selbst scopen (`NoteTarget::Server`). Hat nur die
/// **Gruppe** je eine Notiz-Revision bekommen, zeigt die Kontext-Vorschau
/// trotzdem Inhalt (den der Gruppe), während das Server-Notizfeld und
/// dessen Historie korrekterweise leer bleiben — kein Bug, sondern
/// beabsichtigte Scope-Trennung (Spec 0003, Abschnitt 5.1/5.3).
#[tokio::test]
async fn test_group_only_notes_leave_server_scoped_notes_and_history_empty() {
    let store = in_memory_store().await;

    let group = make_group("Team A", None);
    store.create_group(&group).await.unwrap();
    let server = make_server("web-01", Some(group.id), vec![]);
    store.create_server(&server).await.unwrap();

    let group_revision = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Group(group.id),
        content: "Gruppen-weiter Kontext".to_string(),
        edited_by: NoteEditor::User,
        created_at: Utc::now(),
    };
    store.record_note_revision(&group_revision).await.unwrap();

    // Server selbst hat nie eine eigene Notiz-Revision bekommen.
    let server_history = store
        .list_note_revisions(NoteTarget::Server(server.id))
        .await
        .unwrap();
    assert!(server_history.is_empty());

    let fetched_server = store.get_server(&server.id).await.unwrap();
    assert_eq!(fetched_server.notes, "");

    // Trotzdem liefert effective_notes() (Kontext-Vorschau) sichtbaren
    // Inhalt — geerbt von der Gruppe, nicht vom Server.
    let effective = ssh_manager_core::profiles::effective_notes(&fetched_server, &store)
        .await
        .unwrap();
    assert!(effective.contains("Gruppen-weiter Kontext"));
}

/// Spec Abschnitt 6, Testfall 6: "Migrationen sind idempotent: zweimaliges
/// Ausführen von connect() auf derselben DB-Datei bricht nicht". Braucht
/// (anders als die übrigen Tests) eine echte Datei statt `:memory:` — zwei
/// separate `:memory:`-Verbindungen wären ohnehin zwei unabhängige, leere
/// Datenbanken und würden die Idempotenz-Frage gar nicht stellen.
#[tokio::test]
async fn test_migrations_are_idempotent_for_same_db_file() {
    let dir = tempfile::tempdir().expect("temp dir sollte anlegbar sein");
    let db_path = dir.path().join("idempotent-test.db");

    let store1 = SqliteProfileStore::connect(&db_path)
        .await
        .expect("erster connect() sollte klappen");
    let group = make_group("persistiert über einen Reconnect hinweg", None);
    store1.create_group(&group).await.unwrap();
    // Explizit schließen statt nur droppen, damit die Datei-Sperre sicher
    // freigegeben ist, bevor der zweite `connect()` versucht, sie zu öffnen
    // (vermeidet einen flaky "database is locked"-Fehler durch asynchrones
    // Aufräumen im Hintergrund).
    store1.pool.close().await;

    let store2 = SqliteProfileStore::connect(&db_path)
        .await
        .expect("zweiter connect() auf derselben Datei darf nicht brechen");
    let fetched = store2.get_group(&group.id).await.unwrap();
    assert_eq!(fetched, group);
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0004): `foreign_
/// keys` ist eine PRO-VERBINDUNG-Pragma, kein Pool-weiter Zustand — dieser
/// Test verankert das explizit, statt sich nur auf die indirekte
/// Beobachtung über `ON DELETE CASCADE`/`SET NULL`-Verhalten in den
/// anderen Tests zu verlassen (die würden bei deaktivierten Foreign Keys
/// zwar ebenfalls fehlschlagen, aber ohne den eigentlichen Grund zu
/// benennen).
#[tokio::test]
async fn test_foreign_keys_pragma_is_enabled_on_the_connection() {
    let store = in_memory_store().await;

    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&store.pool)
        .await
        .expect("PRAGMA foreign_keys sollte lesbar sein");

    assert_eq!(
        enabled, 1,
        "foreign_keys muss auf jeder Verbindung aktiv sein — sonst wirken \
         ON DELETE CASCADE/SET NULL aus dem Schema (Spec 0004 Abschnitt 4) \
         still nicht mehr"
    );
}
