use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::Mutex as AsyncMutex;

use chrono::Utc;

use ssh_manager_core::ai::{
    default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, RedactedContext,
    SessionContext,
};
use ssh_manager_core::filter::{FilterEngine, PolicyRules};
use ssh_manager_core::profiles::Server;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize, SshError, SshTransport};

use crate::first_run_notice::test_support::{lock_async, test_app};
use app_logic::confirmation::ConfirmationRegistry;
use app_logic::events::TestEmitter;
use app_logic::session::SessionParts;
use app_logic::test_support::InMemoryProfileStore;

use super::*;

/// Nie tatsächlich aufgerufen — dieser Test führt kein Kommando aus,
/// die KI schlägt keins vor (s. `NoopAiProvider`).
struct UnusedTransport;
#[async_trait]
impl SshTransport for UnusedTransport {
    async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
        unreachable!("dieser Test ruft SshTransport::execute nie auf")
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!("dieser Test ruft SshTransport::open_shell nie auf")
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

struct NoopAiProvider;
impl AiProvider for NoopAiProvider {
    fn send(
        &self,
        _context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![AiEvent::Done]))
    }
}

fn test_session(server_id: ServerId) -> Session {
    Session::new(SessionParts {
        transport: app_logic::session::SessionTransport::new(Box::new(UnusedTransport)),
        server_id,
        tags: Vec::new(),
        redactor: app_logic::session::SwappableRedactor::new(
            Box::new(DefaultOutputRedactor::new()),
        ),
        ai: app_logic::session::AiConfig {
            provider: Box::new(NoopAiProvider),
            provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
            context: AsyncMutex::new(SessionContext {
                system_context: String::new(),
                history: Vec::new(),
                available_actions: default_action_schemas(),
                max_tokens_hint: None,
            }),
            label: "test-provider".to_string(),
            model: "test-model".to_string(),
            system_context_parts: AsyncMutex::new(
                app_logic::compaction::SystemContextParts::default(),
            ),
            model_context_window_tokens: usize::MAX / 1_000,
            summary: AsyncMutex::new(None),
            context_usage: Default::default(),
            request_paced_at: AsyncMutex::new(None),
        },
        safety: app_logic::session::SafetyState {
            filter_engine: Box::new(FilterEngine::new(app_logic::policy::NoRulesPolicyStore)),
            mcp_origin_flags: std::sync::Mutex::new(Vec::new()),
            sudo_password: app_logic::session::SwappableSudoPassword::new(None),
            mcp_confirmation_claim: std::sync::atomic::AtomicBool::new(false),
            risk_second_opinion_provider: None,
            risk_second_opinion_budget: None,
            red_risk_always_confirm: true,
            outgoing_transform: None,
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            injection_check_provider: None,
            injection_check_budget: None,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            injection_suspected_reason: std::sync::Mutex::new(None),
            injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
            second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(false),
            injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(false),
            redaction_incomplete_notice_pending: std::sync::atomic::AtomicBool::new(false),
        },
        persistence: app_logic::session::ChatPersistence {
            chat_content: Default::default(),
            chat_session_store: None,
            ledger_store: None,
            chat_session_id: AsyncMutex::new(None),
        },
        conn: app_logic::session::TransportState {
            terminal: std::sync::Mutex::new(None),
            status: std::sync::Mutex::new(app_logic::events::ConnectionStatus::Connected),
            pending_action: std::sync::Mutex::new(None),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop_notify: tokio::sync::Notify::new(),
            chat_turn: std::sync::Mutex::new(app_logic::session::ChatTurnState::default()),
            // Spec 0092, A1.2: default on, as in the app
            running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
            auto_reconnect: Default::default(),
        },
    })
}

