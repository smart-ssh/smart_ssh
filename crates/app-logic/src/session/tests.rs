//! Unit tests for `session`.

use std::pin::Pin;

use futures::Stream;
use uuid::Uuid;

use ssh_manager_core::ai::{
    default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, RejectionReason, Role,
    SessionContext,
};
use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, SshError};

use super::*;
use crate::policy::NoRulesPolicyStore;

/// Issue #464: a secret value added after the session was built is masked
/// in output and text, survives a swap of the inner redactor, and an empty
/// value does not mask everything.
#[test]
fn test_added_secret_values_are_masked_and_survive_a_swap() {
    let redactor = SwappableRedactor::new(Box::new(DefaultOutputRedactor::new()));
    let text = "value=plain-secret-42 end";
    assert_eq!(
        redactor.redact_text(text),
        text,
        "unknown before it is added"
    );

    redactor.add_secret_values(&[
        secrecy::SecretString::from("plain-secret-42".to_string()),
        secrecy::SecretString::from(String::new()),
    ]);
    assert_eq!(redactor.redact_text(text), "value=[REDACTED] end");
    let output = CommandOutput {
        stdout: b"a plain-secret-42 b".to_vec(),
        stderr: b"plain-secret-42".to_vec(),
        exit_code: Some(0),
        truncated: false,
    };
    let redacted = redactor.redact(&output);
    assert_eq!(redacted.stdout, b"a [REDACTED] b".to_vec());
    assert_eq!(redacted.stderr, b"[REDACTED]".to_vec());
    assert_eq!(redacted.exit_code, Some(0));

    redactor.replace(Box::new(DefaultOutputRedactor::new()));
    assert_eq!(redactor.redact_text(text), "value=[REDACTED] end");
}

/// Wird in diesen Tests nie tatsächlich aufgerufen (nur die
/// `SessionManager`-Buchführung selbst steht hier auf dem Prüfstand,
/// nicht der Chat-/Terminal-Ablauf, s. `crate::orchestration`s Tests
/// dafür) — `unreachable!` statt einer stillen Fake-Antwort macht einen
/// versehentlichen Aufruf sofort sichtbar.
struct UnusedAiProvider;
impl AiProvider for UnusedAiProvider {
    fn send(&self, _ctx: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
        unreachable!("dieser Test ruft AiProvider::send nie auf")
    }
}

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

fn dummy_session(server_id: ServerId) -> Session {
    Session::new(SessionParts {
        transport: SessionTransport::new(Box::new(UnusedTransport)),
        ai_provider: Box::new(UnusedAiProvider),
        ai_provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
        context: AsyncMutex::new(SessionContext {
            system_context: String::new(),
            history: Vec::new(),
            available_actions: default_action_schemas(),
            max_tokens_hint: None,
        }),
        filter_engine: Box::new(FilterEngine::new(NoRulesPolicyStore)),
        server_id,
        tags: Vec::new(),
        terminal: StdMutex::new(None),
        redactor: SwappableRedactor::new(Box::new(DefaultOutputRedactor::new())),
        ai_provider_label: "test-provider".to_string(),
        ai_model: "test-model".to_string(),
        system_context_parts: AsyncMutex::new(crate::compaction::SystemContextParts::default()),
        // Absichtlich riesig: Tests sollen nie versehentlich in die
        // Etappe-2-Kompaktierung laufen, außer sie setzen diesen Wert
        // explizit klein (s. `orchestration::tests`, `compaction`-Modul
        // für die dedizierten Kompaktierungs-Tests).
        model_context_window_tokens: usize::MAX / 1_000,
        summary: AsyncMutex::new(None),
        context_usage: Default::default(),
        mcp_origin_flags: StdMutex::new(Vec::new()),
        sudo_password: SwappableSudoPassword::new(None),
        status: StdMutex::new(ConnectionStatus::Connected),
        pending_action: StdMutex::new(None),
        mcp_confirmation_claim: std::sync::atomic::AtomicBool::new(false),
        auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
        auto_continue_stop_notify: tokio::sync::Notify::new(),
        chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
        risk_second_opinion_provider: None,
        risk_second_opinion_budget: None,
        // Spec 0092, A1.2: in Test-Fixtures wie in der App standardmäßig
        // „an".
        red_risk_always_confirm: true,
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        outgoing_transform: None,
        chat_content: Arc::default(),
        untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
        post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
        injection_check_provider: None,
        injection_check_budget: None,
        injection_suspected: std::sync::atomic::AtomicBool::new(false),
        injection_suspected_reason: std::sync::Mutex::new(None),
        injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
        second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(false),
        injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(false),
        chat_session_store: None,
        ledger_store: None,
        chat_session_id: AsyncMutex::new(None),
        ai_request_paced_at: AsyncMutex::new(None),
        auto_reconnect: Default::default(),
    })
}

