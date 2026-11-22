//! Read-only commands over the activity record (Spec 0057, issue #1450):
//! thin wrappers around the `app_logic::ext` read API of #1449, for the
//! frontend public API. They sit behind the startup gate like every other
//! command (not on its allow list), so they fail while the app is locked.

use app_logic::error::CommandResult;
use app_logic::ext::{
    self, RecordEntryFilter, RecordEntryInfo, RecordPage, RecordPageRequest, RecordedSessionFilter,
    RecordedSessionInfo,
};
use app_logic::state::AppState;
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub async fn record_list_sessions(
    state: State<'_, AppState>,
    filter: RecordedSessionFilter,
    page: RecordPageRequest,
) -> CommandResult<RecordPage<RecordedSessionInfo>> {
    ext::list_recorded_sessions(&state, &filter, &page).await
}

#[tauri::command]
pub async fn record_list_entries(
    state: State<'_, AppState>,
    filter: RecordEntryFilter,
    page: RecordPageRequest,
) -> CommandResult<RecordPage<RecordEntryInfo>> {
    ext::list_record_entries(&state, &filter, &page).await
}

#[tauri::command]
pub async fn record_session_entries(
    state: State<'_, AppState>,
    session_id: Uuid,
) -> CommandResult<Vec<RecordEntryInfo>> {
    ext::record_entries_of_session(&state, session_id).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use app_logic::confirmation::ConfirmationRegistry;
    use app_logic::test_support::InMemoryCredentialStore;
    use persistence_sqlite::SqliteProfileStore;
    use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
    use ssh_manager_core::profiles::ProfileStore;
    use ssh_manager_core::ssh::{HostKeyDecision, HostKeyStore, SshError};
    use ssh_manager_core::testing::ServerBuilder;
    use tauri::Manager;

    use super::*;

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

    async fn state(dir: &std::path::Path) -> (AppState, Uuid) {
        let db = SqliteProfileStore::connect_plaintext(&dir.join("main.db"))
            .await
            .unwrap();
        let server = ServerBuilder::new().build();
        db.create_server(&server).await.unwrap();
        let session = db
            .chat_session_store()
            .create_session(&server.id, None)
            .await
            .unwrap();
        for i in 0..3 {
            db.ledger_store()
                .append_entry(
                    session,
                    LedgerSource::User,
                    &LedgerEntryContent::ActivityTick,
                )
                .await
                .unwrap_or_else(|e| panic!("append {i}: {e}"));
        }
        let data_changes = db.data_changes();
        let db = Arc::new(db);
        let state = AppState {
            sessions: app_logic::session::SessionManager::new(),
            session_extension: None,
            root_key_fingerprint: [0; 32],
            ai_provider_store: Arc::new(db.ai_provider_store()),
            backup_store: db.backup_store(),
            policy_store: db.policy_store(),
            prompt_history_store: db.prompt_history_store(),
            chat_session_store: db.chat_session_store(),
            ledger_store: db.ledger_store(),
            profile_store: db.clone(),
            credential_store: Arc::new(InMemoryCredentialStore::new()),
            key_file_reader: Arc::new(ssh_manager_core::ssh::mock::MockKeyFileReader::new()),
            keychain: credentials_keyring::KeychainAvailability::Available,
            file_host_keys: Arc::new(
                app_logic::host_key_store::FileHostKeyStore::load(dir.join("hk.json")).unwrap(),
            ),
            host_key_store: Arc::new(NoHostKeys),
            data_changes,
            connector: Arc::new(app_logic::test_connection::RealConnector),
            entitlements: Arc::new(ssh_manager_core::entitlements::FixedEntitlements(
                ssh_manager_core::entitlements::Entitlements::free(),
            )),
            pending_host_key_confirmations: ConfirmationRegistry::new(),
            pending_auth_prompts: Default::default(),
            connect_attempts: Default::default(),
            pending_action_confirmations: ConfirmationRegistry::new(),
            running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
            mcp: app_logic::state::McpState::default(),
            rate_limit_registry: ai_providers::RateLimitRegistry::new(),
            pending_ssh_config_import: std::sync::Mutex::new(None),
            outgoing_transform: None,
            chat_content: Default::default(),
            title_attempts: Default::default(),
            ai_usage_observers: Default::default(),
            ai_provider_decorators: Default::default(),
        };
        (state, session)
    }

    /// Each command returns exactly what the `ext` function returns for the
    /// same filter, including a second page reached with the cursor.
    #[tokio::test]
    async fn test_commands_return_the_same_result_as_the_ext_functions() {
        let dir = tempfile::tempdir().unwrap();
        let (state, session) = state(dir.path()).await;
        let app = tauri::test::mock_app();
        app.manage(state);
        let st = || app.state::<AppState>();

        let sf = RecordedSessionFilter::default();
        let page = RecordPageRequest {
            page_size: 10,
            cursor: None,
        };
        let via_cmd = record_list_sessions(st(), sf.clone(), page.clone())
            .await
            .unwrap();
        let via_ext = ext::list_recorded_sessions(&st(), &sf, &page)
            .await
            .unwrap();
        assert_eq!(via_cmd, via_ext);
        assert_eq!(via_cmd.items.len(), 1);

        let ef = RecordEntryFilter::default();
        let small = RecordPageRequest {
            page_size: 2,
            cursor: None,
        };
        let first = record_list_entries(st(), ef.clone(), small.clone())
            .await
            .unwrap();
        assert_eq!(
            first,
            ext::list_record_entries(&st(), &ef, &small).await.unwrap()
        );
        assert_eq!(first.items.len(), 2);
        let next = RecordPageRequest {
            page_size: 2,
            cursor: first.next_cursor.clone(),
        };
        let second = record_list_entries(st(), ef.clone(), next.clone())
            .await
            .unwrap();
        assert_eq!(
            second,
            ext::list_record_entries(&st(), &ef, &next).await.unwrap()
        );
        assert_eq!(second.items.len(), 1);

        let all = record_session_entries(st(), session).await.unwrap();
        assert_eq!(
            all,
            ext::record_entries_of_session(&st(), session)
                .await
                .unwrap()
        );
        assert_eq!(all.len(), 3);
    }
}
