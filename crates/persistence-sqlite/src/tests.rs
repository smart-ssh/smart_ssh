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
        ai_injection_check_enabled: false,
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

/// Spec 0047, Fund A3: `test_migrations_are_idempotent_for_same_db_file`
/// oben belegt nur, dass ein zweiter `connect()` auf einer bereits
/// VOLLSTÄNDIG migrierten Datei nicht bricht — nicht, dass ein echter
/// N→N+1-Durchlauf auf einer Datei mit einem ÄLTEREN Schema-Stand UND
/// vorhandenen Daten diese Daten erhält. Genau das simuliert dieser Test:
/// eine echte DB-Datei wird zunächst nur mit den Migrationen bis
/// einschließlich `0008_chat_sessions.sql` aufgebaut (der Stand, auf dem
/// `chat_messages.content`/`prompt_history.content` noch reines TEXT sind,
/// nicht die seit `0009`/`0010` genutzte BLOB-Spalte) und mit Testdaten in
/// jeder betroffenen Tabelle befüllt — genau die beiden riskanten
/// Migrationen in diesem Projekt: SQLite kennt kein `ALTER COLUMN ...
/// TYPE`, `0009`/`0010` nutzen daher das Tabelle-neu-anlegen-und-Zeilen-
/// kopieren-Muster, das bei einer falschen Spaltenliste stillschweigend
/// Daten verlieren könnte. Alle übrigen Migrationen (0002-0008) sind reine
/// `CREATE TABLE`/`ALTER TABLE ADD COLUMN`, strukturell nicht
/// datenverlust-fähig — trotzdem belegt dieser Test seit dem
/// Spec-0047-Review-Pass mindestens eine Zeile in jeder Tabelle mit
/// eigenem Anwendungsdaten-Charakter, nicht nur `groups`/`servers`/
/// `server_tags`/`note_revisions`: die Spec nennt "Regeln" ausdrücklich,
/// `filter_rules` (0003) ist zudem die sicherheitsrelevanteste Tabelle im
/// gesamten Schema — eine künftige create-copy-drop-rename-Migration
/// darauf, die eine Spalte vergisst, verlöre sonst still `deny`-Regeln,
/// ohne dass dieser Test es bemerkte. `ai_provider_configs` (0002) ergänzt
/// dieselbe Absicherung für die Provider-Konfiguration (nur die
/// `credential_ref`-Referenz, nie ein Klartext-Secret).
///
/// Danach öffnet ein regulärer `SqliteProfileStore::connect()` (der volle,
/// zur Compile-Zeit eingebettete `sqlx::migrate!()`-Satz) dieselbe Datei
/// erneut — das wendet `0009`/`0010` auf die bereits vorhandenen Daten an
/// — und der Test verifiziert, dass jede zuvor geschriebene Zeile
/// unverändert lesbar ist.
#[tokio::test]
async fn test_migration_from_earlier_schema_with_real_data_preserves_all_rows() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    let dir = tempfile::tempdir().expect("temp dir sollte anlegbar sein");
    let db_path = dir.path().join("upgrade-test.db");

    // 1. Nur die Migrationen bis "0008" in ein temporäres Verzeichnis
    // kopieren (Dateiname-Präfix reicht als Sortier-/Filterkriterium, alle
    // vierstellig nullgepolstert) — ein separater `sqlx::migrate::Migrator`
    // liest zur Laufzeit von Disk, anders als das `sqlx::migrate!()`-Makro
    // in `store.rs`, das den kompletten, aktuellen Satz zur Compile-Zeit
    // einbettet und sich daher nicht auf einen Teilstand einschränken
    // lässt.
    let real_migrations_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let old_migrations_dir = tempfile::tempdir().expect("temp dir sollte anlegbar sein");
    for entry in std::fs::read_dir(&real_migrations_dir).expect("migrations/ sollte lesbar sein") {
        let entry = entry.expect("Verzeichniseintrag sollte lesbar sein");
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.len() >= 4 && &name[..4] <= "0008" {
            std::fs::copy(entry.path(), old_migrations_dir.path().join(&*name))
                .expect("Migrationsdatei sollte kopierbar sein");
        }
    }

    let old_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .foreign_keys(true);
    let old_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(old_options)
        .await
        .expect("Verbindung zur alten Schema-Stufe sollte klappen");
    sqlx::migrate::Migrator::new(old_migrations_dir.path())
        .await
        .expect("Teil-Migrator sollte sich aus den kopierten Dateien aufbauen lassen")
        .run(&old_pool)
        .await
        .expect("Migrationen bis 0008 sollten sauber durchlaufen");

    // 2. Testdaten auf diesem älteren Schema-Stand anlegen — `groups`/
    // `servers`/`server_tags`/`note_revisions` über die normale
    // `ProfileStore`-API (das Schema für alle vier existiert bereits seit
    // 0001/0003), `chat_sessions`/`chat_messages`/`prompt_history` über
    // rohes SQL mit reinem TEXT-Inhalt — genau das Format, das eine echte
    // Installation auf diesem Schema-Stand tatsächlich geschrieben hätte
    // (die BLOB-Spalte gibt es dort noch nicht).
    let old_store = SqliteProfileStore {
        pool: old_pool.clone(),
    };
    let group = make_group("Produktion", None);
    old_store.create_group(&group).await.unwrap();
    let server = make_server("web-01", Some(group.id), vec!["prod".to_string()]);
    old_store.create_server(&server).await.unwrap();
    let note = NoteRevision {
        id: Uuid::new_v4(),
        target: NoteTarget::Server(server.id),
        content: "Notiz vor dem Upgrade".to_string(),
        edited_by: NoteEditor::User,
        created_at: Utc::now(),
    };
    old_store.record_note_revision(&note).await.unwrap();

    // Spec-Reviewer-Fund (Spec 0047, Review dieses Schritts): die Spec
    // nennt "Regeln" (`filter_rules`) ausdrücklich als zu schützende Daten
    // — bislang war nur über die vier oben genannten Tabellen abgesichert,
    // dass `0001`-`0008` (reine `CREATE`/`ADD COLUMN`) strukturell nicht
    // datenverlust-fähig sind, ohne `filter_rules`/`ai_provider_configs`
    // selbst je in einer Zeile zu belegen. Beide Tabellen existieren
    // bereits seit `0002`/`0003`, also lange vor der Schema-Grenze dieses
    // Tests — rohes SQL wie bei `chat_sessions` oben statt der
    // `SqlitePolicyStore`/`SqliteAiProviderStore`-APIs, da die Testdaten
    // hier absichtlich exakt das Zeilenformat einer echten Installation
    // auf diesem älteren Schema-Stand abbilden sollen. `credential_ref`
    // ist bewusst nur eine Referenz-Zeichenkette, kein Klartext-Secret
    // (das echte Secret läge im Keychain, nicht in dieser DB).
    let rule_id = "test-rule-deny-rm-rf";
    sqlx::query(
        "INSERT INTO filter_rules \
         (id, pattern_type, pattern_value, action, scope_type, scope_value, priority, \
          created_at, updated_at) \
         VALUES (?, 'exact', 'rm -rf /', 'deny', 'global', NULL, 100, ?, ?)",
    )
    .bind(rule_id)
    .bind(Utc::now().to_rfc3339())
    .bind(Utc::now().to_rfc3339())
    .execute(&old_pool)
    .await
    .unwrap();

    let provider_id = "test-provider-anthropic";
    let provider_credential_ref = "app:ai_provider:test-provider-anthropic:api_key";
    sqlx::query(
        "INSERT INTO ai_provider_configs \
         (id, provider_type, display_name, base_url, model, \
          supports_native_tool_calling, credential_ref, is_active, created_at, updated_at) \
         VALUES (?, 'anthropic', 'Prod Claude', NULL, 'claude-sonnet-5', 1, ?, 0, ?, ?)",
    )
    .bind(provider_id)
    .bind(provider_credential_ref)
    .bind(Utc::now().to_rfc3339())
    .bind(Utc::now().to_rfc3339())
    .execute(&old_pool)
    .await
    .unwrap();

    let chat_session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO chat_sessions (id, server_id, title, started_at, ended_at, ai_provider_id) \
         VALUES (?, ?, ?, ?, NULL, NULL)",
    )
    .bind(chat_session_id.to_string())
    .bind(server.id.0.to_string())
    .bind("Alte Sitzung")
    .bind(Utc::now().to_rfc3339())
    .execute(&old_pool)
    .await
    .unwrap();

    let chat_message_id = Uuid::new_v4();
    let chat_message_plaintext = r#"{"Text":"Klartext vor der BLOB-Migration"}"#;
    sqlx::query(
        "INSERT INTO chat_messages (id, session_id, role, content_type, content, sequence, created_at) \
         VALUES (?, ?, 'user', 'text', ?, 1, ?)",
    )
    .bind(chat_message_id.to_string())
    .bind(chat_session_id.to_string())
    .bind(chat_message_plaintext)
    .bind(Utc::now().to_rfc3339())
    .execute(&old_pool)
    .await
    .unwrap();

    let prompt_history_id = Uuid::new_v4();
    let prompt_history_plaintext = "ls -la /var/log";
    sqlx::query(
        "INSERT INTO prompt_history (id, server_id, content, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(prompt_history_id.to_string())
    .bind(server.id.0.to_string())
    .bind(prompt_history_plaintext)
    .bind(Utc::now().to_rfc3339())
    .execute(&old_pool)
    .await
    .unwrap();

    old_pool.close().await;

    // 3. Regulärer `connect()` mit dem vollen, aktuellen Migrationssatz —
    // wendet 0009/0010 auf die vorhandenen Zeilen an.
    let upgraded = SqliteProfileStore::connect(&db_path)
        .await
        .expect("der Aufstieg auf den aktuellen Schema-Stand darf nicht fehlschlagen");

    // 4. Alles muss unverändert da sein.
    let fetched_group = upgraded.get_group(&group.id).await.unwrap();
    assert_eq!(fetched_group, group);

    let fetched_server = upgraded.get_server(&server.id).await.unwrap();
    assert_eq!(fetched_server.name, server.name);
    assert_eq!(fetched_server.tags, vec!["prod".to_string()]);
    assert_eq!(fetched_server.notes, "Notiz vor dem Upgrade");

    let revisions = upgraded
        .list_note_revisions(NoteTarget::Server(server.id))
        .await
        .unwrap();
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].content, "Notiz vor dem Upgrade");

    let session_title: String = sqlx::query_scalar("SELECT title FROM chat_sessions WHERE id = ?")
        .bind(chat_session_id.to_string())
        .fetch_one(&upgraded.pool)
        .await
        .unwrap();
    assert_eq!(session_title, "Alte Sitzung");

    // 0009 stellt NUR den Spaltentyp um (TEXT -> BLOB), kopiert die Bytes
    // unverändert — der ehemalige Klartext muss byteidentisch in der neuen
    // BLOB-Spalte stehen (die eigentliche Verschlüsselung ist eine
    // separate, bereits eigenständig getestete Anwendungs-Routine, s.
    // `prompt_history_store::migrate_legacy_plaintext_content`, läuft beim
    // App-Start NACH dieser SQL-Migration).
    let migrated_message_content: Vec<u8> =
        sqlx::query_scalar("SELECT content FROM chat_messages WHERE id = ?")
            .bind(chat_message_id.to_string())
            .fetch_one(&upgraded.pool)
            .await
            .unwrap();
    assert_eq!(migrated_message_content, chat_message_plaintext.as_bytes());

    let migrated_prompt_content: Vec<u8> =
        sqlx::query_scalar("SELECT content FROM prompt_history WHERE id = ?")
            .bind(prompt_history_id.to_string())
            .fetch_one(&upgraded.pool)
            .await
            .unwrap();
    assert_eq!(migrated_prompt_content, prompt_history_plaintext.as_bytes());

    // Spec-Reviewer-Fund (Spec 0047, Review dieses Schritts): die
    // sicherheitsrelevanteste Tabelle — eine `deny`-Regel darf einen
    // Versions-Sprung nicht stillschweigend verlieren.
    let (fetched_action, fetched_pattern): (String, String) =
        sqlx::query_as("SELECT action, pattern_value FROM filter_rules WHERE id = ?")
            .bind(rule_id)
            .fetch_one(&upgraded.pool)
            .await
            .unwrap();
    assert_eq!(fetched_action, "deny");
    assert_eq!(fetched_pattern, "rm -rf /");

    let (fetched_model, fetched_credential_ref): (String, String) =
        sqlx::query_as("SELECT model, credential_ref FROM ai_provider_configs WHERE id = ?")
            .bind(provider_id)
            .fetch_one(&upgraded.pool)
            .await
            .unwrap();
    assert_eq!(fetched_model, "claude-sonnet-5");
    assert_eq!(fetched_credential_ref, provider_credential_ref);
}