/// Spec 0038, section 2a: after an extension triggers a rebuild, a
/// newly added secret is masked by the open session of that server only.
#[tokio::test]
async fn test_rebuild_server_redactors_masks_new_secret_for_that_server_only() {
    use crate::session_extension::{rebuild_redactors_for_sessions, ServerScope, SessionExtension};
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};
    use secrecy::SecretString;

    struct Changing(StdMutex<Vec<String>>);
    impl SessionExtension for Changing {
        fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|s| SecretString::from(s.clone()))
                .collect()
        }
    }

    let manager = SessionManager::new();
    let server_id = ServerId::new();
    let other_server = ServerId::new();
    let id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    manager.insert(id, Arc::new(dummy_session(server_id)));
    manager.insert(other_id, Arc::new(dummy_session(other_server)));
    let ext = Changing(StdMutex::new(Vec::new()));
    let profiles = InMemoryProfileStore::new();
    let credentials = InMemoryCredentialStore::new();
    let text = "key=late-secret-77";

    assert_eq!(manager.get(id).unwrap().redactor.redact_text(text), text);

    ext.0.lock().unwrap().push("late-secret-77".to_string());
    rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&ext), server_id).await;

    assert!(!manager
        .get(id)
        .unwrap()
        .redactor
        .redact_text(text)
        .contains("late-secret-77"));
    // the session of the other server keeps its old redactor
    assert_eq!(
        manager.get(other_id).unwrap().redactor.redact_text(text),
        text
    );
}

/// Issue #635: `replace_with_literals` installs the new set atomically —
/// a still-known value is never unmasked while another thread replaces
/// the set — and drops empty values.
#[test]
fn test_replace_with_literals_is_atomic_for_known_values() {
    use secrecy::SecretString;
    let red = Arc::new(SwappableRedactor::new(Box::new(
        DefaultOutputRedactor::new(),
    )));
    let keep = SecretString::from("keep-secret-9".to_string());
    red.add_secret_values(std::slice::from_ref(&keep));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (red, stop) = (red.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut leaks = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                if red
                    .redact_text("x keep-secret-9 y")
                    .contains("keep-secret-9")
                {
                    leaks += 1;
                }
            }
            leaks
        })
    };
    for _ in 0..100 {
        red.replace_with_literals(
            Box::new(DefaultOutputRedactor::new()),
            &[keep.clone(), SecretString::from(String::new())],
        );
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(reader.join().unwrap(), 0);
    assert_eq!(red.redact_text("a keep-secret-9"), "a [REDACTED]");
    red.replace_with_literals(Box::new(DefaultOutputRedactor::new()), &[]);
    assert_eq!(red.redact_text("a keep-secret-9"), "a keep-secret-9");
}

/// Issue #635: a rebuild with a known set replaces the literal values;
/// a plain swap (reconnect) keeps them.
#[tokio::test]
async fn test_rebuild_replaces_literals_but_reconnect_swap_keeps_them() {
    use crate::session_extension::{rebuild_redactors_for_sessions, ServerScope, SessionExtension};
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};
    use secrecy::SecretString;

    struct Known(Vec<String>);
    impl SessionExtension for Known {
        fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
            self.0.iter().cloned().map(SecretString::from).collect()
        }
    }

    let manager = SessionManager::new();
    let server_id = ServerId::new();
    let id = Uuid::new_v4();
    manager.insert(id, Arc::new(dummy_session(server_id)));
    let session = manager.get(id).unwrap();
    let text = "a=value-aaa-1 b=value-bbb-2";
    session
        .redactor
        .add_secret_values(&[SecretString::from("value-aaa-1".to_string())]);
    assert_eq!(
        session.redactor.redact_text(text),
        "a=[REDACTED] b=value-bbb-2"
    );

    // reconnect-style swap: A stays masked
    session.swap_redactor(Box::new(DefaultOutputRedactor::new()));
    assert_eq!(
        session.redactor.redact_text(text),
        "a=[REDACTED] b=value-bbb-2"
    );

    // rebuild with {B}: A no longer masked, B is
    let ext = Known(vec!["value-bbb-2".to_string()]);
    let profiles = InMemoryProfileStore::new();
    let credentials = InMemoryCredentialStore::new();
    rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&ext), server_id).await;
    assert_eq!(
        session.redactor.redact_text(text),
        "a=value-aaa-1 b=[REDACTED]"
    );

    // a value still known stays masked after another rebuild, and a
    // later reconnect swap keeps it
    rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&ext), server_id).await;
    session.swap_redactor(Box::new(DefaultOutputRedactor::new()));
    assert_eq!(
        session.redactor.redact_text(text),
        "a=value-aaa-1 b=[REDACTED]"
    );

    // rebuild with an empty set withdraws everything
    let none = Known(Vec::new());
    rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&none), server_id).await;
    assert_eq!(session.redactor.redact_text(text), text);
}

