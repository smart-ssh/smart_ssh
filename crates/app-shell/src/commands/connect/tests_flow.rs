//! Tests of `connect_session` against an injected connector.
//! Issue #259: `connect_session` gegen einen eingespeisten `Connector` — der
//! echte Verbindungsaufbau (Verbinden, Fortsetzen, Fehlerpfad beim
//! Fortsetzen) ohne Netzwerk. Echte SQLite-Stores (je Test eigene
//! Datenbankdateien), `MockRuntime`, ein Mock-Connector mit Mock-Transport.
// Testcode-Ausnahme zum `deny` — s. `lib.rs`, Modulkopf.

#![allow(clippy::unwrap_used, clippy::expect_used)]
/// Issue #395: the in-process SSH test server of `ssh-transport`, reused for
/// the real two-hop reconnect test in `connect_flow_tests`. Test-only.
#[cfg(test)]
#[allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
#[path = "../../../../ssh-transport/tests/fixtures/test_server.rs"]
mod two_hop_fixture;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use persistence_sqlite::{AiProviderConfig, SqliteProfileStore};
use ssh_manager_core::ai::{ChatMessage, MessageContent, ProviderId, ProviderType, Role};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, CredentialStore, PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::ssh::{
    CommandOutput, ConnectionTarget, HostKeyDecision, HostKeyStore, InteractiveShell,
    KeyFileReader, PtySize, SftpSession, SshTransport,
};
use ssh_transport::ConnectOutcome;
use tauri::Listener as _;
use tauri_plugin_store::StoreExt;

use app_logic::state::AppState;
use app_logic::test_connection::Connector;
use app_logic::test_support::InMemoryCredentialStore;

use super::*;
use crate::first_run_notice::test_support::{lock_async, reset, test_app};

const PROVIDER_SECRET_REF: &str = "test:ai";

/// Zeichnet auf, ob `disconnect()` aufgerufen wurde.
struct RecordingTransport {
    disconnected: Arc<AtomicBool>,
}

#[async_trait]
impl SshTransport for RecordingTransport {
    async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
        Ok(CommandOutput {
            stdout: b"Linux test 6.1 x86_64\n".to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
            truncated: false,
        })
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!("not used by connect_session")
    }
    async fn open_sftp_via_exec(
        &mut self,
        _command: &str,
    ) -> Result<Box<dyn SftpSession>, SshError> {
        unreachable!("not used by connect_session")
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        self.disconnected.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Liefert genau einmal das vorbereitete Ergebnis; `on_call` läuft vor
/// der Rückgabe (z. B. um eine Datenbank zu schließen).
struct ScriptedConnector {
    result: Mutex<Option<Result<ConnectOutcome, SshError>>>,
    calls: AtomicUsize,
    /// Hop count of the target of the latest connect.
    last_hops: AtomicUsize,
}

impl ScriptedConnector {
    /// Issue #355: queues the result for the next connect (a reconnect).
    fn push(&self, result: Result<ConnectOutcome, SshError>) {
        *self.result.lock().unwrap() = Some(result);
    }

    fn new(result: Result<ConnectOutcome, SshError>) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Some(result)),
            calls: AtomicUsize::new(0),
            last_hops: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl Connector for ScriptedConnector {
    async fn connect(
        &self,
        target: &ConnectionTarget,
        _credentials: &(dyn CredentialStore + Send + Sync),
        _key_files: &(dyn KeyFileReader + Send + Sync),
        _host_keys: Arc<dyn HostKeyStore>,
        _log: &ConnectLog,
        _limits: ssh_transport::ConnectLimits,
    ) -> Result<ConnectOutcome, SshError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.last_hops.store(target.hops.len(), Ordering::SeqCst);
        self.result
            .lock()
            .unwrap()
            .take()
            .expect("connector called more than once")
    }
}

struct NoHostKeys;
impl HostKeyStore for NoHostKeys {
    fn check(&self, _host: &str, _port: u16, key: &[u8]) -> HostKeyDecision {
        HostKeyDecision::Unknown {
            fingerprint: format!("{}", key.len()),
        }
    }
    fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
        Ok(())
    }
}

fn server() -> Server {
    let now = chrono::Utc::now();
    Server {
        id: ServerId(uuid::Uuid::new_v4()),
        name: "web-1".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "root".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::Balanced,
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    }
}

fn provider_config() -> AiProviderConfig {
    let now = chrono::Utc::now();
    AiProviderConfig {
        id: ProviderId::new(),
        display_name: "test".to_string(),
        provider_type: ProviderType::Anthropic,
        base_url: None,
        model: "claude-test".to_string(),
        credential_ref: CredentialRef::new(PROVIDER_SECRET_REF),
        is_active: true,
        supports_native_tool_calling: true,
        extra_headers: Vec::new(),
        max_tokens_override: None,
        web_research_enabled: false,
        attestation_url: None,
        created_at: now,
        updated_at: now,
    }
}

/// Server und Provider stehen in **beiden** Datenbanken: `chat_sessions`
/// referenziert `servers` und `ai_providers` per Fremdschlüssel.
struct Fixture {
    state: AppState,
    /// Hält die Datenbankdateien am Leben.
    _dir: tempfile::TempDir,
    /// Die Datenbank mit Chat und Ledger — ein Test schließt sie, um
    /// einen Ladefehler der Historie zu erzeugen.
    chat_db: SqliteProfileStore,
    server_id: ServerId,
}

