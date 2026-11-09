//! Tests of the session builder with in-memory stores (issue #839): a
//! scripted transport, real SQLite stores in a temp dir, no Tauri.
#![expect(clippy::expect_used, reason = "test code")]

use ssh_manager_core::testing::ServerBuilder;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use persistence_sqlite::{AiProviderConfig, SqliteProfileStore};
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::ai::{
    fence_untrusted, AiEvent, ChatMessage, MessageContent, OutputRedactor, ProviderId,
    ProviderType, RedactedContext, Role, UntrustedKind,
};
use ssh_manager_core::profiles::{CredentialRef, ProfileStore, Server};
use ssh_manager_core::ssh::{
    CommandOutput, HostKeyDecision, HostKeyStore, InteractiveShell, PtySize, SshError, SshTransport,
};

use super::*;
use crate::confirmation::ConfirmationRegistry;
use crate::session_extension::{ScopeLookup, ServerScope, SessionExtension};
use crate::test_support::InMemoryCredentialStore;

const SUDO_SECRET: &str = "hunter2-sudo-pw";
const EXT_SECRET: &str = "ext-secret-4711";

struct NoHostKeys;
impl HostKeyStore for NoHostKeys {
    fn check(&self, _host: &str, _port: u16, _key: &[u8]) -> HostKeyDecision {
        HostKeyDecision::Unknown {
            fingerprint: String::new(),
        }
    }
    fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
        Ok(())
    }
}

/// Answers `uname -a` and `id -u` from a script and records every command.
struct ScriptedTransport {
    uname: Result<String, ()>,
    id_u: Result<String, ()>,
    commands: Arc<Mutex<Vec<String>>>,
    disconnected: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl SshTransport for ScriptedTransport {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        self.commands.lock().unwrap().push(command.to_string());
        let answer = if command == "uname -a" {
            &self.uname
        } else {
            &self.id_u
        };
        match answer {
            Ok(text) => Ok(CommandOutput {
                stdout: text.clone().into_bytes(),
                stderr: Vec::new(),
                exit_code: Some(0),
                truncated: false,
            }),
            Err(()) => Err(SshError::ChannelError("closed".into())),
        }
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!("not used by the builder")
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        self.disconnected.store(true, Ordering::SeqCst);
        Ok(())
    }
}

struct NoopAi;
impl AiProvider for NoopAi {
    fn send(
        &self,
        _context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![AiEvent::Done]))
    }
}