// --- Spec 0039, Abschnitt 5: history_contains_untrusted_content -------

#[test]
fn test_history_contains_untrusted_content_false_for_empty_history() {
    assert!(!history_contains_untrusted_content(&[]));
}

#[test]
fn test_history_contains_untrusted_content_false_for_plain_text_only() {
    let history = vec![ChatMessage {
        role: Role::User,
        content: MessageContent::Text("Wie geht es dir?".to_string()),
    }];
    assert!(!history_contains_untrusted_content(&history));
}

#[test]
fn test_history_contains_untrusted_content_true_for_command_result() {
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::CommandResult {
            command: "ls -la".to_string(),
            output: CommandOutput {
                stdout: b"total 0".to_vec(),
                stderr: Vec::new(),
                exit_code: Some(0),
                truncated: false,
            },
            cancelled: false,
            waiting_for_input: false,
        },
    }];
    assert!(
        history_contains_untrusted_content(&history),
        "CommandResult wird beim Versand gefenced (ai-providers::format_command_result), \
         zählt also schon hier als untrusted-Inhalt"
    );
}

#[test]
fn test_history_contains_untrusted_content_true_for_fenced_remote_file_text() {
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(
            "Inhalt von '/etc/hosts':\n\n<remote_file>\n<source>/etc/hosts</source>\n\
             127.0.0.1 localhost\n</remote_file>"
                .to_string(),
        ),
    }];
    assert!(history_contains_untrusted_content(&history));
}

/// Issue #504 (ADR 0141, decision 4): a resumed history with an extension
/// part sets the untrusted-content escalation, with no other untrusted
/// content around.
#[test]
fn test_history_contains_untrusted_content_true_for_extension_part() {
    use ssh_manager_core::ai::{ExtensionMedia, ExtensionPart};
    let text = ChatMessage {
        role: Role::User,
        content: MessageContent::Text("hello".to_string()),
    };
    assert!(!history_contains_untrusted_content(std::slice::from_ref(
        &text
    )));
    let part = ChatMessage {
        role: Role::User,
        content: MessageContent::Extension(ExtensionPart {
            extension: "files".into(),
            kind: "k".into(),
            id: "i".into(),
            label: "a.txt".into(),
            media: ExtensionMedia::Text,
            estimated_tokens: 3,
        }),
    };
    assert!(history_contains_untrusted_content(&[text, part]));
}

/// Spec 0064 Nachtrag: eine Regression dieses Schritts selbst gefangen
/// — der `uname`-Banner zog von `SystemContextParts` in eine eigene,
/// gefencte Verlaufs-Nachricht (`UntrustedKind::RemoteOsInfo`,
/// Tag `<remote_system>`) um; ein hartcodiertes `FENCE_OPEN_TAGS`-Array
/// hier hätte diesen neuen Tag glatt vergessen können (genau das ist
/// vor dem Fix passiert, bevor auf `fence_markers()` umgestellt wurde)
/// — eine Sitzung, deren einziger Untrusted-Inhalt der Banner ist
/// (kein Notiz, kein Kommando bisher), hätte
/// `untrusted_content_ingested` dann fälschlich nie gesetzt.
#[test]
fn test_history_contains_untrusted_content_true_for_fenced_remote_os_info_text() {
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(
            "<remote_system>\n<source>uname -a</source>\nLinux srv1 5.10.0\n</remote_system>"
                .to_string(),
        ),
    }];
    assert!(history_contains_untrusted_content(&history));
}