async fn fixture(connector: Arc<dyn Connector>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let main_db = SqliteProfileStore::connect_plaintext(&dir.path().join("main.db"))
        .await
        .unwrap();
    let chat_db = SqliteProfileStore::connect_plaintext(&dir.path().join("chat.db"))
        .await
        .unwrap();
    let server = server();
    let provider = provider_config();
    for db in [&main_db, &chat_db] {
        db.create_server(&server).await.unwrap();
        db.ai_provider_store().create(&provider).await.unwrap();
    }
    main_db
        .ai_provider_store()
        .set_active(&provider.id, chrono::Utc::now())
        .await
        .unwrap();
    let credential_store = InMemoryCredentialStore::new()
        .with_secret(&CredentialRef::new(PROVIDER_SECRET_REF), "sk-test");
    let data_changes = main_db.data_changes();
    let state = AppState {
        sessions: app_logic::session::SessionManager::new(),
        session_extension: None,
        root_key_fingerprint: [0; 32],
        ai_provider_store: Arc::new(main_db.ai_provider_store()),
        backup_store: main_db.backup_store(),
        policy_store: main_db.policy_store(),
        prompt_history_store: main_db.prompt_history_store(),
        chat_session_store: chat_db.chat_session_store(),
        ledger_store: chat_db.ledger_store(),
        profile_store: Arc::new(main_db),
        credential_store: Arc::new(credential_store),
        key_file_reader: Arc::new(ssh_manager_core::ssh::mock::MockKeyFileReader::new()),
        keychain: credentials_keyring::KeychainAvailability::Available,
        file_host_keys: Arc::new(
            app_logic::host_key_store::FileHostKeyStore::load(dir.path().join("hk.json")).unwrap(),
        ),
        host_key_store: Arc::new(NoHostKeys),
        data_changes,
        connector,
        entitlements: Arc::new(ssh_manager_core::entitlements::FixedEntitlements(
            ssh_manager_core::entitlements::Entitlements::free(),
        )),
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        connect_attempts: Default::default(),
        pending_action_confirmations: ConfirmationRegistry::new(),
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        mcp: app_logic::state::McpState::default(),
        rate_limit_registry: ai_providers::RateLimitRegistry::new(),
        pending_ssh_config_import: std::sync::Mutex::new(None),
        outgoing_transform: None,
        chat_content: Default::default(),
        ai_usage_observers: Default::default(),
    };
    Fixture {
        state,
        _dir: dir,
        chat_db,
        server_id: server.id,
    }
}

fn acknowledged_app() -> tauri::App<tauri::test::MockRuntime> {
    let app = test_app();
    let handle = app.handle().clone();
    reset(&handle);
    handle
        .store("settings.json")
        .unwrap()
        .set("first_run_notice_acknowledged", serde_json::json!(true));
    app
}

fn connected(disconnected: &Arc<AtomicBool>) -> Result<ConnectOutcome, SshError> {
    Ok(ConnectOutcome::Connected(Box::new(RecordingTransport {
        disconnected: disconnected.clone(),
    })))
}

fn user_text(text: &str) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: MessageContent::Text(text.to_string()),
    }
}

fn status_events(app: &tauri::App<tauri::test::MockRuntime>) -> Arc<Mutex<Vec<String>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    app.handle()
        .listen("connection-status-changed", move |event| {
            sink.lock().unwrap().push(event.payload().to_string());
        });
    seen
}

#[tokio::test]
async fn test_connect_registers_session_emits_connected_and_creates_chat_session() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let events = status_events(&app);
    let disconnected = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&disconnected));
    let fx = fixture(connector.clone()).await;
    let session_id = uuid::Uuid::new_v4();

    let result = connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        session_id,
        None,
        true,
    )
    .await
    .expect("connect should succeed");

    assert_eq!(result, session_id);
    assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
    assert!(fx.state.sessions.get(session_id).is_some());
    assert!(!disconnected.load(Ordering::SeqCst));
    let sessions = fx
        .state
        .chat_session_store
        .list_sessions_for_server(&fx.server_id)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1, "one chat session row is created");
    let session = fx.state.sessions.get(session_id).unwrap();
    assert_eq!(*session.chat_session_id.lock().await, Some(sessions[0].id));
    let seen = events.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|p| p.contains(&session_id.to_string()) && p.to_lowercase().contains("connected")),
        "connected status event expected, got {seen:?}"
    );
    reset(app.handle());
}

#[tokio::test]
async fn test_connect_without_persisting_creates_no_chat_session_row() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let disconnected = Arc::new(AtomicBool::new(false));
    let fx = fixture(ScriptedConnector::new(connected(&disconnected))).await;

    connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        false,
    )
    .await
    .unwrap();

    let sessions = fx
        .state
        .chat_session_store
        .list_sessions_for_server(&fx.server_id)
        .await
        .unwrap();
    assert!(sessions.is_empty());
    reset(app.handle());
}

