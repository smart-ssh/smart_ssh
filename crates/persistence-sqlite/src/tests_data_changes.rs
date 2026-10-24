//! Issue #672: the coalesced "data changed" notification fires after
//! committed changes to the user data set, and only then.

use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use secrecy::SecretString;
use sqlx::sqlite::SqliteConnectOptions;
use uuid::Uuid;

use ssh_manager_core::ai::{ProviderId, ProviderType};
use ssh_manager_core::data_change::{DataChangeNotifier, DataKind};
use ssh_manager_core::filter::{Pattern, RuleAction, RuleId, Scope};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, CredentialStore, Group, GroupId, NoteEditor, NoteRevision,
    NoteTarget, PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::{AiProviderConfig, SqliteProfileStore, StoredRule};

const WINDOW: Duration = Duration::from_millis(80);
const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(400);

async fn store() -> (SqliteProfileStore, Receiver<BTreeSet<DataKind>>) {
    let options = SqliteConnectOptions::new().filename(":memory:");
    let mut store = SqliteProfileStore::connect_with(options).await.unwrap();
    store.data_changes = DataChangeNotifier::with_window(WINDOW);
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    store.data_changes().subscribe(Arc::new(move |kinds| {
        tx.lock().unwrap().send(kinds.clone()).unwrap();
    }));
    (store, rx)
}

fn got(rx: &Receiver<BTreeSet<DataKind>>) -> BTreeSet<DataKind> {
    rx.recv_timeout(WAIT).expect("a notification")
}

fn silent(rx: &Receiver<BTreeSet<DataKind>>) {
    assert!(rx.recv_timeout(QUIET).is_err(), "unexpected notification");
}

fn kinds(list: &[DataKind]) -> BTreeSet<DataKind> {
    list.iter().copied().collect()
}

fn group() -> Group {
    let now = Utc::now();
    Group {
        id: GroupId::new(),
        name: "g".into(),
        parent_id: None,
        notes: String::new(),
        created_at: now,
        updated_at: now,
    }
}

fn server(tags: Vec<String>) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: "s".into(),
        host: "example.invalid".into(),
        port: 22,
        username: "u".into(),
        group_id: None,
        tags,
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    }
}

fn rule(id: &str, priority: i32) -> StoredRule {
    let now = Utc::now();
    StoredRule {
        id: RuleId(id.into()),
        pattern: Pattern::Glob("ls *".into()),
        action: RuleAction::Allow,
        scope: Scope::Global,
        priority,
        created_at: now,
        updated_at: now,
    }
}

fn provider() -> AiProviderConfig {
    let now = Utc::now();
    AiProviderConfig {
        id: ProviderId::new(),
        provider_type: ProviderType::Anthropic,
        display_name: "p".into(),
        base_url: None,
        model: "m".into(),
        supports_native_tool_calling: true,
        credential_ref: CredentialRef::new("ai-provider:p".to_string()),
        is_active: false,
        extra_headers: Vec::new(),
        attestation_url: None,
        max_tokens_override: None,
        web_research_enabled: true,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn groups_servers_and_notes_notify_after_create_update_delete() {
    let (store, rx) = store().await;

    let mut g = group();
    store.create_group(&g).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Groups]));
    g.name = "renamed".into();
    store.update_group(&g).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Groups]));

    let mut s = server(vec!["t".into()]);
    store.create_server(&s).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Servers, DataKind::Tags]));
    s.name = "renamed".into();
    store.update_server(&s).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Servers, DataKind::Tags]));

    store
        .record_note_revision(&NoteRevision {
            id: Uuid::new_v4(),
            target: NoteTarget::Server(s.id),
            content: "n".into(),
            edited_by: NoteEditor::User,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Notes, DataKind::Servers]));

    store.delete_server(&s.id).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Servers, DataKind::Tags]));
    store.delete_group(&g.id).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Groups, DataKind::Servers]));
    silent(&rx);
}

#[tokio::test]
async fn rules_notify_after_create_update_delete() {
    let (store, rx) = store().await;
    let rules = store.policy_store();

    let mut r = rule("a", 1);
    rules.create(&r).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Rules]));
    r.priority = 2;
    rules.update(&r).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Rules]));
    rules.delete(&r.id).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Rules]));
    silent(&rx);
}

#[tokio::test]
async fn ai_providers_notify_after_create_activate_delete() {
    let (store, rx) = store().await;
    let providers = store.ai_provider_store();

    let p = provider();
    providers.create(&p).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::AiProviders]));
    providers.set_active(&p.id, Utc::now()).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::AiProviders]));
    let other = provider();
    providers.create(&other).await.unwrap();
    got(&rx);
    providers.delete(&other.id).await.unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::AiProviders]));
}

#[tokio::test(flavor = "multi_thread")]
async fn secrets_notify_after_set_and_delete() {
    let (store, rx) = store().await;
    let secrets = store.credential_store(tokio::runtime::Handle::current());
    let r = CredentialRef::new("server:1:password".to_string());

    secrets
        .set(&r, SecretString::from("x".to_string()))
        .unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Secrets]));
    secrets.delete(&r).unwrap();
    assert_eq!(got(&rx), kinds(&[DataKind::Secrets]));
}

#[tokio::test]
async fn ten_writes_within_the_window_yield_one_union_notification() {
    let (store, rx) = store().await;
    let rules = store.policy_store();
    for i in 0..5 {
        rules.create(&rule(&format!("r{i}"), i)).await.unwrap();
        store.create_group(&group()).await.unwrap();
    }
    assert_eq!(got(&rx), kinds(&[DataKind::Rules, DataKind::Groups]));
    silent(&rx);
}

#[tokio::test]
async fn a_rolled_back_write_emits_nothing() {
    let (store, rx) = store().await;
    let rules = store.policy_store();
    rules.create(&rule("a", 10)).await.unwrap();
    rules.create(&rule("b", 3)).await.unwrap();
    got(&rx);
    sqlx::query(
        "CREATE TRIGGER fail_b BEFORE UPDATE ON filter_rules WHEN OLD.id = 'b' \
         BEGIN SELECT RAISE(ABORT, 'simulated failure'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    assert!(rules
        .swap_priorities(&RuleId("a".into()), &RuleId("b".into()), Utc::now())
        .await
        .is_err());
    silent(&rx);

    // A server write whose tag insert fails rolls back as well.
    sqlx::query(
        "CREATE TRIGGER fail_tag BEFORE INSERT ON server_tags \
         BEGIN SELECT RAISE(ABORT, 'simulated failure'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    assert!(store
        .create_server(&server(vec!["t".into()]))
        .await
        .is_err());
    silent(&rx);
}

#[tokio::test]
async fn chat_messages_emit_nothing() {
    use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
    let (store, rx) = store().await;
    let s = server(Vec::new());
    store.create_server(&s).await.unwrap();
    got(&rx);

    let chats = store.chat_session_store();
    let session = chats.create_session(&s.id, None).await.unwrap();
    chats
        .append_message(
            session,
            &ChatMessage {
                role: Role::User,
                content: MessageContent::Text("hello".into()),
            },
        )
        .await
        .unwrap();
    chats.mark_ended(session).await.unwrap();
    silent(&rx);
}