/// Baut eine echte, migrierte temporäre SQLite-DB samt `servers`-Zeile
/// und daran gebundenem `SqliteChatSessionStore` — derselbe Aufbau wie
/// `orchestration::tests::session_with_real_chat_persistence`
/// (dortiges Modul ist nicht von hier erreichbar, daher lokal
/// nachgebaut statt geteilt — reines Test-Setup, keine Produktionslogik).
async fn session_with_real_persistence() -> (
    Session,
    persistence_sqlite::SqliteProfileStore,
    persistence_sqlite::SqliteChatSessionStore,
    tempfile::TempDir,
) {
    let tmp_dir = tempfile::tempdir().expect("TempDir konnte nicht angelegt werden");
    let db_path = tmp_dir.path().join("test.sqlite3");
    let profile_store = persistence_sqlite::SqliteProfileStore::connect_plaintext(&db_path)
        .await
        .expect("frische DB sollte immer aufbaubar sein");

    let server_id = ServerId::new();
    let now = Utc::now();
    profile_store
        .create_server(&Server {
            id: server_id,
            name: "Test-Server".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: ssh_manager_core::profiles::AuthMethod::Agent,
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            start_directory: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();

    let chat_store = profile_store.chat_session_store();
    let chat_session_id = chat_store.create_session(&server_id, None).await.unwrap();

    let mut session = test_session(server_id);
    session.set_chat_session_store_for_tests(Some(chat_store.clone()));
    session.set_chat_session_id_for_tests(AsyncMutex::new(Some(chat_session_id)));

    (session, profile_store, chat_store, tmp_dir)
}

/// Der eigentliche Regressionstest: ruft `send_chat_message_impl`
/// direkt auf (genau die Funktion, die vorher den Nutzertext nur in
/// den In-Memory-Kontext schrieb) und prüft, dass die Nachricht
/// tatsächlich in `chat_messages` landet — nicht nur in
/// `session.context`.
#[tokio::test]
async fn test_send_chat_message_persists_user_text_via_push_history() {
    let _guard = lock_async().await;
    let (session, profile_store, chat_store, _tmp_dir) = session_with_real_persistence().await;
    let chat_session_id = session.persistence.chat_session_id.lock().await.unwrap();
    let app = test_app();
    let handle = app.handle();
    let emitter = TestEmitter::default();
    let in_memory_profile_store = InMemoryProfileStore::default();
    let policy_store = profile_store.policy_store();
    let confirmations = ConfirmationRegistry::new();

    send_chat_message_impl(
        handle,
        &emitter,
        &session,
        uuid::Uuid::new_v4(),
        "räum mal /tmp auf".to_string(),
        Some(&profile_store.prompt_history_store()),
        &in_memory_profile_store,
        &policy_store,
        &confirmations,
    )
    .await
    .unwrap();

    let loaded = chat_store.load_session(chat_session_id).await.unwrap();
    assert!(
        loaded.iter().any(|m| matches!(
            &m.content,
            MessageContent::Text(t) if t == "räum mal /tmp auf"
        ) && m.role == Role::User),
        "die Nutzer-Nachricht muss in chat_messages persistiert sein, geladen: {loaded:?}"
    );
}

/// Issue #600: "Try again" runs a turn over the existing history; the
/// user message is not added a second time.
#[tokio::test]
async fn test_retry_chat_turn_does_not_duplicate_user_message() {
    let _guard = lock_async().await;
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let gate = Arc::new(tokio::sync::Notify::new());
    gate.notify_one();
    let mut session = test_session(ServerId::new());
    session.set_ai_provider_for_tests(Box::new(GatedRecordingProvider {
        gate,
        contexts: contexts.clone(),
    }));
    session.ai.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text("hello".to_string()),
    });
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    retry_chat_turn_impl(
        app.handle(),
        &TestEmitter::default(),
        &session,
        uuid::Uuid::new_v4(),
        &InMemoryProfileStore::default(),
        &policy_store,
        &ConfirmationRegistry::new(),
    )
    .await
    .unwrap();
    {
        let sent = contexts.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(history_texts(&sent[0]), vec!["hello".to_string()]);
    }

    // The history now ends with something else (the turn ran): refused.
    session.ai.context.lock().await.history.push(ChatMessage {
        role: Role::Assistant,
        content: MessageContent::Text("partial".to_string()),
    });
    let err = retry_chat_turn_impl(
        app.handle(),
        &TestEmitter::default(),
        &session,
        uuid::Uuid::new_v4(),
        &InMemoryProfileStore::default(),
        &policy_store,
        &ConfirmationRegistry::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, Some(app_logic::orchestration::RETRY_NOT_POSSIBLE));
    assert_eq!(contexts.lock().unwrap().len(), 1);
}