#[tokio::test]
async fn test_resume_loads_history_and_clears_ended_at() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let disconnected = Arc::new(AtomicBool::new(false));
    let fx = fixture(ScriptedConnector::new(connected(&disconnected))).await;
    let store = &fx.state.chat_session_store;
    let existing = store.create_session(&fx.server_id, None).await.unwrap();
    store
        .append_message(existing, &user_text("earlier question"))
        .await
        .unwrap();
    store.mark_ended(existing).await.unwrap();
    let before = store.list_sessions_for_server(&fx.server_id).await.unwrap();
    assert!(before[0].ended_at.is_some(), "precondition: session ended");

    let tab = uuid::Uuid::new_v4();
    connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        tab,
        Some(existing),
        true,
    )
    .await
    .expect("resume should succeed");

    let session = fx.state.sessions.get(tab).expect("session registered");
    assert_eq!(*session.chat_session_id.lock().await, Some(existing));
    let history = session.context.lock().await.history.clone();
    assert!(
        history.iter().any(|m| matches!(
            &m.content,
            MessageContent::Text(t) if t == "earlier question"
        )),
        "stored history is loaded into the session"
    );
    let after = store.list_sessions_for_server(&fx.server_id).await.unwrap();
    assert_eq!(after.len(), 1, "no new chat session row");
    assert!(after[0].ended_at.is_none(), "ended_at is cleared");
    assert!(!disconnected.load(Ordering::SeqCst));
    reset(app.handle());
}

#[tokio::test]
async fn test_resume_with_failing_history_load_disconnects_and_registers_nothing() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let disconnected = Arc::new(AtomicBool::new(false));
    let fx = fixture(ScriptedConnector::new(connected(&disconnected))).await;
    let existing = fx
        .state
        .chat_session_store
        .create_session(&fx.server_id, None)
        .await
        .unwrap();
    // Das Laden der Historie schlägt fehl, wenn die Chat-Datenbank weg
    // ist; Server und Provider liegen in der anderen Datenbank.
    fx.chat_db.close().await;

    let tab = uuid::Uuid::new_v4();
    let err = connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        tab,
        Some(existing),
        true,
    )
    .await
    .expect_err("resume must fail when the history cannot be loaded");

    assert!(!err.message.is_empty());
    assert!(
        disconnected.load(Ordering::SeqCst),
        "the established connection must be closed explicitly"
    );
    assert!(fx.state.sessions.get(tab).is_none());
    reset(app.handle());
}

#[tokio::test]
async fn test_connector_error_is_mapped_with_its_stable_code() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let connector = ScriptedConnector::new(Err(SshError::AuthenticationFailed));
    let fx = fixture(connector.clone()).await;
    let tab = uuid::Uuid::new_v4();

    let err = connect_session(app.handle(), &fx.state, fx.server_id, tab, None, true)
        .await
        .expect_err("connector error must fail the connect");

    assert_eq!(err.code, Some(SshError::AuthenticationFailed.code()));
    assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
    assert!(fx.state.sessions.get(tab).is_none());
    assert!(fx
        .state
        .chat_session_store
        .list_sessions_for_server(&fx.server_id)
        .await
        .unwrap()
        .is_empty());
    reset(app.handle());
}

#[tokio::test]
async fn test_unacknowledged_first_run_notice_never_reaches_the_connector() {
    let _guard = lock_async().await;
    let app = test_app();
    reset(app.handle());
    let connector = ScriptedConnector::new(Err(SshError::Timeout));
    let fx = fixture(connector.clone()).await;

    let err = connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        true,
    )
    .await
    .expect_err("gate must block");

    assert_eq!(err.code, Some("FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED"));
    assert_eq!(connector.calls.load(Ordering::SeqCst), 0);
}

// --- Issue #355 / Spec 0105: reconnect ---------------------------------

fn registry_pair() -> (
    crate::elevated_sftp::ElevatedSftpRegistry,
    app_logic::local_path_grants::LocalPathGrants,
) {
    (
        crate::elevated_sftp::ElevatedSftpRegistry::default(),
        app_logic::local_path_grants::LocalPathGrants::default(),
    )
}

/// Connects and simulates the drop the terminal actor reports.
async fn connected_then_lost(
    app: &tauri::App<tauri::test::MockRuntime>,
    fx: &Fixture,
    connector: &ScriptedConnector,
    disconnected: &Arc<AtomicBool>,
) -> (SessionId, Arc<Session>) {
    connector.push(connected(disconnected));
    let session_id = uuid::Uuid::new_v4();
    connect_session(
        app.handle(),
        &fx.state,
        fx.server_id,
        session_id,
        None,
        true,
    )
    .await
    .unwrap();
    let session = fx.state.sessions.get(session_id).unwrap();
    *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
    (session_id, session)
}

#[tokio::test]
async fn test_reconnect_keeps_session_id_and_ai_context_and_replaces_transport() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let events = status_events(&app);
    let old_closed = Arc::new(AtomicBool::new(false));
    let new_closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&old_closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &old_closed).await;
    session
        .context
        .lock()
        .await
        .history
        .push(user_text("before the drop"));
    let history_len = session.context.lock().await.history.len();
    connector.push(connected(&new_closed));
    let (elevated, grants) = registry_pair();

    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .expect("reconnect should succeed");

    let after = fx.state.sessions.get(session_id).unwrap();
    assert!(Arc::ptr_eq(&session, &after), "same session object and id");
    assert_eq!(*after.status.lock().unwrap(), ConnectionStatus::Connected);
    assert_eq!(after.context.lock().await.history.len(), history_len);
    assert!(old_closed.load(Ordering::SeqCst), "old transport is closed");
    assert!(
        !new_closed.load(Ordering::SeqCst),
        "new transport stays open"
    );
    assert!(!after.transport.is_closed());
    let seen = events.lock().unwrap().clone();
    assert!(seen.iter().any(|p| p.contains("reconnecting")), "{seen:?}");
    assert!(
        seen.last()
            .is_some_and(|p| p.contains("connected") && !p.contains("dis")),
        "{seen:?}"
    );
    reset(app.handle());
}