#[test]
fn test_history_contains_untrusted_content_false_for_action_rejected() {
    let history = vec![ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::ActionRejected {
            command: "rm -rf /".to_string(),
            reason: RejectionReason::User,
        },
    }];
    assert!(
        !history_contains_untrusted_content(&history),
        "Kommando/Grund stammen von der KI/lokalen Filter-Engine, nie vom Remote-Server"
    );
}

fn status_events(emitter: &crate::events::TestEmitter) -> Vec<serde_json::Value> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "connection-status-changed")
        .map(|(_, payload)| payload.clone())
        .collect()
}

async fn wait_for_status_events(emitter: &crate::events::TestEmitter, count: usize) {
    for _ in 0..200 {
        if status_events(emitter).len() >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// Issue #353: terminal EOF first, then a later loss report (exec/SFTP
/// error, keepalive) must not emit a second `disconnected` event.
#[test]
fn test_connection_loss_is_reported_once_per_session() {
    let session = dummy_session(ServerId::new());
    let emitter = crate::events::TestEmitter::default();
    let id = Uuid::new_v4();

    assert!(report_connection_lost(id, &session, &emitter, None));
    assert!(!report_connection_lost(
        id,
        &session,
        &emitter,
        Some("The server stopped responding".to_string())
    ));
    assert_eq!(status_events(&emitter).len(), 1);
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );
}

/// Issue #353: a chat-only session (no terminal actor) is marked
/// `Disconnected` by the watcher, with the reason, exactly once even if
/// the loss is reported again afterwards.
#[tokio::test]
async fn test_connection_watch_reports_loss_without_terminal() {
    let session = Arc::new(dummy_session(ServerId::new()));
    let emitter = Arc::new(crate::events::TestEmitter::default());
    let (tx, rx) = tokio::sync::watch::channel(None);
    let id = Uuid::new_v4();
    spawn_connection_watch(id, &session, rx, emitter.clone());

    tx.send(Some(ssh_manager_core::ssh::ConnectionLoss {
        reason: Some("The server stopped responding".to_string()),
    }))
    .unwrap();
    wait_for_status_events(&emitter, 1).await;

    let events = status_events(&emitter);
    assert_eq!(events.len(), 1);
    assert!(events[0]
        .to_string()
        .contains("The server stopped responding"));
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );
    assert!(!report_connection_lost(
        id,
        &session,
        emitter.as_ref(),
        None
    ));
    assert_eq!(status_events(&emitter).len(), 1);
}

/// Issue #353: an explicit `disconnect()` closes the transport on
/// purpose; the resulting signal is not a loss.
#[tokio::test]
async fn test_connection_watch_ignores_intentional_close() {
    let session = Arc::new(dummy_session(ServerId::new()));
    let emitter = Arc::new(crate::events::TestEmitter::default());
    let (tx, rx) = tokio::sync::watch::channel(None);
    spawn_connection_watch(Uuid::new_v4(), &session, rx, emitter.clone());

    session.transport.mark_closed();
    tx.send(Some(ssh_manager_core::ssh::ConnectionLoss { reason: None }))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(status_events(&emitter).is_empty());
    assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);
}

/// Issue #565: after a reconnect the old transport's watch stays
/// silent, and a watch started for the new transport reports its loss.
#[tokio::test]
async fn test_connection_watch_after_reconnect_covers_new_transport_only() {
    let session = Arc::new(dummy_session(ServerId::new()));
    let emitter = Arc::new(crate::events::TestEmitter::default());
    let id = Uuid::new_v4();
    let (old_tx, old_rx) = tokio::sync::watch::channel(None);
    spawn_connection_watch(id, &session, old_rx, emitter.clone());

    *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
    let claim = session.begin_reconnect().expect("claim");
    claim
        .succeed(
            Box::new(UnusedTransport),
            Box::new(DefaultOutputRedactor::new()),
            None,
        )
        .await;
    let (new_tx, new_rx) = tokio::sync::watch::channel(None);
    spawn_connection_watch(id, &session, new_rx, emitter.clone());

    old_tx
        .send(Some(ssh_manager_core::ssh::ConnectionLoss { reason: None }))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(status_events(&emitter).is_empty());
    assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);

    new_tx
        .send(Some(ssh_manager_core::ssh::ConnectionLoss { reason: None }))
        .unwrap();
    wait_for_status_events(&emitter, 1).await;
    assert_eq!(status_events(&emitter).len(), 1);
    assert_eq!(
        *session.status.lock().unwrap(),
        ConnectionStatus::Disconnected
    );
}