/// Spec 0066, §2: erster `send()` hängt, bis `gate` geöffnet wird; jeder
/// weitere liefert sofort `Done`. Zeichnet jeden gesendeten Kontext auf.
struct GatedRecordingProvider {
    gate: Arc<tokio::sync::Notify>,
    contexts: Arc<std::sync::Mutex<Vec<SessionContext>>>,
}
impl AiProvider for GatedRecordingProvider {
    fn send(
        &self,
        context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let mut contexts = self.contexts.lock().unwrap();
        let first = contexts.is_empty();
        contexts.push(context.into_parts());
        if first {
            let gate = self.gate.clone();
            Box::pin(futures::stream::once(async move {
                gate.notified().await;
                AiEvent::Done
            }))
        } else {
            Box::pin(futures::stream::iter(vec![AiEvent::Done]))
        }
    }
}

fn history_texts(context: &SessionContext) -> Vec<String> {
    context
        .history
        .iter()
        .filter_map(|m| match (&m.role, &m.content) {
            (Role::User, MessageContent::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// Issue #315: eine Nachricht, die eintrifft, während „Neuer Chat" den
/// Turn-Slot hält, wird abgelehnt und nicht eingereiht — sonst hinge sie
/// unbemerkt an der ersten Anfrage des neuen Chats.
#[tokio::test]
async fn test_message_sent_during_new_chat_swap_is_rejected_not_queued() {
    let _guard = lock_async().await;
    let session = test_session(ServerId::new());
    {
        let mut turn = session.conn.chat_turn.lock().unwrap();
        turn.running = true;
        turn.swapping = true;
    }
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    let err = send_chat_message_impl(
        app.handle(),
        &TestEmitter::default(),
        &session,
        uuid::Uuid::new_v4(),
        "während des Wechsels".to_string(),
        None,
        &InMemoryProfileStore::default(),
        &policy_store,
        &ConfirmationRegistry::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, Some(app_logic::orchestration::CHAT_SWITCHING));
    let turn = session.conn.chat_turn.lock().unwrap();
    assert!(turn.queued.is_empty(), "nothing may leak into the new chat");
    assert!(turn.running, "the swap's claim stays untouched");
}

/// Issue #505: extension content while a turn runs is rejected with
/// `ATTACHMENTS_WHILE_BUSY`, nothing is queued and nothing is persisted.
#[tokio::test]
async fn test_extension_content_while_turn_running_is_rejected() {
    use app_logic::chat_content_extension::{
        ChatContentExtension, ChatContentRegistry, PendingContent,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Ext(std::sync::Arc<AtomicBool>);
    #[async_trait::async_trait]
    impl ChatContentExtension for Ext {
        fn name(&self) -> &str {
            "files"
        }
        fn take_pending(&self, _s: uuid::Uuid, _id: &str) -> Option<PendingContent> {
            Some(PendingContent {
                label: "a.txt".into(),
                kind: "doc".into(),
                blocks: vec![ssh_manager_core::ai::ResolvedBlock::Text { text: "x".into() }],
            })
        }
        async fn persist(
            &self,
            _s: uuid::Uuid,
            _p: &str,
            _b: Vec<ssh_manager_core::ai::RedactedBlock>,
        ) -> Result<(), app_logic::chat_content_extension::ChatContentError> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    let _guard = lock_async().await;
    let session = test_session(ServerId::new());
    session.conn.chat_turn.lock().unwrap().running = true;
    let persisted = std::sync::Arc::new(AtomicBool::new(false));
    let registry = ChatContentRegistry::new(vec![std::sync::Arc::new(Ext(persisted.clone()))]);
    let parts = app_logic::orchestration::accept_extension_content(
        &registry,
        session.redactor.as_ref(),
        uuid::Uuid::new_v4(),
        &["id".to_string()],
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    let err = send_chat_message_with_attachments_impl(
        app.handle(),
        &TestEmitter::default(),
        &session,
        uuid::Uuid::new_v4(),
        "hi".to_string(),
        parts,
        None,
        &InMemoryProfileStore::default(),
        &policy_store,
        &ConfirmationRegistry::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        err.code,
        Some(app_logic::orchestration::ATTACHMENTS_WHILE_BUSY)
    );
    assert!(session.conn.chat_turn.lock().unwrap().queued.is_empty());
    assert!(!persisted.load(Ordering::SeqCst));
}

/// Spec 0066, §2: eine Nachricht, die während eines laufenden Turns
/// gesendet wird, kehrt sofort zurück (Eingabe nie blockiert), wird
/// eingereiht und nach Turn-Ende als normale Nachricht gesendet — über
/// denselben Pfad, also auch mit Redaction vor dem Versand.
#[tokio::test]
#[expect(clippy::large_futures, reason = "test future built inline; not boxed")]
async fn test_message_sent_during_running_turn_is_queued_then_sent_redacted() {
    let _guard = lock_async().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut session = test_session(ServerId::new());
    session.set_ai_provider_for_tests(Box::new(GatedRecordingProvider {
        gate: gate.clone(),
        contexts: contexts.clone(),
    }));
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    let handle = app.handle();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = uuid::Uuid::new_v4();

    let first = send_chat_message_impl(
        handle,
        &emitter,
        &session,
        session_id,
        "erste Frage".to_string(),
        None,
        &profile_store,
        &policy_store,
        &confirmations,
    );
    let second = async {
        while contexts.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            send_chat_message_impl(
                handle,
                &emitter,
                &session,
                session_id,
                "Korrektur: nimm AKIAABCDEFGHIJKLMNOP nicht".to_string(),
                None,
                &profile_store,
                &policy_store,
                &confirmations,
            ),
        )
        .await
        .expect("Senden während eines laufenden Turns darf nicht blockieren")
        .unwrap();
        assert_eq!(
            contexts.lock().unwrap().len(),
            1,
            "die eingereihte Nachricht darf den laufenden Request nicht unterbrechen"
        );
        gate.notify_waiters();
    };
    let (first_result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(first, second)
    })
    .await
    .expect("beide Turns müssen enden");
    first_result.unwrap();

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "eingereihte Nachricht → genau ein Folge-Request"
    );
    let sent = history_texts(&contexts[1]);
    assert!(
        sent.iter().any(|t| t.starts_with("Korrektur: nimm")),
        "eingereihte Nachricht muss im Folge-Request stehen: {sent:?}"
    );
    assert!(
        !sent.iter().any(|t| t.contains("AKIAABCDEFGHIJKLMNOP")),
        "Secret in eingereihter Nachricht muss vor dem Versand redigiert sein: {sent:?}"
    );
    assert!(emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|(name, _)| name == "chat-queued-messages-sent"));
    let turn = session.conn.chat_turn.lock().unwrap();
    assert!(!turn.running && turn.queued.is_empty());
}

struct AllowEverythingPolicyStore;
#[async_trait]
impl ssh_manager_core::filter::PolicyStore for AllowEverythingPolicyStore {
    async fn rules_for(&self, _scope: &ssh_manager_core::filter::EffectiveScope) -> PolicyRules {
        PolicyRules::complete(vec![ssh_manager_core::filter::Rule {
            id: ssh_manager_core::filter::RuleId("allow-all".to_string()),
            pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
            action: ssh_manager_core::filter::RuleAction::Allow,
            scope: ssh_manager_core::filter::Scope::Global,
            priority: 0,
            origin: ssh_manager_core::filter::RuleOrigin::User,
        }])
    }
}

/// Führt jedes Kommando sofort mit fester Ausgabe aus.
struct EchoTransport;
#[async_trait]
impl SshTransport for EchoTransport {
    async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
        Ok(CommandOutput {
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
            truncated: false,
        })
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!()
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

/// Runde 1 hängt bis `gate`, schlägt dann ein Kommando vor; jeder
/// weitere Request liefert sofort `Done`.
struct GatedActionProvider {
    gate: Arc<tokio::sync::Notify>,
    contexts: Arc<std::sync::Mutex<Vec<SessionContext>>>,
}
impl AiProvider for GatedActionProvider {
    fn send(
        &self,
        context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let mut contexts = self.contexts.lock().unwrap();
        let first = contexts.is_empty();
        contexts.push(context.into_parts());
        if first {
            let gate = self.gate.clone();
            Box::pin(
                futures::stream::once(async move {
                    gate.notified().await;
                    futures::stream::iter(vec![
                        AiEvent::ActionProposed(
                            ssh_manager_core::profiles::AiAction::SuggestCommand {
                                command: "echo eins".to_string(),
                            },
                        ),
                        AiEvent::Done,
                    ])
                })
                .flatten(),
            )
        } else {
            Box::pin(futures::stream::iter(vec![AiEvent::Done]))
        }
    }
}

/// Spec 0066, spec-reviewer-Fund: wird eine eingereihte Nachricht an der
/// Rundengrenze in den Verlauf gelegt und der Request danach (vor dem
/// Versand) gestoppt, muss sie trotzdem noch beantwortet werden — per
/// Folge-Turn.
#[tokio::test]
#[expect(clippy::large_futures, reason = "test future built inline; not boxed")]
async fn test_queued_message_injected_then_stopped_before_send_is_still_answered() {
    let _guard = lock_async().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut session = test_session(ServerId::new());
    session.set_transport_for_tests(app_logic::session::SessionTransport::new(Box::new(
        EchoTransport,
    )));
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
    session.set_ai_provider_for_tests(Box::new(GatedActionProvider {
        gate: gate.clone(),
        contexts: contexts.clone(),
    }));
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    let handle = app.handle();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = uuid::Uuid::new_v4();

    let first = send_chat_message_impl(
        handle,
        &emitter,
        &session,
        session_id,
        "erste".to_string(),
        None,
        &profile_store,
        &policy_store,
        &confirmations,
    );
    let driver = async {
        while contexts.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        send_chat_message_impl(
            handle,
            &emitter,
            &session,
            session_id,
            "Korrektur".to_string(),
            None,
            &profile_store,
            &policy_store,
            &confirmations,
        )
        .await
        .unwrap();
        gate.notify_waiters();
        // Sobald die Nachricht an der Rundengrenze übergeben ist, steckt
        // Runde 2 in der Wartezeit vor dem Send (Pacing) — genau dann
        // Stopp.
        while !emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "chat-queued-messages-sent")
        {
            tokio::task::yield_now().await;
        }
        session.request_auto_continue_stop();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(first, driver)
    })
    .await
    .expect("Turns müssen enden");
    result.unwrap();

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "Runde 2 wurde gestoppt, die Korrektur braucht trotzdem einen Request"
    );
    assert!(history_texts(&contexts[1]).iter().any(|t| t == "Korrektur"));
    assert!(!session.conn.chat_turn.lock().unwrap().running);
}

/// Spec 0066, spec-reviewer-Fund: ein Stopp, der vor dem eigentlichen
/// Rundenbeginn eintrifft, darf nicht verschluckt werden — `run_chat_turn`
/// setzt das Flag nicht mehr selbst zurück.
#[tokio::test]
async fn test_run_chat_turn_does_not_swallow_an_early_stop() {
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut session = test_session(ServerId::new());
    session.set_ai_provider_for_tests(Box::new(GatedRecordingProvider {
        gate: Arc::new(tokio::sync::Notify::new()),
        contexts: contexts.clone(),
    }));
    session.request_auto_continue_stop();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_chat_turn(
            &session,
            uuid::Uuid::new_v4(),
            &emitter,
            &profile_store,
            &confirmations,
        ),
    )
    .await
    .expect("ein bereits gesetzter Stopp muss den Turn sofort beenden");

    assert!(
        contexts.lock().unwrap().is_empty(),
        "nach einem früh gedrückten Stopp darf kein Request rausgehen"
    );
}