#[tokio::test]
async fn test_reconnect_rereads_sudo_password_for_sudo_and_redactor() {
    use app_logic::server_credentials::sudo_password_credential_ref;
    use secrecy::ExposeSecret;
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let old_closed = Arc::new(AtomicBool::new(false));
    let new_closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&old_closed));
    let fx = fixture(connector.clone()).await;
    let cred_ref = sudo_password_credential_ref(fx.server_id);
    fx.state
        .credential_store
        .set(&cred_ref, "old-sudo-pw".to_string().into())
        .unwrap();
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &old_closed).await;
    assert_eq!(
        session.sudo_password.get().unwrap().expose_secret(),
        "old-sudo-pw"
    );
    fx.state
        .credential_store
        .set(&cred_ref, "new-sudo-pw".to_string().into())
        .unwrap();
    connector.push(connected(&new_closed));
    let (elevated, grants) = registry_pair();

    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .expect("reconnect should succeed");

    let after = fx.state.sessions.get(session_id).unwrap();
    // The password used for `sudo -S` and the redactor come from one read.
    assert_eq!(
        after.sudo_password.get().unwrap().expose_secret(),
        "new-sudo-pw"
    );
    let redacted = after.redactor.redact_text("echo new-sudo-pw");
    assert!(!redacted.contains("new-sudo-pw"), "{redacted}");
    reset(app.handle());
}

#[tokio::test]
async fn test_reconnect_of_a_live_session_does_nothing() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    *session.status.lock().unwrap() = ConnectionStatus::Connected;
    let calls = connector.calls.load(Ordering::SeqCst);
    let (elevated, grants) = registry_pair();

    let err = reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .unwrap_err();

    assert_eq!(err.code, Some("SESSION_NOT_RECONNECTABLE"));
    assert_eq!(connector.calls.load(Ordering::SeqCst), calls);
    assert!(!closed.load(Ordering::SeqCst), "live connection untouched");
    reset(app.handle());
}

#[tokio::test]
async fn test_second_reconnect_trigger_while_one_runs_starts_no_connect() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    let calls = connector.calls.load(Ordering::SeqCst);
    let (elevated, grants) = registry_pair();

    // The first trigger holds the claim (its connect is "running").
    let running = session.begin_reconnect().expect("first trigger claims");
    let err = reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .unwrap_err();

    assert_eq!(err.code, Some("SESSION_NOT_RECONNECTABLE"));
    assert_eq!(connector.calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Reconnecting
    );
    drop(running);
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );
    reset(app.handle());
}

#[tokio::test]
async fn test_failed_reconnect_stays_disconnected_and_can_be_retried() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    let (elevated, grants) = registry_pair();
    connector.push(Err(SshError::ConnectionFailed("down".to_string())));

    let err = reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .unwrap_err();
    assert!(err.code.is_some(), "the translated error code is reported");
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );

    let fresh = Arc::new(AtomicBool::new(false));
    connector.push(connected(&fresh));
    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .expect("the button stays usable");
    assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);
    reset(app.handle());
}

#[tokio::test]
async fn test_pending_confirmation_at_drop_is_rejected_not_executed_after_reconnect() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    let action_id = uuid::Uuid::new_v4();
    let waiting = fx.state.pending_action_confirmations.register(action_id);
    *session.pending_action.lock().unwrap() = Some(action_id);
    connector.push(connected(&Arc::new(AtomicBool::new(false))));
    let (elevated, grants) = registry_pair();

    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .unwrap();

    assert!(
        matches!(waiting.await, Ok(app_logic::dto::ActionUserDecision::Deny)),
        "the waiting action is denied, never approved"
    );
    reset(app.handle());
}

/// Issue #356: an automatic attempt never opens the host key dialog and
/// never trusts a key; the tab stays disconnected.
#[tokio::test]
async fn test_automatic_attempt_stops_at_a_changed_host_key_without_a_dialog() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    connector.push(Ok(ConnectOutcome::PendingHostKeyConfirmation {
        host: "example.invalid".to_string(),
        port: 22,
        raw_key: vec![1, 2, 3],
        decision: HostKeyDecision::Mismatch {
            expected_fingerprint: "SHA256:old".to_string(),
            actual_fingerprint: "SHA256:new".to_string(),
        },
    }));
    let (elevated, grants) = registry_pair();

    let err = reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        false,
    )
    .await
    .unwrap_err();

    assert_eq!(err.code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
    assert!(!app_logic::auto_reconnect::is_retryable_code(err.code));
    assert!(
        fx.state
            .pending_host_key_confirmations
            .resolve(&session_id, HostKeyUserDecision::Trust)
            .is_err(),
        "no host key question was registered"
    );
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );
    reset(app.handle());
}

#[tokio::test]
async fn test_rejected_host_key_during_reconnect_leaves_the_tab_disconnected() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    connector.push(Ok(ConnectOutcome::PendingHostKeyConfirmation {
        host: "example.invalid".to_string(),
        port: 22,
        raw_key: vec![1, 2, 3],
        decision: HostKeyDecision::Mismatch {
            expected_fingerprint: "SHA256:old".to_string(),
            actual_fingerprint: "SHA256:new".to_string(),
        },
    }));
    let (elevated, grants) = registry_pair();

    let reject = async {
        loop {
            if fx
                .state
                .pending_host_key_confirmations
                .resolve(&session_id, HostKeyUserDecision::Reject)
                .is_ok()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    let (result, ()) = tokio::join!(
        reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true
        ),
        reject
    );

    assert_eq!(result.unwrap_err().code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );
    reset(app.handle());
}

