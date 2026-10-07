//! Issue #15: die Release-Kette. Jede eingecheckte Release-Fixture
//! (`tests/fixtures/releases/`, vom veröffentlichten Build selbst
//! geschrieben) wird **Migration für Migration** auf diesen Build gehoben;
//! nach jedem Schritt prüft der Test, dass keine Tabelle, Spalte oder Zeile,
//! die die Datei vorher enthielt, fehlt oder sich verändert hat. Am Ende
//! wird die Datei über `SqliteProfileStore::connect_encrypted` geöffnet —
//! den Aufruf des Startablaufs — und die Daten des Release über die Stores
//! zurückgelesen.
//!
//! Die Reihenfolge folgt einem echten Update: Eine Klartext-Datei wird erst
//! nach SQLCipher umgewandelt (`convert_plaintext_database`, Spec 0101 A6)
//! und dann migriert — so wie `app_logic::database_startup` es tut.
//!
//! Wie die Fixture eines neuen Release hinzukommt:
//! `tests/fixtures/releases/README.md`.

use std::sync::Arc;

use ssh_manager_core::ai::{MessageContent, Role};
use ssh_manager_core::audit::LedgerEntryContent;
use ssh_manager_core::crypto::{ChaCha20Poly1305Cipher, ContentCipher, DatabaseKey};
use ssh_manager_core::filter::{Pattern, RuleAction, Scope};
use ssh_manager_core::profiles::{AuthMethod, NoteTarget, PostIngestPolicy, ProfileStore};

use crate::test_support::{
    align_migration_checksums, applied_migrations, max_known_migration_version, migrate_up_to,
    snapshot_database, FixtureEncryption, ReleaseFixture, RELEASE_FIXTURES,
    RELEASE_FIXTURE_ROOT_KEY,
};
use crate::SqliteProfileStore;
use crate::{convert_plaintext_database, detect_database_file_state, DatabaseFileState};

/// Die Daten, die jede Release-Fixture trägt (s. Generator neben jeder
/// Fixture). Zeilen eines späteren Release bekommen eigene Marker; diese
/// hier schreibt schon die älteste Fixture, sie stehen also in jeder.
mod marker {
    pub const GROUP_NOTE: &str = "group-note-r052";
    pub const SERVER_A_HOST: &str = "host-a-r052.example";
    pub const SERVER_A_USER: &str = "user-a-r052";
    pub const SERVER_A_NOTE: &str = "server-note-r052";
    pub const SERVER_A_SFTP: &str = "/usr/lib/sftp-r052";
    pub const SERVER_B_HOST: &str = "host-b-r052.example";
    pub const PROVIDER_HEADER: &str = "Header-r052";
    pub const PROVIDER_MODEL: &str = "model-r052";
    pub const CHAT_USER: &str = "chat-user-r052";
    pub const CHAT_ASSISTANT: &str = "chat-assistant-r052";
    pub const CHAT_TITLE: &str = "title-r052";
    pub const CHAT_SUMMARY: &str = "summary-r052";
    pub const PROMPT_1: &str = "prompt-r052-1";
    pub const PROMPT_2: &str = "prompt-r052-2";
    pub const LEDGER_COMMAND: &str = "uptime-r052";
}

fn fixture_key() -> DatabaseKey {
    DatabaseKey::from_root_key(&RELEASE_FIXTURE_ROOT_KEY)
}

fn fixture_cipher() -> Arc<dyn ContentCipher> {
    Arc::new(ChaCha20Poly1305Cipher::new(&RELEASE_FIXTURE_ROOT_KEY))
}