#[test]
fn test_insert_get_remove_round_trip() {
    let manager = SessionManager::new();
    let id = Uuid::new_v4();
    let server_id = ServerId::new();
    manager.insert(id, Arc::new(dummy_session(server_id)));

    assert!(manager.get(id).is_some());
    assert_eq!(manager.get(id).unwrap().server_id, server_id);

    let removed = manager.remove(id);
    assert!(removed.is_some());
    assert!(manager.get(id).is_none());
}

#[test]
fn test_snapshot_reports_status_and_pending_action_per_session() {
    let manager = SessionManager::new();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let server_a = ServerId::new();
    let server_b = ServerId::new();

    let session_a = dummy_session(server_a);
    // Simuliert eine wartende Confirm-Bestätigung (Spec 0017, Abschnitt
    // 5), wie sie `orchestration::handle_action_proposed` setzt.
    *session_a.pending_action.lock().unwrap() = Some(Uuid::new_v4());
    manager.insert(id_a, Arc::new(session_a));

    let session_b = dummy_session(server_b);
    *session_b.status.lock().unwrap() = ConnectionStatus::Disconnected;
    manager.insert(id_b, Arc::new(session_b));

    let snapshot = manager.snapshot();
    assert_eq!(snapshot.len(), 2);

    let entry_a = snapshot.iter().find(|e| e.session_id == id_a).unwrap();
    assert_eq!(entry_a.server_id, server_a);
    assert_eq!(entry_a.status, ConnectionStatus::Connected);
    assert!(
        entry_a.has_pending_action,
        "Session mit wartender Confirm-Entscheidung muss has_pending_action=true melden"
    );

    let entry_b = snapshot.iter().find(|e| e.session_id == id_b).unwrap();
    assert_eq!(entry_b.status, ConnectionStatus::Disconnected);
    assert!(!entry_b.has_pending_action);
}

#[test]
fn test_snapshot_includes_pending_host_key_connections_as_awaiting() {
    let manager = SessionManager::new();
    let pending_id = Uuid::new_v4();
    let server_id = ServerId::new();
    manager.register_pending_connection(pending_id, server_id);

    let snapshot = manager.snapshot();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].session_id, pending_id);
    assert_eq!(snapshot[0].server_id, server_id);
    assert_eq!(snapshot[0].status, ConnectionStatus::AwaitingHostKey);
    assert!(!snapshot[0].has_pending_action);

    manager.clear_pending_connection(pending_id);
    assert!(manager.snapshot().is_empty());
}

#[test]
fn test_session_count_watch_follows_insert_and_remove() {
    let manager = SessionManager::new();
    let rx = manager.subscribe_count();
    assert_eq!(*rx.borrow(), 0);
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    manager.insert(a, Arc::new(dummy_session(ServerId(Uuid::new_v4()))));
    manager.insert(b, Arc::new(dummy_session(ServerId(Uuid::new_v4()))));
    assert_eq!(*rx.borrow(), 2);
    manager.remove(a);
    assert_eq!(*rx.borrow(), 1);
    manager.remove(a);
    manager.remove(b);
    assert_eq!(*rx.borrow(), 0);
}

#[test]
fn test_snapshot_combines_real_sessions_and_pending_connections() {
    let manager = SessionManager::new();
    let connected_id = Uuid::new_v4();
    manager.insert(connected_id, Arc::new(dummy_session(ServerId::new())));

    let pending_id = Uuid::new_v4();
    manager.register_pending_connection(pending_id, ServerId::new());

    let snapshot = manager.snapshot();
    assert_eq!(snapshot.len(), 2);
    assert!(snapshot
        .iter()
        .any(|e| e.session_id == connected_id && e.status == ConnectionStatus::Connected));
    assert!(snapshot
        .iter()
        .any(|e| e.session_id == pending_id && e.status == ConnectionStatus::AwaitingHostKey));
}

// --- Spec 0040, Abschnitt 7: is_chat_session_active ---------------------