#[tokio::test]
async fn test_reconnect_goes_through_the_jump_host_chain_again() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let closed = Arc::new(AtomicBool::new(false));
    let connector = ScriptedConnector::new(connected(&closed));
    let fx = fixture(connector.clone()).await;
    let mut jump = server();
    jump.name = "bastion".to_string();
    fx.state.profile_store.create_server(&jump).await.unwrap();
    let mut target = fx
        .state
        .profile_store
        .get_server(&fx.server_id)
        .await
        .unwrap();
    target.jump_host = Some(jump.id);
    fx.state.profile_store.update_server(&target).await.unwrap();
    let (session_id, _session) = connected_then_lost(&app, &fx, &connector, &closed).await;
    assert_eq!(connector.last_hops.load(Ordering::SeqCst), 2);
    connector.last_hops.store(0, Ordering::SeqCst);
    connector.push(connected(&Arc::new(AtomicBool::new(false))));
    let (elevated, grants) = registry_pair();

    reconnect_session_inner(
        app.handle(),
        &fx.state,
        &elevated,
        &grants,
        session_id,
        true,
    )
    .await
    .unwrap();

    assert_eq!(
        connector.last_hops.load(Ordering::SeqCst),
        2,
        "bastion + target"
    );
    reset(app.handle());
}

// --- Issue #395: reconnect through a real two-hop SSH chain -------------

use self::two_hop_fixture::{RunningTestServer, TEST_PASSWORD, TEST_USERNAME};

const HOP_PASSWORD_REF: &str = "test:hop-password";

/// Trusts every host key: the host key dialog is not under test here.
struct TrustAll;
impl HostKeyStore for TrustAll {
    fn check(&self, _host: &str, _port: u16, _key: &[u8]) -> HostKeyDecision {
        HostKeyDecision::Trusted
    }
    fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
        Ok(())
    }
}

/// TCP forwarder in front of the bastion. It counts the connections it
/// accepted (what only a connection *through* the bastion can raise),
/// can cut all live connections like a network drop, and can be stopped
/// so that the bastion is unreachable.
struct BastionProxy {
    port: u16,
    accepted: Arc<AtomicUsize>,
    connections: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    accept_task: tokio::task::JoinHandle<()>,
}

impl BastionProxy {
    async fn start(target: std::net::SocketAddr) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let (count, conns) = (accepted.clone(), connections.clone());
        let accept_task = tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let handle = tokio::spawn(async move {
                    if let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
                conns.lock().unwrap().push(handle);
            }
        });
        Self {
            port,
            accepted,
            connections,
            accept_task,
        }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Cuts every live connection; new ones are still accepted.
    async fn sever_connections(&self) {
        let handles: Vec<_> = self.connections.lock().unwrap().drain(..).collect();
        for handle in handles {
            handle.abort();
            let _ = handle.await;
        }
    }

    /// Closes the listener: the bastion is unreachable from now on.
    async fn stop(&self) {
        self.accept_task.abort();
        // Wait until the aborted task has dropped the listener.
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_err()
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("the bastion proxy still accepts connections");
    }
}

fn password_server(name: &str, port: u16, jump_host: Option<ServerId>) -> Server {
    let mut server = server();
    server.name = name.to_string();
    server.host = "127.0.0.1".to_string();
    server.port = port;
    server.username = TEST_USERNAME.to_string();
    server.auth = AuthMethod::Password {
        credential_ref: CredentialRef::new(HOP_PASSWORD_REF),
    };
    server.jump_host = jump_host;
    server
}

struct TwoHop {
    fx: Fixture,
    target_id: ServerId,
    proxy: BastionProxy,
    // Keep the in-process servers alive.
    _bastion: RunningTestServer,
    _target: RunningTestServer,
}

/// Real connector, bastion behind a [`BastionProxy`], target reachable
/// only by the bastion's jump chain from the profile's point of view.
async fn two_hop_fixture() -> TwoHop {
    let bastion = RunningTestServer::start().await;
    let target = RunningTestServer::start().await;
    let proxy = BastionProxy::start(bastion.addr).await;
    let mut fx = fixture(Arc::new(app_logic::test_connection::RealConnector)).await;
    fx.state.host_key_store = Arc::new(TrustAll);
    fx.state.credential_store = Arc::new(
        InMemoryCredentialStore::new()
            .with_secret(&CredentialRef::new(PROVIDER_SECRET_REF), "sk-test")
            .with_secret(&CredentialRef::new(HOP_PASSWORD_REF), TEST_PASSWORD),
    );
    let jump = password_server("bastion", proxy.port, None);
    let target_server = password_server("target", target.addr.port(), Some(jump.id));
    fx.state.profile_store.create_server(&jump).await.unwrap();
    fx.state
        .profile_store
        .create_server(&target_server)
        .await
        .unwrap();
    TwoHop {
        fx,
        target_id: target_server.id,
        proxy,
        _bastion: bastion,
        _target: target,
    }
}

