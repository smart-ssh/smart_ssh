//! Issue #464 (Spec 0038): the outgoing transform runs on the final approved
//! text, right before the transport call, and nowhere else.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use secrecy::SecretString;
use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyStore, Rule};
use ssh_manager_core::outgoing::{
    OutgoingKind, OutgoingTransform, Prepared, Rejection, ServerScope,
};
use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::events::TestEmitter;
use crate::orchestration::run_chat_turn;
use crate::test_support::log_capture;

use super::super::test_support::*;
use super::*;

/// Replaces every `X` by `Y`, optionally reporting `secret` as inserted
/// (`X` is then replaced by the secret instead), and records each call.
struct ReplaceX {
    replacement: String,
    secrets: Vec<SecretString>,
    calls: Mutex<Vec<(OutgoingKind, String)>>,
}

impl ReplaceX {
    fn new() -> Arc<Self> {
        Self::with_secret("Y", None)
    }

    fn with_secret(replacement: &str, secret: Option<&str>) -> Arc<Self> {
        Arc::new(Self {
            replacement: replacement.to_string(),
            secrets: secret
                .map(|s| vec![SecretString::from(s.to_string())])
                .unwrap_or_default(),
            calls: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl OutgoingTransform for ReplaceX {
    async fn prepare_outgoing(
        &self,
        _scope: &ServerScope,
        kind: OutgoingKind,
        text: &str,
    ) -> Result<Prepared, Rejection> {
        self.calls.lock().unwrap().push((kind, text.to_string()));
        Ok(Prepared::new(
            text.replace('X', &self.replacement),
            self.secrets.clone(),
        ))
    }
}

struct Reject;

#[async_trait]
impl OutgoingTransform for Reject {
    async fn prepare_outgoing(
        &self,
        _scope: &ServerScope,
        _kind: OutgoingKind,
        _text: &str,
    ) -> Result<Prepared, Rejection> {
        Err(Rejection {
            message: "no value available for this server".to_string(),
        })
    }
}

/// A faulty transform: turns any approved command into a blacklisted one.
struct TurnsIntoBlacklisted;

#[async_trait]
impl OutgoingTransform for TurnsIntoBlacklisted {
    async fn prepare_outgoing(
        &self,
        _scope: &ServerScope,
        _kind: OutgoingKind,
        _text: &str,
    ) -> Result<Prepared, Rejection> {
        Ok(Prepared::new("rm -rf /".to_string(), Vec::new()))
    }
}

fn propose(command: &str) -> Vec<AiEvent> {
    vec![
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: command.to_string(),
        }),
        AiEvent::Done,
    ]
}

/// Runs a chat turn and approves the confirmation if the action needs one
/// (a command the filter lets through needs none).
async fn run_approved_turn(session: &Session, emitter: &TestEmitter) {
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let turn = run_chat_turn(
        session,
        Uuid::new_v4(),
        emitter,
        &profile_store,
        &confirmations,
    );
    tokio::pin!(turn);
    let responder = async {
        loop {
            let id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    if name != "chat-action-proposed" {
                        return None;
                    }
                    payload.get("decision").and_then(|d| d.get("Confirm"))?;
                    Some(payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(id) = id {
                let action_id: ActionId = id.parse().unwrap();
                let _ = confirmations.resolve(&action_id, ActionUserDecision::Approve);
                return;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::pin!(responder);
    tokio::select! {
        _ = &mut turn => {}
        _ = &mut responder => { turn.await; }
    }
}

fn all_event_text(emitter: &TestEmitter) -> String {
    format!("{:?}", emitter.events.lock().unwrap())
}

#[tokio::test]
async fn test_transform_changes_what_the_server_receives_but_sinks_keep_the_original() {
    let transport = MockSshTransport::default().with_response("echo Y", output("done"));
    let executed = transport.executed_handle();
    let (mut session, _chat_store, chat_session_id, _tmp, ledger_store) =
        session_with_real_chat_and_ledger_persistence(propose("echo X"), transport).await;
    session.set_outgoing_transform_for_tests(Some(ReplaceX::new()));
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert_eq!(*executed.lock().unwrap(), vec!["echo Y".to_string()]);
    let events = all_event_text(&emitter);
    assert!(events.contains("echo X"), "{events}");
    assert!(!events.contains("echo Y"), "{events}");
    let history = format!("{:?}", session.context.lock().await.history);
    assert!(history.contains("echo X"), "{history}");
    assert!(!history.contains("echo Y"), "{history}");
    let ledger = format!(
        "{:?}",
        ledger_store.load_entries(chat_session_id).await.unwrap()
    );
    assert!(ledger.contains("echo X"), "{ledger}");
    assert!(!ledger.contains("echo Y"), "{ledger}");
}

#[test]
fn test_transformed_command_is_not_written_to_the_log() {
    log_capture::start_recording();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let transport =
            MockSshTransport::default().with_response("echo QQQ-prepared", output("ok"));
        let mut session = test_session_allowing_everything(propose("echo X"), transport);
        session.set_outgoing_transform_for_tests(Some(ReplaceX::with_secret("QQQ-prepared", None)));
        let emitter = TestEmitter::default();
        run_approved_turn(&session, &emitter).await;
        assert!(!all_event_text(&emitter).contains("QQQ-prepared"));
    });
    let log = log_capture::recorded_text();
    assert!(!log.contains("QQQ-prepared"), "{log}");
}

#[tokio::test]
async fn test_filter_sees_the_original_text_not_the_prepared_one() {
    struct DenyY;
    #[async_trait]
    impl PolicyStore for DenyY {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-y".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("*Y*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }
    let transport = MockSshTransport::default().with_response("echo Y", output("done"));
    let executed = transport.executed_handle();
    let mut session = test_session(propose("echo X"), transport);
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyY)));
    session.set_outgoing_transform_for_tests(Some(ReplaceX::new()));
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert_eq!(
        *executed.lock().unwrap(),
        vec!["echo Y".to_string()],
        "a rule that matches only the prepared text must not fire"
    );
}

#[tokio::test]
async fn test_edited_text_is_what_gets_transformed() {
    let transport = MockSshTransport::default().with_response("echo YY", output("done"));
    let executed = transport.executed_handle();
    let mut session = test_session(vec![AiEvent::Done], transport);
    let transform = ReplaceX::new();
    session.set_outgoing_transform_for_tests(Some(transform.clone()));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let action = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "echo a".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    );
    let responder = respond_to_first_proposed_action(
        &emitter,
        &confirmations,
        ActionUserDecision::EditThenApprove {
            command: "echo XX".to_string(),
        },
    );
    tokio::join!(action, responder);