struct SecretExtension;
#[async_trait]
impl SessionExtension for SecretExtension {
    fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
        vec![SecretString::from(EXT_SECRET.to_string())]
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
        credential_ref: CredentialRef::new("test:ai"),
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

struct Fixture {
    state: AppState,
    _dir: tempfile::TempDir,
    server: Server,
    config: AiProviderConfig,
}

async fn fixture(
    credentials: InMemoryCredentialStore,
    extension: Option<Arc<dyn SessionExtension>>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteProfileStore::connect_plaintext(&dir.path().join("main.db"))
        .await
        .unwrap();
    let server = ServerBuilder::new()
        .name("web-1")
        .username("root")
        .post_ingest_policy(ssh_manager_core::profiles::PostIngestPolicy::Balanced)
        .build();
    let config = provider_config();
    db.create_server(&server).await.unwrap();
    db.ai_provider_store().create(&config).await.unwrap();
    let data_changes = db.data_changes();
    let state = AppState {
        sessions: crate::session::SessionManager::new(),
        session_extension: extension,
        root_key_fingerprint: [0; 32],
        ai_provider_store: Arc::new(db.ai_provider_store()),
        backup_store: db.backup_store(),
        policy_store: db.policy_store(),
        prompt_history_store: db.prompt_history_store(),
        chat_session_store: db.chat_session_store(),
        ledger_store: db.ledger_store(),
        profile_store: Arc::new(db),
        credential_store: Arc::new(credentials),
        key_file_reader: Arc::new(ssh_manager_core::ssh::mock::MockKeyFileReader::new()),
        keychain: credentials_keyring::KeychainAvailability::Available,
        file_host_keys: Arc::new(
            crate::ext::FileHostKeyStore::load(dir.path().join("hk.json")).unwrap(),
        ),
        host_key_store: Arc::new(NoHostKeys),
        data_changes,
        connector: Arc::new(crate::test_connection::RealConnector),
        entitlements: Arc::new(ssh_manager_core::entitlements::FixedEntitlements(
            ssh_manager_core::entitlements::Entitlements::free(),
        )),
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        pending_auth_prompts: Default::default(),
        connect_attempts: Default::default(),
        pending_action_confirmations: ConfirmationRegistry::new(),
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        mcp: crate::state::McpState::default(),
        rate_limit_registry: ai_providers::RateLimitRegistry::new(),
        pending_ssh_config_import: Mutex::new(None),
        outgoing_transform: None,
        chat_content: Default::default(),
        title_attempts: Default::default(),
        ai_usage_observers: Default::default(),
        ai_provider_decorators: Default::default(),
    };
    Fixture {
        state,
        _dir: dir,
        server,
        config,
    }
}

struct Run {
    built: Result<BuiltSession, CommandError>,
    commands: Arc<Mutex<Vec<String>>>,
}

async fn run(
    fx: &Fixture,
    server_id: ServerId,
    server: Server,
    uname: Result<String, ()>,
    id_u: Result<String, ()>,
    resume: Option<uuid::Uuid>,
) -> Run {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        uname,
        id_u,
        commands: commands.clone(),
        disconnected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let scope = ServerScope {
        server_id,
        group_chain: Vec::new(),
        tags: Vec::new(),
    };
    let built = build_session(
        &fx.state,
        Box::new(transport),
        SessionBuildInput {
            server_id,
            session_id: uuid::Uuid::new_v4(),
            server,
            resume,
            persist_chat_session: true,
            active_config: fx.config.clone(),
            ai_provider: Box::new(NoopAi),
            ai_provider_budget: Arc::new(ProviderBudgetGuard::new()),
            scope_lookup: ScopeLookup {
                scope: Some(scope),
                incomplete: false,
            },
            prompt_language: PromptLanguage::En,
            second_opinion: None,
            second_opinion_setup_failed: false,
            injection_check: None,
            red_risk_always_confirm: true,
        },
    )
    .await;
    Run { built, commands }
}

fn ok(text: &str) -> Result<String, ()> {
    Ok(text.to_string())
}

#[tokio::test]
async fn test_sudo_password_is_masked_by_the_built_redactor() {
    let probe = ServerBuilder::new()
        .name("web-1")
        .username("root")
        .post_ingest_policy(ssh_manager_core::profiles::PostIngestPolicy::Balanced)
        .build();
    let creds = InMemoryCredentialStore::new().with_secret(
        &crate::credential_slots::sudo_password_credential_ref(probe.id),
        SUDO_SECRET,
    );
    let mut fx = fixture(creds, None).await;
    fx.server.id = probe.id;
    let db_server = Server {
        id: probe.id,
        ..fx.server.clone()
    };
    fx.state
        .profile_store
        .create_server(&db_server)
        .await
        .unwrap();
    let run = run(
        &fx,
        probe.id,
        db_server,
        ok("Linux x 6.1"),
        ok("1000"),
        None,
    )
    .await;
    let session = run.built.unwrap().session;
    let out = session
        .redactor
        .redact_text(&format!("password is {SUDO_SECRET} ok"));
    assert!(!out.contains(SUDO_SECRET), "sudo password leaked: {out}");
    assert_eq!(
        session.safety.sudo_password.get().unwrap().expose_secret(),
        SUDO_SECRET
    );
}

#[tokio::test]
async fn test_extension_known_secrets_are_masked() {
    let fx = fixture(
        InMemoryCredentialStore::new(),
        Some(Arc::new(SecretExtension)),
    )
    .await;
    let run = run(
        &fx,
        fx.server.id,
        fx.server.clone(),
        ok("Linux x 6.1"),
        ok("1000"),
        None,
    )
    .await;
    let session = run.built.unwrap().session;
    let out = session
        .redactor
        .redact_text(&format!("token {EXT_SECRET} end"));
    assert!(!out.contains(EXT_SECRET), "extension secret leaked: {out}");
}

#[tokio::test]
async fn test_uname_banner_is_the_first_history_message_and_fenced() {
    let fx = fixture(InMemoryCredentialStore::new(), None).await;
    let run = run(
        &fx,
        fx.server.id,
        fx.server.clone(),
        ok("Linux web-1 6.1.0 x86_64"),
        ok("0"),
        None,
    )
    .await;
    let session = run.built.unwrap().session;
    let ctx = session.ai.context.lock().await;
    let first = ctx.history.first().expect("banner message");
    assert_eq!(first.role, Role::ActionResult);
    let MessageContent::Text(text) = &first.content else {
        panic!("banner is text");
    };
    assert_eq!(
        text,
        &fence_untrusted(
            UntrustedKind::RemoteOsInfo,
            "uname -a",
            "Linux web-1 6.1.0 x86_64"
        )
    );
    assert_eq!(session.is_root(), Some(true));
}

#[tokio::test]
async fn test_failed_id_u_probe_means_root_unknown_and_does_not_fail_connect() {
    let fx = fixture(InMemoryCredentialStore::new(), None).await;
    let run = run(
        &fx,
        fx.server.id,
        fx.server.clone(),
        ok("Linux x 6.1"),
        Err(()),
        None,
    )
    .await;
    let built = run.built.expect("connect must not fail");
    assert_eq!(built.session.is_root(), None);
}

#[tokio::test]
async fn test_resume_loads_history_and_flags_untrusted_content() {
    let fx = fixture(InMemoryCredentialStore::new(), None).await;
    let store = &fx.state.chat_session_store;
    let chat_id = store
        .create_session(&fx.server.id, Some(fx.config.id.0))
        .await
        .unwrap();
    let fenced = ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(fence_untrusted(
            UntrustedKind::CommandStdout,
            "cat /etc/motd",
            "hello",
        )),
    };
    store.append_message(chat_id, &fenced).await.unwrap();
    let run = run(
        &fx,
        fx.server.id,
        fx.server.clone(),
        Err(()),
        ok("1000"),
        Some(chat_id),
    )
    .await;
    let built = run.built.unwrap();
    assert_eq!(built.chat_session_id, Some(chat_id));
    let session = built.session;
    let ctx = session.ai.context.lock().await;
    // No banner (uname failed): exactly the stored message.
    assert_eq!(ctx.history.len(), 1);
    assert!(ctx.history[0].content == fenced.content);
    assert!(session
        .safety
        .untrusted_content_ingested
        .load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_uname_banner_precedes_resumed_stored_history() {
    let fx = fixture(InMemoryCredentialStore::new(), None).await;
    let store = &fx.state.chat_session_store;
    let chat_id = store
        .create_session(&fx.server.id, Some(fx.config.id.0))
        .await
        .unwrap();
    let stored = ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(fence_untrusted(
            UntrustedKind::CommandStdout,
            "cat /etc/motd",
            "hello",
        )),
    };
    store.append_message(chat_id, &stored).await.unwrap();
    let run = run(
        &fx,
        fx.server.id,
        fx.server.clone(),
        ok("Linux web-1 6.1.0 x86_64"),
        ok("0"),
        Some(chat_id),
    )
    .await;
    let session = run.built.unwrap().session;
    let ctx = session.ai.context.lock().await;
    assert_eq!(ctx.history.len(), 2);
    let MessageContent::Text(first) = &ctx.history[0].content else {
        panic!("banner is text");
    };
    assert_eq!(
        first,
        &fence_untrusted(
            UntrustedKind::RemoteOsInfo,
            "uname -a",
            "Linux web-1 6.1.0 x86_64"
        )
    );
    assert!(ctx.history[1].content == stored.content);
}

#[tokio::test]
async fn test_local_pseudo_server_gets_no_sudo_password_and_no_root_probe() {
    // A sudo password stored for some other server must not reach the local one.
    let other = ServerId(uuid::Uuid::new_v4());
    let creds = InMemoryCredentialStore::new().with_secret(
        &crate::credential_slots::sudo_password_credential_ref(other),
        SUDO_SECRET,
    );
    let fx = fixture(creds, None).await;
    let local_id = crate::dto::LOCAL_SERVER_ID;
    let mut local = fx.server.clone();
    local.id = local_id;
    let run = run(&fx, local_id, local, ok("Linux x 6.1"), ok("0"), None).await;
    let built = run.built.unwrap();
    assert!(built.session.safety.sudo_password.get().is_none());
    assert_eq!(built.session.is_root(), None);
    assert_eq!(built.chat_session_id, None);
    assert_eq!(run.commands.lock().unwrap().as_slice(), ["uname -a"]);
}