/// Gegenstück: eine neue Nachricht nach einem alten Stopp läuft normal —
/// `send_chat_message_impl` setzt das Flag beim Turn-Start zurück.
#[tokio::test]
async fn test_new_message_after_an_old_stop_runs_normally() {
    let _guard = lock_async().await;
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut session = test_session(ServerId::new());
    let gate = Arc::new(tokio::sync::Notify::new());
    session.set_ai_provider_for_tests(Box::new(GatedRecordingProvider {
        gate: gate.clone(),
        contexts: contexts.clone(),
    }));
    session.request_auto_continue_stop();
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let send = send_chat_message_impl(
        app.handle(),
        &emitter,
        &session,
        uuid::Uuid::new_v4(),
        "neue Frage".to_string(),
        None,
        &profile_store,
        &policy_store,
        &confirmations,
    );
    let opener = async {
        while contexts.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        gate.notify_waiters();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(send, opener)
    })
    .await
    .expect("Turn muss enden");
    result.unwrap();
    assert_eq!(contexts.lock().unwrap().len(), 1);
}

// --- Issue #356: AI continuation after an automatic reconnect ----------

/// Proposes `echo next` as soon as the connection-restored note is the
/// last history entry; otherwise just ends the round.
struct ProposeAfterNoteProvider {
    contexts: Arc<std::sync::Mutex<Vec<SessionContext>>>,
}
impl AiProvider for ProposeAfterNoteProvider {
    fn send(
        &self,
        context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let mut contexts = self.contexts.lock().unwrap();
        let context = context.into_parts();
        let last = history_texts(&context).pop().unwrap_or_default();
        let after_note = contexts.is_empty() && last.contains("wiederhergestellt");
        contexts.push(context);
        if after_note {
            Box::pin(futures::stream::iter(vec![
                AiEvent::ActionProposed(ssh_manager_core::profiles::AiAction::SuggestCommand {
                    command: "echo next".to_string(),
                }),
                AiEvent::Done,
            ]))
        } else {
            Box::pin(futures::stream::iter(vec![AiEvent::Done]))
        }
    }
}