/// Das Verzeichnis selbst: Jede eingetragene Fixture existiert, ist, was
/// sie behauptet (Klartext oder nicht, ihre Schemaversion), und die Liste
/// ist nach Release geordnet. Ohne diese Prüfung würde eine Fixture mit
/// falsch eingetragener Schemaversion still Migrationsschritte
/// überspringen.
#[tokio::test(flavor = "multi_thread")]
async fn test_release_fixtures_are_registered_consistently() {
    assert!(
        !RELEASE_FIXTURES.is_empty(),
        "at least the current release needs a fixture"
    );
    let mut previous_schema = 0;
    for fixture in RELEASE_FIXTURES {
        let path = fixture.path();
        assert!(
            path.exists(),
            "fixture {} is missing: {path:?}",
            fixture.release
        );
        assert!(
            fixture.schema_version >= previous_schema,
            "RELEASE_FIXTURES must be ordered oldest first"
        );
        assert!(
            fixture.schema_version <= max_known_migration_version(),
            "fixture {} claims a schema this build does not know",
            fixture.release
        );
        previous_schema = fixture.schema_version;

        let header = std::fs::read(&path).expect("fixture is readable");
        let is_plaintext = header.starts_with(crate::SQLITE_PLAINTEXT_HEADER);
        assert_eq!(
            is_plaintext,
            fixture.encryption == FixtureEncryption::Plaintext,
            "fixture {}: encryption does not match the file",
            fixture.release
        );

        let dir = tempfile::tempdir().expect("temp dir");
        let copy = dir.path().join(fixture.file_name);
        std::fs::copy(&path, &copy).expect("fixture can be copied");
        let key = (fixture.encryption == FixtureEncryption::Sqlcipher).then(fixture_key);
        let applied = applied_migrations(&copy, key.as_ref()).await;
        let expected: Vec<i64> = (1..=fixture.schema_version).collect();
        assert_eq!(
            applied, expected,
            "fixture {} must carry exactly the migrations 1..={}",
            fixture.release, fixture.schema_version
        );
    }
}

/// Die Kette selbst, für jedes eingetragene Release.
#[tokio::test(flavor = "multi_thread")]
async fn test_every_release_fixture_upgrades_step_by_step_without_losing_data() {
    for fixture in RELEASE_FIXTURES {
        upgrade_step_by_step(fixture).await;
    }
}

async fn upgrade_step_by_step(fixture: &ReleaseFixture) {
    let release = fixture.release;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("smart-ssh.db");
    std::fs::copy(fixture.path(), &path).expect("fixture can be copied");
    let key = fixture_key();

    // --- Ausgangspunkt: die Datei, wie das Release sie hinterlassen hat.
    let file_key = match fixture.encryption {
        FixtureEncryption::Plaintext => None,
        FixtureEncryption::Sqlcipher => Some(&key),
    };
    align_migration_checksums(&path, file_key).await;
    let mut previous = snapshot_database(&path, file_key).await;
    assert_release_rows_present(&previous, release);

    // --- Eine Klartext-Datei wird zuerst umgewandelt, wie beim Start.
    if fixture.encryption == FixtureEncryption::Plaintext {
        convert_plaintext_database(&path, &key)
            .await
            .unwrap_or_else(|err| panic!("{release}: conversion failed: {err:?}"));
        assert_eq!(
            detect_database_file_state(&path).expect("file state"),
            DatabaseFileState::Other,
            "{release}: the converted file must no longer be plaintext"
        );
        let converted = snapshot_database(&path, Some(&key)).await;
        assert_eq!(
            previous, converted,
            "{release}: the conversion must keep schema and data exactly"
        );
        previous = converted;
    }

    // --- Eine Migration nach der anderen, bis zu diesem Build.
    let steps: Vec<i64> = crate::test_support::migration_files()
        .into_iter()
        .map(|(version, _)| version)
        .filter(|version| *version > fixture.schema_version)
        .collect();
    assert_eq!(
        steps.last().copied().unwrap_or(fixture.schema_version),
        max_known_migration_version(),
        "{release}: the steps must end at this build's newest migration"
    );
    for version in steps {
        let step = format!("{release} -> migration {version}");
        migrate_up_to(&path, Some(&key), version).await;
        let applied = applied_migrations(&path, Some(&key)).await;
        assert_eq!(
            applied.last().copied(),
            Some(version),
            "{step}: not applied"
        );
        let current = snapshot_database(&path, Some(&key)).await;
        previous.assert_preserved_in(&current, &step);
        assert_release_rows_present(&current, &step);
        previous = current;
    }

    // --- Das Öffnen des Startablaufs: nichts mehr zu migrieren, alles über
    // die Stores lesbar.
    let store = SqliteProfileStore::connect_encrypted(&path, &key)
        .await
        .unwrap_or_else(|err| panic!("{release}: the upgraded file does not open: {err}"));
    assert_release_data_readable(&store, release).await;
    // Vor dem Abbild schließen: `connect_encrypted` hält die einzige
    // Verbindung, und das Abbild soll sehen, was das Öffnen auf der Platte
    // hinterlassen hat.
    store.close().await;
    let after_open = snapshot_database(&path, Some(&key)).await;
    assert_eq!(
        previous, after_open,
        "{release}: opening the fully migrated file must change nothing"
    );
}