/// Connects through both hops, then cuts the connection to the bastion
/// and waits until the session has noticed (as the terminal actor
/// would report it).
async fn connect_and_sever(
    app: &tauri::App<tauri::test::MockRuntime>,
    hop: &TwoHop,
) -> (SessionId, Arc<Session>) {
    let session_id = uuid::Uuid::new_v4();
    connect_session(
        app.handle(),
        &hop.fx.state,
        hop.target_id,
        session_id,
        None,
        false,
    )
    .await
    .expect("two-hop connect should succeed");
    assert_eq!(hop.proxy.accepted(), 1, "first connect went via bastion");
    let session = hop.fx.state.sessions.get(session_id).unwrap();
    let out = session
        .transport
        .lock()
        .await
        .execute("before")
        .await
        .unwrap();
    assert_eq!(out.stdout, b"echo:before\n");

    hop.proxy.sever_connections().await;
    let mut gone = false;
    for _ in 0..100 {
        if session.transport.lock().await.execute("x").await.is_err() {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(gone, "the session should end after the cut");
    *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
    (session_id, session)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_reconnect_over_a_real_two_hop_chain_runs_a_command_via_the_bastion() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let hop = two_hop_fixture().await;
    let (session_id, session) = connect_and_sever(&app, &hop).await;
    let (elevated, grants) = registry_pair();

    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        reconnect_session_inner(
            app.handle(),
            &hop.fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        ),
    )
    .await
    .expect("reconnect must not hang")
    .expect("reconnect through the bastion should succeed");

    assert_eq!(
        hop.proxy.accepted(),
        2,
        "the reconnect must open a new connection to the BASTION; a direct \
         connection to the target would leave this at 1"
    );
    assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);
    assert!(!session.transport.is_closed());
    let out = session
        .transport
        .lock()
        .await
        .execute("after-reconnect")
        .await
        .expect("a command runs on the new transport");
    assert_eq!(out.stdout, b"echo:after-reconnect\n");
    assert_eq!(out.exit_code, Some(0));
    reset(app.handle());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_reconnect_with_unreachable_bastion_fails_and_session_stays_dead() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let hop = two_hop_fixture().await;
    let (session_id, session) = connect_and_sever(&app, &hop).await;
    hop.proxy.stop().await;
    let (elevated, grants) = registry_pair();

    let err = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        reconnect_session_inner(
            app.handle(),
            &hop.fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        ),
    )
    .await
    .expect("reconnect must not hang")
    .expect_err("reconnect must fail while the bastion is unreachable");

    assert!(err.code.is_some(), "stable error code expected: {err:?}");
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected,
        "the session stays dead and can be retried"
    );
    assert!(
        session.transport.is_closed(),
        "no half-swapped transport: the dead one stays in place, closed"
    );
    assert!(
        session
            .transport
            .lock()
            .await
            .execute("nope")
            .await
            .is_err(),
        "no command may run on a failed reconnect"
    );
    assert_eq!(hop.proxy.accepted(), 1, "no connection was accepted");
    reset(app.handle());
}

// --- Issue #546: live connect progress and cancel -----------------------

type StepScript = Box<dyn FnOnce(&ConnectLog) + Send>;
type ScriptItem = (StepScript, Option<Result<ConnectOutcome, SshError>>);

/// Writes the scripted steps into the attempt log, then returns the result
/// (`None`: never returns, like a server that does not answer).
struct StepConnector {
    queue: Mutex<std::collections::VecDeque<ScriptItem>>,
}

impl StepConnector {
    fn new(items: Vec<(StepScript, Option<Result<ConnectOutcome, SshError>>)>) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(items.into()),
        })
    }
}

#[async_trait]
impl Connector for StepConnector {
    async fn connect(
        &self,
        _target: &ConnectionTarget,
        _credentials: &(dyn CredentialStore + Send + Sync),
        _key_files: &(dyn KeyFileReader + Send + Sync),
        _host_keys: Arc<dyn HostKeyStore>,
        log: &ConnectLog,
        _limits: ssh_transport::ConnectLimits,
    ) -> Result<ConnectOutcome, SshError> {
        let (script, result) = self
            .queue
            .lock()
            .unwrap()
            .pop_front()
            .expect("connector called too often");
        script(log);
        match result {
            Some(r) => r,
            None => std::future::pending().await,
        }
    }
}

#[derive(Default)]
struct RecordingEmitter(Mutex<Vec<serde_json::Value>>);
impl EventEmitter for RecordingEmitter {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        if event == "connect-progress" {
            self.0.lock().unwrap().push(payload);
        }
    }
}

fn step(log: &ConnectLog, hop: &str, step: ssh_manager_core::ssh::ConnectStep, ok: bool) {
    let id = log.start(0, hop, step);
    if ok {
        log.succeed(id);
    }
}

fn tcp(log: &ConnectLog) {
    step(
        log,
        "root@example.invalid:22",
        ssh_manager_core::ssh::ConnectStep::TcpConnect {
            address: None,
            port: 22,
        },
        true,
    );
}

fn handshake_step() -> ssh_manager_core::ssh::ConnectStep {
    ssh_manager_core::ssh::ConnectStep::Handshake {
        server_version: None,
        kex: None,
        host_key_algorithm: None,
        cipher: None,
        mac: None,
    }
}

fn pending_unknown() -> ConnectOutcome {
    ConnectOutcome::PendingHostKeyConfirmation {
        host: "example.invalid".to_string(),
        port: 22,
        raw_key: vec![1, 2, 3],
        decision: HostKeyDecision::Unknown {
            fingerprint: "SHA256:x".to_string(),
        },
    }
}