    assert_eq!(*executed.lock().unwrap(), vec!["echo YY".to_string()]);
    assert_eq!(
        *transform.calls.lock().unwrap(),
        vec![(OutgoingKind::Command, "echo XX".to_string())]
    );
}

#[tokio::test]
async fn test_mcp_command_is_transformed() {
    let transport = MockSshTransport::default().with_response("echo Y", output("done"));
    let executed = transport.executed_handle();
    let mut session = test_session(vec![AiEvent::Done], transport);
    session.set_outgoing_transform_for_tests(Some(ReplaceX::new()));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let action = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "echo X".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Mcp { client_name: None },
        test_fresh_rejection_flag(),
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    tokio::join!(action, responder);

    assert_eq!(*executed.lock().unwrap(), vec!["echo Y".to_string()]);
}

#[tokio::test]
async fn test_write_remote_file_content_is_transformed_and_the_result_keeps_the_original() {
    let mut session = test_session_allowing_everything(
        vec![
            AiEvent::ActionProposed(AiAction::WriteRemoteFile {
                path: "/tmp/f".to_string(),
                content: "token=X".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    let transform = ReplaceX::new();
    session.set_outgoing_transform_for_tests(Some(transform.clone()));
    let sftp = MockSftpSession::new();
    session.set_sftp_for_tests(Box::new(sftp.clone())).await;
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert_eq!(sftp.file_content("/tmp/f"), Some(b"token=Y".to_vec()));
    assert_eq!(
        *transform.calls.lock().unwrap(),
        vec![(OutgoingKind::FileContent, "token=X".to_string())]
    );
    assert!(!all_event_text(&emitter).contains("token=Y"));
}

#[tokio::test]
async fn test_rejection_sends_nothing_and_tells_the_ai() {
    let transport = MockSshTransport::default();
    let executed = transport.executed_handle();
    let mut session = test_session_allowing_everything(propose("echo X"), transport);
    session.set_outgoing_transform_for_tests(Some(Arc::new(Reject)));
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert!(executed.lock().unwrap().is_empty());
    let history = format!("{:?}", session.context.lock().await.history);
    assert!(
        history.contains("no value available for this server"),
        "{history}"
    );
}

#[tokio::test]
async fn test_rejection_of_file_content_writes_nothing() {
    let mut session = test_session_allowing_everything(
        vec![
            AiEvent::ActionProposed(AiAction::WriteRemoteFile {
                path: "/tmp/f".to_string(),
                content: "x".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    session.set_outgoing_transform_for_tests(Some(Arc::new(Reject)));
    let sftp = MockSftpSession::new();
    session.set_sftp_for_tests(Box::new(sftp.clone())).await;
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert_eq!(sftp.file_content("/tmp/f"), None);
}

#[tokio::test]
async fn test_inserted_secret_is_masked_in_the_output_of_its_own_command() {
    let transport = MockSshTransport::default().with_response(
        "echo hunter-two-secret",
        output("the value is hunter-two-secret\n"),
    );
    let mut session = test_session_allowing_everything(propose("echo X"), transport);
    session.set_outgoing_transform_for_tests(Some(ReplaceX::with_secret(
        "hunter-two-secret",
        Some("hunter-two-secret"),
    )));
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    let events = all_event_text(&emitter);
    assert!(!events.contains("hunter-two-secret"), "{events}");
    assert!(events.contains("[REDACTED]"), "{events}");
    let history = format!("{:?}", session.context.lock().await.history);
    assert!(!history.contains("hunter-two-secret"), "{history}");
}

#[tokio::test]
async fn test_sudo_insertion_composes_with_the_transform() {
    let transport =
        MockSshTransport::default().with_response("sudo -S systemctl restart Y", output("done"));
    let stdin_calls = transport.stdin_calls_handle();
    let mut session =
        test_session_allowing_everything(propose("sudo systemctl restart X"), transport);
    session.set_sudo_password_for_tests(Some(SecretString::from("hunter2".to_string())));
    session.set_outgoing_transform_for_tests(Some(ReplaceX::new()));
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    let calls = stdin_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "sudo -S systemctl restart Y");
    assert_eq!(calls[0].1, b"hunter2\n".to_vec());
    let events = all_event_text(&emitter);
    assert!(events.contains("sudo -S systemctl restart X"), "{events}");
    assert!(!events.contains("restart Y"), "{events}");
}

#[tokio::test]
async fn test_prepared_command_on_the_hard_blacklist_is_rejected_without_quoting_it() {
    let transport = MockSshTransport::default();
    let executed = transport.executed_handle();
    let mut session = test_session_allowing_everything(propose("echo X"), transport);
    session.set_outgoing_transform_for_tests(Some(Arc::new(TurnsIntoBlacklisted)));
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert!(
        executed.lock().unwrap().is_empty(),
        "nothing may be sent when the prepared command is blacklisted"
    );
    let shown = format!(
        "{} {:?}",
        all_event_text(&emitter),
        session.context.lock().await.history
    );
    assert!(shown.contains("hard blacklist"), "{shown}");
    assert!(!shown.contains("rm -rf"), "{shown}");
}

#[tokio::test]
async fn test_without_a_transform_the_text_is_sent_unchanged() {
    let transport = MockSshTransport::default().with_response("echo X", output("done"));
    let executed = transport.executed_handle();
    let session = test_session_allowing_everything(propose("echo X"), transport);
    let emitter = TestEmitter::default();

    run_approved_turn(&session, &emitter).await;

    assert_eq!(*executed.lock().unwrap(), vec!["echo X".to_string()]);
}

// --- Issue #501: the transform gets the group chain of the target server ---

mod group_chain_scope {
    use chrono::Utc;
    use ssh_manager_core::profiles::{AuthMethod, Group, GroupId, PostIngestPolicy, Server};
    use ssh_manager_core::shared::ServerId;

    use super::*;
    use crate::dto::LOCAL_SERVER_ID;
    use crate::orchestration::outgoing::prepare_for_send;
    use crate::session_extension::{scope_for_id, ServerScope};
    use crate::test_support::InMemoryProfileStore;

    /// Records the scope of every call.
    struct ScopeRecorder(Mutex<Vec<ServerScope>>);

    #[async_trait]
    impl OutgoingTransform for ScopeRecorder {
        async fn prepare_outgoing(
            &self,
            scope: &ServerScope,
            _kind: OutgoingKind,
            text: &str,
        ) -> Result<Prepared, Rejection> {
            self.0.lock().unwrap().push(scope.clone());
            Ok(Prepared::new(text.to_string(), Vec::new()))
        }
    }

    fn group(parent: Option<GroupId>) -> Group {
        let now = Utc::now();
        Group {
            id: GroupId::new(),
            name: "g".to_string(),
            parent_id: parent,
            notes: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn server(group_id: Option<GroupId>) -> Server {
        let now = Utc::now();
        Server {
            id: ServerId::new(),
            name: "web".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id,
            tags: vec!["prod".to_string()],
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

    fn session_for(server_id: ServerId, recorder: Arc<ScopeRecorder>) -> Session {
        let mut session = test_session_allowing_everything(vec![], MockSshTransport::default());
        session.set_server_id_for_tests(server_id);
        session.set_outgoing_transform_for_tests(Some(recorder));
        session
    }

    #[tokio::test]
    async fn test_nested_groups_give_root_first_chain_equal_to_session_extension() {
        let root = group(None);
        let mid = group(Some(root.id));
        let leaf = group(Some(mid.id));
        let srv = server(Some(leaf.id));
        let store = InMemoryProfileStore::new()
            .with_group(root.clone())
            .with_group(mid.clone())
            .with_group(leaf.clone())
            .with_server(srv.clone());
        let recorder = Arc::new(ScopeRecorder(Mutex::new(Vec::new())));
        let session = session_for(srv.id, recorder.clone());

        prepare_for_send(&session, &store, OutgoingKind::Command, "echo")
            .await
            .unwrap();

        let seen = recorder.0.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].group_chain, vec![root.id, mid.id, leaf.id]);
        assert_eq!(seen[0].tags, vec!["prod".to_string()]);
        let extension_scope = scope_for_id(&store, srv.id, None).await.unwrap();
        assert_eq!(seen[0], extension_scope);
    }

    #[tokio::test]
    async fn test_server_without_group_has_empty_chain() {
        let srv = server(None);
        let store = InMemoryProfileStore::new().with_server(srv.clone());
        let recorder = Arc::new(ScopeRecorder(Mutex::new(Vec::new())));
        let session = session_for(srv.id, recorder.clone());

        prepare_for_send(&session, &store, OutgoingKind::Command, "echo")
            .await
            .unwrap();

        assert!(recorder.0.lock().unwrap()[0].group_chain.is_empty());
    }

    #[tokio::test]
    async fn test_local_pseudo_server_has_empty_chain() {
        let store = InMemoryProfileStore::new();
        let recorder = Arc::new(ScopeRecorder(Mutex::new(Vec::new())));
        let session = session_for(LOCAL_SERVER_ID, recorder.clone());

        prepare_for_send(&session, &store, OutgoingKind::Command, "echo")
            .await
            .unwrap();

        let seen = recorder.0.lock().unwrap().clone();
        assert_eq!(seen[0].server_id, LOCAL_SERVER_ID);
        assert!(seen[0].group_chain.is_empty());
    }

    /// The group does not exist in the store, so the lookup fails: the chain
    /// is empty and the text is still prepared.
    #[tokio::test]
    async fn test_failing_group_lookup_gives_empty_chain_and_text_is_prepared() {
        let srv = server(Some(GroupId::new()));
        let store = InMemoryProfileStore::new().with_server(srv.clone());
        let recorder = Arc::new(ScopeRecorder(Mutex::new(Vec::new())));
        let session = session_for(srv.id, recorder.clone());

        let prepared = prepare_for_send(&session, &store, OutgoingKind::Command, "echo")
            .await
            .unwrap();

        assert!(prepared.is_some());
        assert!(recorder.0.lock().unwrap()[0].group_chain.is_empty());
    }

    /// Without a transform nothing is looked up: the failing lookup of the
    /// test above would log a warning, which must not appear here.
    #[test]
    fn test_without_transform_no_group_lookup_happens() {
        log_capture::start_recording();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let srv = server(Some(GroupId::new()));
            let store = InMemoryProfileStore::new().with_server(srv.clone());
            let mut session = test_session_allowing_everything(vec![], MockSshTransport::default());
            session.set_server_id_for_tests(srv.id);
            let out = prepare_for_send(&session, &store, OutgoingKind::Command, "echo")
                .await
                .unwrap();
            assert!(out.is_none());
        });
        let log = log_capture::recorded_text();
        assert!(!log.contains("group_chain failed"), "{log}");
    }
}