/// Zeilenzahlen der Release-Daten, geprüft am rohen Abbild — so geht es
/// auf jedem Zwischenschema, auf dem die Stores dieses Builds noch nicht
/// lesen können.
fn assert_release_rows_present(snapshot: &crate::test_support::DatabaseSnapshot, step: &str) {
    for (table, at_least) in [
        ("groups", 2),
        ("servers", 2),
        ("server_tags", 2),
        ("note_revisions", 2),
        ("filter_rules", 3),
        ("ai_provider_configs", 1),
        ("chat_sessions", 1),
        ("chat_messages", 2),
        ("prompt_history", 2),
        ("ledger_entries", 2),
    ] {
        assert!(
            snapshot.row_count(table) >= at_least,
            "{step}: table {table} has {} rows, expected at least {at_least}",
            snapshot.row_count(table)
        );
    }
    let hosts: Vec<&String> = snapshot.tables["servers"]
        .rows
        .iter()
        .filter_map(|row| row.get("host"))
        .collect();
    for host in [marker::SERVER_A_HOST, marker::SERVER_B_HOST] {
        assert!(
            hosts.iter().any(|h| h.contains(host)),
            "{step}: server {host} missing: {hosts:?}"
        );
    }
}

/// Die Daten des Release, gelesen über die Stores dieses Builds — Feld für
/// Feld, auch der feldweise verschlüsselte Chat, Prompt-Historie und
/// Ledger.
async fn assert_release_data_readable(store: &SqliteProfileStore, release: &str) {
    let groups = store.list_groups().await.expect("groups readable");
    let parent = groups
        .iter()
        .find(|g| g.notes == marker::GROUP_NOTE)
        .unwrap_or_else(|| panic!("{release}: parent group missing: {groups:?}"));
    let child = groups
        .iter()
        .find(|g| g.parent_id == Some(parent.id))
        .unwrap_or_else(|| panic!("{release}: child group missing: {groups:?}"));

    let servers = store.list_servers().await.expect("servers readable");
    let a = servers
        .iter()
        .find(|s| s.host == marker::SERVER_A_HOST)
        .unwrap_or_else(|| panic!("{release}: server A missing"));
    assert_eq!(a.port, 2222);
    assert_eq!(a.username, marker::SERVER_A_USER);
    assert_eq!(a.group_id, Some(child.id));
    let mut tags = a.tags.clone();
    tags.sort();
    assert_eq!(tags, vec!["db".to_string(), "prod-r052".to_string()]);
    assert_eq!(a.notes, marker::SERVER_A_NOTE);
    assert_eq!(a.post_ingest_policy, PostIngestPolicy::Strict);
    assert!(a.ai_injection_check_enabled);
    assert_eq!(a.sftp_server_path.as_deref(), Some(marker::SERVER_A_SFTP));
    assert_eq!(
        a.start_directory, None,
        "{release}: a column added later must start empty"
    );
    match &a.auth {
        AuthMethod::Password { credential_ref } => {
            assert_eq!(
                credential_ref.as_str(),
                format!("server:{}:password", a.id.0)
            );
        }
        other => panic!("{release}: server A auth changed: {other:?}"),
    }
    let b = servers
        .iter()
        .find(|s| s.host == marker::SERVER_B_HOST)
        .unwrap_or_else(|| panic!("{release}: server B missing"));
    assert_eq!(b.jump_host, Some(a.id));
    assert_eq!(b.post_ingest_policy, PostIngestPolicy::Balanced);
    assert!(
        matches!(
            &b.auth,
            AuthMethod::PrivateKey {
                passphrase_ref: Some(_),
                ..
            }
        ),
        "{release}: server B auth changed: {:?}",
        b.auth
    );

    let server_notes = store
        .list_note_revisions(NoteTarget::Server(a.id))
        .await
        .expect("note revisions readable");
    assert_eq!(server_notes.len(), 1);
    assert_eq!(server_notes[0].content, marker::SERVER_A_NOTE);
    let group_notes = store
        .list_note_revisions(NoteTarget::Group(parent.id))
        .await
        .expect("note revisions readable");
    assert_eq!(group_notes.len(), 1);
    assert_eq!(group_notes[0].content, marker::GROUP_NOTE);

    let rules = store
        .policy_store()
        .list_all()
        .await
        .expect("rules readable");
    assert_eq!(rules.len(), 3, "{release}: filter rules lost");
    let deny = rules
        .iter()
        .find(|r| r.id.0 == "rule-r052-deny")
        .expect("deny rule present");
    assert_eq!(deny.action, RuleAction::Deny);
    assert_eq!(deny.scope, Scope::Global);
    assert_eq!(deny.priority, 100);
    assert_eq!(deny.pattern, Pattern::Regex(r"rm\s+-rf\s+/".to_string()));
    let confirm = rules
        .iter()
        .find(|r| r.id.0 == "rule-r052-confirm")
        .expect("confirm rule present");
    assert_eq!(confirm.action, RuleAction::Confirm);
    assert_eq!(confirm.scope, Scope::Tag("prod-r052".to_string()));
    let allow = rules
        .iter()
        .find(|r| r.id.0 == "rule-r052-allow")
        .expect("allow rule present");
    assert_eq!(allow.action, RuleAction::Allow);
    assert_eq!(allow.scope, Scope::Server(a.id));

    let providers = store
        .ai_provider_store()
        .list()
        .await
        .expect("providers readable");
    assert_eq!(providers.len(), 1);
    let provider = &providers[0];
    assert_eq!(provider.model, marker::PROVIDER_MODEL);
    assert!(provider.is_active);
    assert_eq!(provider.max_tokens_override, Some(4096));
    assert!(provider
        .extra_headers
        .iter()
        .any(|(_, v)| v == marker::PROVIDER_HEADER));

    let chat = store.chat_session_store(fixture_cipher());
    let sessions = chat
        .list_sessions_for_server(&a.id)
        .await
        .expect("sessions readable");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title.as_deref(), Some(marker::CHAT_TITLE));
    let messages = chat
        .load_session(sessions[0].id)
        .await
        .expect("chat content decrypts with the release's key");
    let texts: Vec<(Role, String)> = messages
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::Text(text) => Some((m.role, text.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            (Role::User, marker::CHAT_USER.to_string()),
            (Role::Assistant, marker::CHAT_ASSISTANT.to_string()),
        ]
    );
    let summary = chat
        .load_summary(sessions[0].id)
        .await
        .expect("summary readable");
    assert_eq!(summary, Some((marker::CHAT_SUMMARY.to_string(), 1)));

    let history = store
        .prompt_history_store(fixture_cipher())
        .list(&a.id)
        .await
        .expect("prompt history readable");
    let mut history_sorted = history.clone();
    history_sorted.sort();
    assert_eq!(
        history_sorted,
        vec![marker::PROMPT_1.to_string(), marker::PROMPT_2.to_string()]
    );

    let ledger = store
        .ledger_store(fixture_cipher())
        .load_entries(sessions[0].id)
        .await
        .expect("ledger readable");
    assert_eq!(ledger.len(), 2);
    assert!(ledger.iter().any(|e| matches!(
        &e.content,
        LedgerEntryContent::CommandProposed { command } if command == marker::LEDGER_COMMAND
    )));
}