fn host_key_unknown_steps(log: &ConnectLog) {
    tcp(log);
    let id = log.start(
        0,
        "root@example.invalid:22",
        ssh_manager_core::ssh::ConnectStep::HostKeyCheck {
            key_type: None,
            fingerprint: None,
            result: Some(ssh_manager_core::ssh::HostKeyCheckResult::Unknown),
        },
    );
    log.fail(id, ssh_manager_core::ssh::HOST_KEY_UNKNOWN);
}

async fn wait_until(mut cond: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !cond() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("condition not reached in time");
}

#[tokio::test]
async fn test_progress_events_carry_attempt_id_and_end_with_all_steps_ok() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let disconnected = Arc::new(AtomicBool::new(false));
    let script: StepScript = Box::new(|log| {
        tcp(log);
        step(log, "root@example.invalid:22", handshake_step(), true);
        step(
            log,
            "root@example.invalid:22",
            ssh_manager_core::ssh::ConnectStep::SessionReady,
            true,
        );
    });
    let connector = StepConnector::new(vec![(script, Some(connected(&disconnected)))]);
    let fx = fixture(connector).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "attempt-1".into(), emitter.clone());

    connect_session_with_progress(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        true,
        Some(&progress),
    )
    .await
    .unwrap();

    let events = emitter.0.lock().unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().all(|e| e["attemptId"] == "attempt-1"
        && e["attempt"] == 1
        && e["serverId"] == serde_json::json!(fx.server_id)));
    let last = events.last().unwrap()["steps"].as_array().unwrap().clone();
    assert_eq!(last.last().unwrap()["kind"], "sessionReady");
    assert!(last.iter().all(|s| s["status"] == "ok"));
    reset(app.handle());
}

#[tokio::test]
async fn test_auth_failure_emits_failed_auth_step_and_keeps_the_error_log() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let script: StepScript = Box::new(|log| {
        tcp(log);
        step(
            log,
            "root@example.invalid:22",
            ssh_manager_core::ssh::ConnectStep::Authentication {
                method: ssh_manager_core::ssh::AuthMethodKind::IdentityFile,
                remaining_methods: vec![],
                partial_success: false,
            },
            false,
        );
        log.fail_running("SSH_AUTH_FAILED");
    });
    let connector = StepConnector::new(vec![(script, Some(Err(SshError::AuthenticationFailed)))]);
    let fx = fixture(connector).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "a".into(), emitter.clone());

    let err = connect_session_with_progress(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        true,
        Some(&progress),
    )
    .await
    .unwrap_err();

    assert_eq!(err.code, Some("SSH_AUTH_FAILED"));
    assert!(err.connect_log.is_some());
    let events = emitter.0.lock().unwrap();
    let last = events.last().unwrap()["steps"].as_array().unwrap().clone();
    let auth = last.last().unwrap();
    assert_eq!(auth["kind"], "authentication");
    assert_eq!(auth["status"], "failed");
    assert_eq!(auth["errorCode"], "SSH_AUTH_FAILED");
    assert_eq!(auth["detail"]["method"], "publickey (key file)");
    reset(app.handle());
}

#[tokio::test]
async fn test_handshake_timeout_emits_failed_step_with_timeout_code() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let script: StepScript = Box::new(|log| {
        step(log, "root@example.invalid:22", handshake_step(), false);
        log.fail_running("SSH_TIMEOUT");
    });
    let connector = StepConnector::new(vec![(script, Some(Err(SshError::Timeout)))]);
    let fx = fixture(connector).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "a".into(), emitter.clone());

    let err = connect_session_with_progress(
        app.handle(),
        &fx.state,
        fx.server_id,
        uuid::Uuid::new_v4(),
        None,
        true,
        Some(&progress),
    )
    .await
    .unwrap_err();

    assert_eq!(err.code, Some("SSH_TIMEOUT"));
    let events = emitter.0.lock().unwrap();
    let first = events.first().unwrap()["steps"][0].clone();
    assert_eq!(first["status"], "running");
    assert_eq!(first["detail"]["code"], "waitingForResponse");
    let last = events.last().unwrap()["steps"][0].clone();
    assert_eq!(last["status"], "failed");
    assert_eq!(last["errorCode"], "SSH_TIMEOUT");
    reset(app.handle());
}

