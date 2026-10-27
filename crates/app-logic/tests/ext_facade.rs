//! Issue #878: compile-and-run check of `app_logic::ext`. The extension code
//! below imports only from `app_logic::ext` (and `ssh_manager_core`). Only
//! the `fixture` module, which builds a test `AppState` (something an
//! extension never does), reaches outside the facade.

use std::sync::Arc;

use app_logic::ext::{
    self, AiUsageObserver, AiUsagePurpose, AiUsageRecord, AppState, ChatContentExtension,
    PendingContent, PromptLanguage, ResolvedBlock, ServerScope, SessionExtension,
};
use ssh_manager_core::shared::ServerId;

mod fixture {
    use std::sync::Arc;

    use app_logic::confirmation::ConfirmationRegistry;
    use app_logic::ext::{AppState, FileHostKeyStore};
    use persistence_sqlite::SqliteProfileStore;
    use ssh_manager_core::profiles::{CredentialError, CredentialRef, CredentialResult};
    use ssh_manager_core::ssh::{HostKeyDecision, HostKeyStore, SshError};

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

    struct NoCredentials;
    impl ssh_manager_core::profiles::CredentialStore for NoCredentials {
        fn get(&self, r: &CredentialRef) -> CredentialResult<secrecy::SecretString> {
            Err(CredentialError::NotFound(r.clone()))
        }
        fn set(&self, _r: &CredentialRef, _v: secrecy::SecretString) -> CredentialResult<()> {
            Ok(())
        }
        fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
            Ok(())
        }
    }

    pub async fn app_state(
        dir: &std::path::Path,
        extension: Option<Arc<dyn app_logic::ext::SessionExtension>>,
    ) -> AppState {
        let db = SqliteProfileStore::connect_plaintext(&dir.join("main.db"))
            .await
            .unwrap();
        let data_changes = db.data_changes();
        AppState {
            sessions: app_logic::session::SessionManager::new(),
            session_extension: extension,
            root_key_fingerprint: [0; 32],
            ai_provider_store: Arc::new(db.ai_provider_store()),
            backup_store: db.backup_store(),
            policy_store: db.policy_store(),
            prompt_history_store: db.prompt_history_store(),
            chat_session_store: db.chat_session_store(),
            ledger_store: db.ledger_store(),
            profile_store: Arc::new(db),
            credential_store: Arc::new(NoCredentials),
            key_file_reader: Arc::new(ssh_manager_core::ssh::mock::MockKeyFileReader::new()),
            keychain: credentials_keyring::KeychainAvailability::Available,
            file_host_keys: Arc::new(FileHostKeyStore::load(dir.join("hk.json")).unwrap()),
            host_key_store: Arc::new(NoHostKeys),
            data_changes,
            connector: Arc::new(app_logic::test_connection::RealConnector),
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
        }
    }
}

struct MySessionExtension;

#[async_trait::async_trait]
impl SessionExtension for MySessionExtension {
    async fn context_section(
        &self,
        _scope: &ServerScope,
        _language: PromptLanguage,
    ) -> Option<String> {
        Some("section".to_string())
    }
}

struct MyChatContent;

impl ChatContentExtension for MyChatContent {
    fn name(&self) -> &str {
        "facade"
    }
    fn take_pending(&self, _session_id: uuid::Uuid, id: &str) -> Option<PendingContent> {
        Some(PendingContent {
            label: id.to_string(),
            kind: "text".to_string(),
            blocks: Vec::<ResolvedBlock>::new(),
        })
    }
}

struct MyUsageObserver;

impl AiUsageObserver for MyUsageObserver {
    fn on_usage(&self, record: &AiUsageRecord) {
        let _: AiUsagePurpose = record.purpose;
    }
}

#[tokio::test]
async fn test_extension_uses_only_the_facade() {
    let dir = tempfile::tempdir().unwrap();
    let extension: Arc<dyn SessionExtension> = Arc::new(MySessionExtension);
    let state: AppState = fixture::app_state(dir.path(), Some(extension)).await;

    // The extension traits are object-safe and usable as registered.
    let _chat: Arc<dyn ChatContentExtension> = Arc::new(MyChatContent);
    let _usage: Arc<dyn AiUsageObserver> = Arc::new(MyUsageObserver);

    // Every accessor works on the test state.
    let profiles = ext::profile_store(&state);
    assert!(profiles.list_servers().await.unwrap().is_empty());
    let snapshot = ext::backup_store(&state);
    let _ = snapshot;
    let host_keys = ext::file_host_keys(&state);
    let _ = host_keys;
    assert!(ext::open_session_server_ids(&state).is_empty());
    ext::rebuild_redactors_for_open_sessions(&state).await;

    // Helpers re-exported from the facade.
    assert!(!ext::is_local(ServerId::new()));
    assert!(ext::scope_for_id(profiles, ServerId::new(), None)
        .await
        .is_none());
    assert!(ext::KNOWN_SECTIONS.len() > 1);
}