/// Spec 0088, T15 (A3.2): Die EINE Stelle, die einen leeren normalen
/// SFTP-Kanal in einen Fehler umwandelt, tut das an einer frischen
/// Sitzung auch tatsächlich — statt zu panicken.
///
/// Frisch heisst hier: `ensure_sftp_open` lief nie. Genau dieser Zustand
/// stand vorher hinter sechs
/// `expect("ensure_sftp_open lief erfolgreich durch")` in
/// `orchestration::remote_files`.
#[tokio::test]
async fn test_sftp_or_err_reports_an_error_on_a_session_whose_channel_was_never_opened() {
    let session = dummy_session(ServerId::new());
    let mut guard = session.lock_sftp().await;
    assert!(!guard.is_open(), "frische Sitzung: Kanal ist zu");

    let err = guard
        .sftp_or_err()
        .err()
        .expect("ein nicht geöffneter Kanal muss ein Fehler sein, kein Erfolg");
    assert_eq!(err.code(), "SSH_CHANNEL_ERROR");
    assert!(
        guard.sftp().is_none(),
        "und `sftp()` meldet weiterhin schlicht None"
    );
}

#[tokio::test]
async fn test_is_chat_session_active_true_for_a_live_session_bound_to_it() {
    let manager = SessionManager::new();
    let chat_session_id = Uuid::new_v4();
    let mut session = dummy_session(ServerId::new());
    session.set_chat_session_id_for_tests(AsyncMutex::new(Some(chat_session_id)));
    manager.insert(Uuid::new_v4(), Arc::new(session));

    assert!(manager.is_chat_session_active(chat_session_id).await);
}

#[tokio::test]
async fn test_is_chat_session_active_false_for_an_unrelated_chat_session_id() {
    let manager = SessionManager::new();
    let mut session = dummy_session(ServerId::new());
    session.set_chat_session_id_for_tests(AsyncMutex::new(Some(Uuid::new_v4())));
    manager.insert(Uuid::new_v4(), Arc::new(session));

    assert!(!manager.is_chat_session_active(Uuid::new_v4()).await);
}

#[tokio::test]
async fn test_is_chat_session_active_false_when_no_live_session_has_a_chat_session_id() {
    let manager = SessionManager::new();
    manager.insert(Uuid::new_v4(), Arc::new(dummy_session(ServerId::new())));

    assert!(!manager.is_chat_session_active(Uuid::new_v4()).await);
}

// Issue #242: Claim-Logik (Resume vs. Löschen).
#[tokio::test]
async fn delete_is_refused_while_a_resume_holds_the_claim() {
    let manager = SessionManager::new();
    let id = Uuid::new_v4();
    let _resume = manager.claim_chat_session_for_resume(id).unwrap();
    assert!(matches!(
        manager.claim_chat_session_for_delete(id).await,
        Err(ChatSessionClaimError::InUse)
    ));
}

#[tokio::test]
async fn delete_is_allowed_again_after_a_failed_resume_drops_the_claim() {
    let manager = SessionManager::new();
    let id = Uuid::new_v4();
    let resume = manager.claim_chat_session_for_resume(id).unwrap();
    drop(resume); // Fehlerpfad: Guard fällt
    assert!(manager.claim_chat_session_for_delete(id).await.is_ok());
}

#[tokio::test]
async fn delete_of_unused_chat_session_is_allowed_and_releases_afterwards() {
    let manager = SessionManager::new();
    let id = Uuid::new_v4();
    let delete = manager.claim_chat_session_for_delete(id).await.unwrap();
    // Läuft das Löschen, scheitert ein Resume und ein zweites Löschen.
    assert!(matches!(
        manager.claim_chat_session_for_resume(id),
        Err(ChatSessionClaimError::BeingDeleted)
    ));
    assert!(matches!(
        manager.claim_chat_session_for_delete(id).await,
        Err(ChatSessionClaimError::BeingDeleted)
    ));
    drop(delete);
    assert!(manager.claim_chat_session_for_resume(id).is_ok());
}

#[tokio::test]
async fn claims_are_per_chat_session() {
    let manager = SessionManager::new();
    let _resume = manager
        .claim_chat_session_for_resume(Uuid::new_v4())
        .unwrap();
    assert!(manager
        .claim_chat_session_for_delete(Uuid::new_v4())
        .await
        .is_ok());
}