#[tokio::test]
async fn test_host_key_prompt_waits_for_decision_and_retry_is_attempt_two() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let disconnected = Arc::new(AtomicBool::new(false));
    let second: StepScript = Box::new(|log| {
        tcp(log);
    });
    let connector = StepConnector::new(vec![
        (
            Box::new(host_key_unknown_steps),
            Some(Ok(pending_unknown())),
        ),
        (second, Some(connected(&disconnected))),
    ]);
    let fx = fixture(connector).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "a".into(), emitter.clone());
    let tab = uuid::Uuid::new_v4();

    let decide = async {
        wait_until(|| {
            emitter.0.lock().unwrap().iter().any(|e| {
                e["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["detail"]["code"] == "waitingForDecision")
            })
        })
        .await;
        let waiting = emitter
            .0
            .lock()
            .unwrap()
            .iter()
            .find_map(|e| {
                e["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|s| s["detail"]["code"] == "waitingForDecision")
                    .cloned()
            })
            .unwrap();
        assert_eq!(waiting["status"], "running");
        assert_eq!(waiting["kind"], "hostKeyCheck");
        wait_until(|| {
            fx.state
                .pending_host_key_confirmations
                .resolve(&tab, HostKeyUserDecision::Trust)
                .is_ok()
        })
        .await;
    };
    let (result, ()) = tokio::join!(
        connect_session_with_progress(
            app.handle(),
            &fx.state,
            fx.server_id,
            tab,
            None,
            true,
            Some(&progress)
        ),
        decide
    );
    result.unwrap();

    let events = emitter.0.lock().unwrap();
    assert_eq!(events.last().unwrap()["attempt"], 2);
    assert_eq!(events.last().unwrap()["steps"].as_array().unwrap().len(), 1);
    reset(app.handle());
}

#[tokio::test]
async fn test_cancel_during_handshake_ends_connect_without_session() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let script: StepScript = Box::new(|log| {
        step(log, "root@example.invalid:22", handshake_step(), false);
    });
    let connector = StepConnector::new(vec![(script, None)]);
    let fx = fixture(connector).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "cancel-me".into(), emitter.clone());
    let tab = uuid::Uuid::new_v4();

    let cancel = async {
        wait_until(|| !emitter.0.lock().unwrap().is_empty()).await;
        fx.state.connect_attempts.cancel("cancel-me");
    };
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            connect_session_with_progress(
                app.handle(),
                &fx.state,
                fx.server_id,
                tab,
                None,
                true,
                Some(&progress)
            )
        ),
        cancel
    );

    let err = result
        .expect("cancel must end connect within 1 s")
        .unwrap_err();
    assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
    assert!(fx.state.sessions.get(tab).is_none());
    assert!(fx
        .state
        .chat_session_store
        .list_sessions_for_server(&fx.server_id)
        .await
        .unwrap()
        .is_empty());
    let events = emitter.0.lock().unwrap();
    let last = events.last().unwrap()["steps"][0].clone();
    assert_eq!(last["status"], "failed");
    assert_eq!(last["errorCode"], "SSH_CONNECT_CANCELLED");
    reset(app.handle());
}

/// Issue #568: a resume carries an attempt id like a plain connect —
/// progress events use it, `cancel_connect` aborts it, and the saved chat
/// session stays untouched.
#[tokio::test]
async fn test_resume_emits_progress_and_cancel_leaves_saved_session_unchanged() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let script: StepScript = Box::new(|log| {
        step(log, "root@example.invalid:22", handshake_step(), false);
    });
    let connector = StepConnector::new(vec![(script, None)]);
    let fx = fixture(connector).await;
    let store = &fx.state.chat_session_store;
    let existing = store.create_session(&fx.server_id, None).await.unwrap();
    store.mark_ended(existing).await.unwrap();
    let before = store.list_sessions_for_server(&fx.server_id).await.unwrap();
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "resume-1".into(), emitter.clone());
    let tab = uuid::Uuid::new_v4();

    let cancel = async {
        wait_until(|| !emitter.0.lock().unwrap().is_empty()).await;
        fx.state.connect_attempts.cancel("resume-1");
    };
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            resume_with_progress(
                app.handle(),
                &fx.state,
                fx.server_id,
                tab,
                existing,
                &progress
            )
        ),
        cancel
    );

    let err = result
        .expect("cancel must end the resume within 1 s")
        .unwrap_err();
    assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
    assert!(fx.state.sessions.get(tab).is_none());
    assert!(emitter
        .0
        .lock()
        .unwrap()
        .iter()
        .all(|e| e["attemptId"] == "resume-1"));
    let after = store.list_sessions_for_server(&fx.server_id).await.unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0].ended_at, before[0].ended_at);
    reset(app.handle());
}

#[tokio::test]
async fn test_cancel_during_host_key_wait_releases_registration_and_ends_prompt() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let connector = StepConnector::new(vec![(
        Box::new(host_key_unknown_steps),
        Some(Ok(pending_unknown())),
    )]);
    let fx = fixture(connector).await;
    let ended = Arc::new(Mutex::new(Vec::new()));
    let sink = ended.clone();
    app.handle()
        .listen("host-key-verification-ended", move |event| {
            sink.lock().unwrap().push(event.payload().to_string());
        });
    let emitter = Arc::new(RecordingEmitter::default());
    let progress = ConnectProgress::new(&fx.state, "hk".into(), emitter.clone());
    let tab = uuid::Uuid::new_v4();

    let cancel = async {
        wait_until(|| {
            emitter.0.lock().unwrap().iter().any(|e| {
                e["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["detail"]["code"] == "waitingForDecision")
            })
        })
        .await;
        fx.state.connect_attempts.cancel("hk");
    };
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            connect_session_with_progress(
                app.handle(),
                &fx.state,
                fx.server_id,
                tab,
                None,
                true,
                Some(&progress)
            )
        ),
        cancel
    );

    let err = result
        .expect("cancel must end connect within 1 s")
        .unwrap_err();
    assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
    assert!(fx.state.sessions.get(tab).is_none());
    assert!(
        fx.state
            .pending_host_key_confirmations
            .resolve(&tab, HostKeyUserDecision::Trust)
            .is_err(),
        "the host key registration was released"
    );
    assert_eq!(ended.lock().unwrap().len(), 1, "prompt end event emitted");
    assert!(fx
        .state
        .chat_session_store
        .list_sessions_for_server(&fx.server_id)
        .await
        .unwrap()
        .is_empty());
    reset(app.handle());
}

#[tokio::test]
async fn test_cancel_of_unknown_attempt_is_a_noop() {
    let _guard = lock_async().await;
    let app = acknowledged_app();
    let connector = StepConnector::new(vec![]);
    let fx = fixture(connector).await;
    assert!(!fx.state.connect_attempts.cancel("does-not-exist"));
    reset(app.handle());
}