#[tokio::test]
async fn test_ai_gets_the_note_after_reconnect_and_its_next_command_needs_confirmation() {
    let _guard = lock_async().await;
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut session = test_session(ServerId::new());
    // `UnusedTransport` panics if a command is executed: the proposal
    // must wait for the user instead.
    session.set_ai_provider_for_tests(Box::new(ProposeAfterNoteProvider {
        contexts: contexts.clone(),
    }));
    let dir = tempfile::tempdir().unwrap();
    let policy_store =
        persistence_sqlite::SqliteProfileStore::connect_plaintext(&dir.path().join("t.db"))
            .await
            .unwrap()
            .policy_store();
    let app = test_app();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = uuid::Uuid::new_v4();

    // A turn was active at the drop.
    session.conn.auto_reconnect.set_ai_turn_interrupted(true);
    let note = crate::auto_reconnect::continuation_note(&session)
        .await
        .expect("an interrupted turn leads to a note");
    assert!(note.contains("wiederhergestellt"), "{note}");
    // At most once per drop.
    assert!(crate::auto_reconnect::continuation_note(&session)
        .await
        .is_none());

    let send = send_chat_message_impl(
        app.handle(),
        &emitter,
        &session,
        session_id,
        note.clone(),
        None,
        &profile_store,
        &policy_store,
        &confirmations,
    );
    let observe = async {
        // The proposal arrives and waits for the user's confirmation.
        let id = loop {
            let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
                (n == "chat-action-proposed" && p["decision"].get("Confirm").is_some())
                    .then(|| p["actionId"].as_str().unwrap().to_string())
            });
            if let Some(id) = pending {
                break id;
            }
            tokio::task::yield_now().await;
        };
        // The user denies; `UnusedTransport` would panic on any execution.
        let _ = confirmations.resolve(&id.parse().unwrap(), ActionUserDecision::Deny);
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(send, observe)
    })
    .await
    .expect("the turn must end");
    result.unwrap();

    let contexts = contexts.lock().unwrap().clone();
    assert!(
        history_texts(&contexts[0]).contains(&note),
        "the AI request carries the note"
    );
}

#[tokio::test]
async fn test_no_note_without_an_interrupted_turn_or_after_stop() {
    let session = test_session(ServerId::new());
    // No AI turn was active at the drop: nothing is sent.
    session.conn.auto_reconnect.set_ai_turn_interrupted(false);
    assert!(crate::auto_reconnect::continuation_note(&session)
        .await
        .is_none());
    // A turn was active, but the user pressed Stop during the drop.
    session.conn.auto_reconnect.set_ai_turn_interrupted(true);
    session.request_auto_continue_stop();
    assert!(crate::auto_reconnect::continuation_note(&session)
        .await
        .is_none());
}
