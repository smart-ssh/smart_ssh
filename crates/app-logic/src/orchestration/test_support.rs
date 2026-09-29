//! Gemeinsame Test-Hilfen (Fixtures, Mock-Aufbauten) für die Testmodule von
//! `orchestration`s Untermodulen `chat_turn`, `action_exec`, `notes` und
//! `remote_files` — Spec 0083, Design (§4): "Gemeinsame Test-Hilfen ...
//! kommen in ein gemeinsames cfg(test)-Modul, nicht dupliziert." Reine
//! Verschiebung aus dem bisherigen `orchestration::tests`-Modulkopf, keine
//! Verhaltensänderung.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;

use ssh_manager_core::ai::{
    default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, SessionContext,
};
use ssh_manager_core::filter::{Decision, EffectiveScope, FilterEngine, PolicyStore, Rule};
use ssh_manager_core::profiles::{
    AiAction, Group, GroupId, NoteRevision, ProfileResult, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize, SshError};

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::TestEmitter;
use crate::policy::NoRulesPolicyStore;
use crate::session::{Session, SessionParts};
use crate::state::ActionId;

use super::action_exec::handle_action_proposed;

use uuid::Uuid;

/// Tests, die `handle_action_proposed` direkt aufrufen: ein frisches,
/// unabhängiges Ablehnungs-Flag pro Aufruf (Spec 0068, Teil 4).
#[cfg(test)]
pub(crate) fn test_fresh_rejection_flag() -> &'static std::sync::atomic::AtomicBool {
    Box::leak(Box::new(std::sync::atomic::AtomicBool::new(false)))
}

/// Konfigurierbar mit einer Sequenz von Runden (je ein `Vec<AiEvent>`
/// pro `send()`-Aufruf) — nötig, um die automatische Folgerunde aus
/// dem Moduldoc zu testen: Runde 1 schlägt z. B. ein Kommando vor,
/// Runde 2 (nach dessen Ausführung) liefert die eigentliche
/// Antwort-Text. Ruft `send()` öfter auf als Runden konfiguriert sind
/// (weil eine Runde nichts ausgeführt hat und die Schleife eigentlich
/// hätte stoppen sollen), liefert jeder weitere Aufruf nur `[Done]` —
/// bequemer Default für Tests, die nur den ersten Round-Trip prüfen
/// wollen, ohne dafür jede Folgerunde einzeln angeben zu müssen.
pub(crate) struct MockAiProvider {
    pub(crate) rounds: StdMutex<std::collections::VecDeque<Vec<AiEvent>>>,
    /// Jeder empfangene `SessionContext`, in Aufrufreihenfolge — geteilt
    /// über einen `Arc`, den ein Test sich per
    /// [`MockAiProvider::received_contexts_handle`] VOR dem Verschieben
    /// des Providers in eine `Session` (`Box<dyn AiProvider>`, danach
    /// nicht mehr direkt inspizierbar) sichern kann. Nötig für Spec
    /// 0010: prüft, dass `suggest_note_update_on_disconnect` einerseits
    /// gar keinen Aufruf macht, wenn die Schwelle nicht erreicht ist,
    /// und andererseits `available_actions` korrekt auf
    /// `propose_note_update` beschränkt, wenn doch.
    pub(crate) received_contexts: Arc<StdMutex<Vec<SessionContext>>>,
}

impl MockAiProvider {
    pub(crate) fn new(events: Vec<AiEvent>) -> Self {
        Self::with_rounds(vec![events])
    }

    pub(crate) fn with_rounds(rounds: Vec<Vec<AiEvent>>) -> Self {
        Self {
            rounds: StdMutex::new(rounds.into()),
            received_contexts: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    pub(crate) fn received_contexts_handle(&self) -> Arc<StdMutex<Vec<SessionContext>>> {
        self.received_contexts.clone()
    }
}

impl AiProvider for MockAiProvider {
    fn send(
        &self,
        context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        self.received_contexts.lock().unwrap().push(context);
        let events = self
            .rounds
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| vec![AiEvent::Done]);
        Box::pin(futures::stream::iter(events))
    }
}

#[derive(Default)]
pub(crate) struct MockSshTransport {
    responses: HashMap<String, CommandOutput>,
    /// Für Kommandos mit dynamischen Bestandteilen (z. B. Sudo-Befehle
    /// mit generiertem Backup-/Temp-Dateinamen, Spec 0020, Abschnitt
    /// 4.3), bei denen der Test den exakten Wortlaut nicht vorhersagen
    /// kann — matcht auf Kommando-Präfix statt Exaktheit.
    prefix_responses: Vec<(String, CommandOutput)>,
    /// Spec 0018: geteilter Handle (analog zu
    /// `MockAiProvider::received_contexts`), damit ein Test nach dem
    /// Lauf prüfen kann, mit welchem (ggf. umgeschriebenen) Kommando und
    /// welchem Stdin-Inhalt `execute_with_stdin` tatsächlich aufgerufen
    /// wurde.
    stdin_calls: StdinCalls,
    /// Spec 0027: Kommandos in dieser Liste simulieren ein nie von
    /// selbst endendes Kommando (`journalctl -f`) — `execute_cancellable`
    /// wartet für sie ausschließlich auf `cancel`, statt sofort
    /// zurückzukehren.
    never_completing: std::collections::HashSet<String>,
}

pub(crate) type StdinCalls = Arc<StdMutex<Vec<(String, Vec<u8>)>>>;

impl MockSshTransport {
    pub(crate) fn with_response(
        mut self,
        command: impl Into<String>,
        output: CommandOutput,
    ) -> Self {
        self.responses.insert(command.into(), output);
        self
    }

    pub(crate) fn with_prefix_response(
        mut self,
        command_prefix: impl Into<String>,
        output: CommandOutput,
    ) -> Self {
        self.prefix_responses.push((command_prefix.into(), output));
        self
    }

    pub(crate) fn stdin_calls_handle(&self) -> StdinCalls {
        self.stdin_calls.clone()
    }

    pub(crate) fn with_never_completing(mut self, command: impl Into<String>) -> Self {
        self.never_completing.insert(command.into());
        self
    }
}

#[async_trait]
impl ssh_manager_core::ssh::SshTransport for MockSshTransport {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        if let Some(output) = self.responses.get(command).cloned() {
            return Ok(output);
        }
        if let Some((_, output)) = self
            .prefix_responses
            .iter()
            .find(|(prefix, _)| command.starts_with(prefix.as_str()))
        {
            return Ok(output.clone());
        }
        Err(SshError::ChannelError(format!(
            "kein Mock-Response für '{command}'"
        )))
    }

    async fn execute_with_stdin(
        &mut self,
        command: &str,
        stdin: &[u8],
    ) -> Result<CommandOutput, SshError> {
        self.stdin_calls
            .lock()
            .unwrap()
            .push((command.to_string(), stdin.to_vec()));
        self.execute(command).await
    }

    async fn execute_cancellable(
        &mut self,
        command: &str,
        cancel: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<ssh_manager_core::ssh::ExecOutcome, SshError> {
        if self.never_completing.contains(command) {
            // Spec 0027: simuliert `journalctl -f` — wartet
            // ausschließlich auf `cancel`, liefert dann eine feste
            // "bereits eingetroffene" Teil-Ausgabe zurück.
            let _ = cancel.await;
            return Ok(ssh_manager_core::ssh::ExecOutcome {
                output: CommandOutput {
                    stdout: b"partial output before cancel".to_vec(),
                    stderr: Vec::new(),
                    exit_code: None,
                    truncated: false,
                },
                cancelled: true,
            });
        }
        Ok(ssh_manager_core::ssh::ExecOutcome {
            output: self.execute(command).await?,
            cancelled: false,
        })
    }

    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        Err(SshError::ChannelError(
            "in diesem Test nicht unterstützt".to_string(),
        ))
    }

    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct InMemoryProfileStore {
    pub(crate) note_revisions: StdMutex<Vec<NoteRevision>>,
    /// Nur von den Spec-0040-Abschnitt-6-Tests ("In Notiz übernehmen")
    /// befüllt, die einen tatsächlich auflösbaren `get_server`-Aufruf
    /// brauchen, um einen bestehenden Notizinhalt vorzuspiegeln — alle
    /// anderen Tests in diesem Modul lassen die Map leer und verlassen
    /// sich weiterhin auf den bisherigen `ServerNotFound`-Fallback
    /// unten.
    pub(crate) servers: StdMutex<HashMap<ServerId, Server>>,
}

#[async_trait]
impl ProfileStore for InMemoryProfileStore {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        self.servers.lock().unwrap().get(id).cloned().ok_or(
            ssh_manager_core::profiles::ProfileError::ServerNotFound(*id),
        )
    }
    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        Err(ssh_manager_core::profiles::ProfileError::GroupNotFound(*id))
    }
    async fn list_servers(&self) -> ProfileResult<Vec<Server>> {
        Ok(Vec::new())
    }
    async fn list_groups(&self) -> ProfileResult<Vec<Group>> {
        Ok(Vec::new())
    }
    async fn create_group(&self, _group: &Group) -> ProfileResult<()> {
        Ok(())
    }
    async fn update_group(&self, _group: &Group) -> ProfileResult<()> {
        Ok(())
    }
    async fn delete_group(&self, _id: &GroupId) -> ProfileResult<()> {
        Ok(())
    }
    async fn create_server(&self, _server: &Server) -> ProfileResult<()> {
        Ok(())
    }
    async fn update_server(&self, _server: &Server) -> ProfileResult<()> {
        Ok(())
    }
    async fn delete_server(&self, _id: &ServerId) -> ProfileResult<()> {
        Ok(())
    }
    async fn record_note_revision(&self, revision: &NoteRevision) -> ProfileResult<()> {
        self.note_revisions.lock().unwrap().push(revision.clone());
        Ok(())
    }
    async fn list_note_revisions(
        &self,
        target: ssh_manager_core::profiles::NoteTarget,
    ) -> ProfileResult<Vec<NoteRevision>> {
        Ok(self
            .note_revisions
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.target == target)
            .cloned()
            .collect())
    }
}

pub(crate) fn test_session(ai_events: Vec<AiEvent>, transport: MockSshTransport) -> Session {
    session_with_ai_provider(MockAiProvider::new(ai_events), transport)
}

pub(crate) fn session_with_ai_provider(
    ai_provider: impl AiProvider + 'static,
    transport: MockSshTransport,
) -> Session {
    Session::new(SessionParts {
        transport: crate::session::SessionTransport::new(Box::new(transport)),
        ai_provider: Box::new(ai_provider),
        ai_provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
        context: AsyncMutex::new(SessionContext {
            system_context: "Testkontext".to_string(),
            history: Vec::new(),
            available_actions: default_action_schemas(),
            max_tokens_hint: None,
        }),
        filter_engine: Box::new(FilterEngine::new(NoRulesPolicyStore)),
        server_id: ServerId::new(),
        tags: Vec::new(),
        terminal: StdMutex::new(None),
        redactor: Box::new(DefaultOutputRedactor::new()),
        ai_provider_label: "test-provider".to_string(),
        ai_model: "test-model".to_string(),
        system_context_parts: AsyncMutex::new(crate::compaction::SystemContextParts::default()),
        model_context_window_tokens: usize::MAX / 1_000,
        summary: AsyncMutex::new(None),
        mcp_origin_flags: StdMutex::new(Vec::new()),
        sudo_password: None,
        status: StdMutex::new(crate::events::ConnectionStatus::Connected),
        pending_action: StdMutex::new(None),
        auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
        auto_continue_stop_notify: tokio::sync::Notify::new(),
        chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
        risk_second_opinion_provider: None,
        risk_second_opinion_budget: None,
        // Spec 0092, §5: standardmäßig „an", damit ein Test mit rotem
        // Kommando und Allow-Regel sichtbar bricht statt still auf „aus" zu
        // laufen. Tests, die das Verhalten bei ausgeschalteter Einstellung
        // prüfen, setzen es ausdrücklich über
        // `parts_mut_for_tests().red_risk_always_confirm = false`.
        red_risk_always_confirm: true,
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
        post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
        injection_check_provider: None,
        injection_check_budget: None,
        injection_suspected: std::sync::atomic::AtomicBool::new(false),
        // Spec 0034: Tests laufen bewusst ohne Persistenz-Anbindung
        // (s. `Session::chat_session_store`-Doc-Kommentar) — kein
        // In-Memory-`ChatSessionStore`-Mock nötig, `push_history`
        // no-opt bei `None` bereits vollständig.
        chat_session_store: None,
        ledger_store: None,
        chat_session_id: AsyncMutex::new(None),
        ai_request_paced_at: AsyncMutex::new(None),
    })
}

/// Wie [`session_with_ai_provider`], aber mit einem konfigurierten
/// Zweitmeinungs-Provider (Spec 0026, Abschnitt 3) — für Tests, die die
/// Eskalationslogik prüfen.
pub(crate) fn session_with_second_opinion(
    ai_events: Vec<AiEvent>,
    transport: MockSshTransport,
    second_opinion_provider: impl AiProvider + 'static,
) -> Session {
    // Spec 0085, A3: `Session` hat ein privates Feld, also kein Struct-Update
    // („`..base`") mehr — die beiden Felder werden nach dem Bau gesetzt.
    let mut session = session_with_ai_provider(MockAiProvider::new(ai_events), transport);
    session.parts_mut_for_tests().risk_second_opinion_provider =
        Some(Box::new(second_opinion_provider));
    session.parts_mut_for_tests().risk_second_opinion_budget =
        Some(Arc::new(ai_providers::ProviderBudgetGuard::new()));
    session
}

pub(crate) fn output(stdout: &str) -> CommandOutput {
    CommandOutput {
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
        exit_code: Some(0),
        truncated: false,
    }
}

/// Blendet `chat-auto-continuation-started` aus einer Event-Liste aus —
/// für ältere Tests, die etwas anderes prüfen und durch das seit Spec
/// 0021 in *jeder* automatischen Folgerunde zusätzlich gesendete
/// Ereignis (Abschnitt 5) nicht gestört werden sollen. Mit `test_session`
/// (ein konfiguriertes `MockAiProvider`-Round) triggert nach Spec 0021
/// praktisch jeder abgeschlossene erste Round-Trip automatisch eine
/// zweite (leere) Runde — dediziert getestet in den `test_auto_*`-Tests
/// unten, hier bewusst ausgeblendet, um die eigentliche Testaussage nicht
/// zu verwässern.
pub(crate) fn event_names_excluding_auto_continuation(
    events: &[(String, serde_json::Value)],
) -> Vec<&str> {
    events
        .iter()
        .filter(|(name, _)| name != "chat-auto-continuation-started")
        .map(|(name, _)| name.as_str())
        .collect()
}

/// `NoRulesPolicyStore` (der `test_session`-Standard) landet für jedes
/// Kommando ohne passende Regel auf `Confirm` (s. `core::filter::engine`:
/// "keine Regel gefunden" ist der Default-Fallback) — für einen
/// AutoExec-Test wird deshalb eine explizite `Allow`-Regel gebraucht,
/// sonst würde der Test denselben Confirm-Wartepfad wie
/// `test_confirm_path_waits_for_respond_to_action_before_executing`
/// nehmen (und ohne Responder-Task ewig auf eine nie eintreffende
/// Bestätigung hängen bleiben).
pub(crate) struct AllowEverythingPolicyStore;
#[async_trait]
impl PolicyStore for AllowEverythingPolicyStore {
    async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        vec![Rule {
            id: ssh_manager_core::filter::RuleId("allow-all".to_string()),
            pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
            action: ssh_manager_core::filter::RuleAction::Allow,
            scope: ssh_manager_core::filter::Scope::Global,
            priority: 0,
            origin: ssh_manager_core::filter::RuleOrigin::User,
        }]
    }
}

// --- Spec 0039, Abschnitt 5: PostIngestPolicy-Eskalation ---------------

/// Anders als `deny_first_proposed_action`/`respond_to_first_proposed_
/// action` (die blind das ERSTE `chat-action-proposed` auflösen und
/// dabei bei `AutoExec` — kein registriertes `confirm_rx`, s.
/// `ConfirmationRegistry::resolve` — mit `.unwrap()` paniken würden):
/// diese Variante wartet gezielt auf eine `Confirm`-Entscheidung und
/// lässt eine `AutoExec`/`Deny`-Aktion (die ohnehin ohne Bestätigung
/// durchläuft) einfach von selbst fertig werden. Nötig, weil die
/// PostIngestPolicy-Tests unten absichtlich beide Ausgänge prüfen.
pub(crate) async fn proposed_decision_code(
    session: &Session,
    action: AiAction,
) -> (Decision, serde_json::Value) {
    proposed_decision_code_with_origin(session, action, ActionOrigin::Internal).await
}

pub(crate) async fn proposed_decision_code_with_origin(
    session: &Session,
    action: AiAction,
    origin: ActionOrigin,
) -> (Decision, serde_json::Value) {
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let action_future = handle_action_proposed(
        session,
        session_id,
        action,
        &emitter,
        &profile_store,
        &confirmations,
        origin,
        test_fresh_rejection_flag(),
    );
    tokio::pin!(action_future);

    let responder = async {
        loop {
            let confirm_action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    if name != "chat-action-proposed" {
                        return None;
                    }
                    payload.get("decision").and_then(|d| d.get("Confirm"))?;
                    Some(payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(id) = confirm_action_id {
                let action_id: ActionId = id.parse().unwrap();
                let _ = confirmations.resolve(&action_id, ActionUserDecision::Deny);
                return;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::pin!(responder);

    // `select!` statt `join!`: bei `AutoExec`/`Deny` wird `action_future`
    // fertig, OHNE dass `responder` je eine `Confirm`-Entscheidung
    // findet (die dortige Schleife würde sonst ewig weiterlaufen). Nur
    // wenn `responder` zuerst fertig wird (eine `Confirm`-Entscheidung
    // wurde gefunden und aufgelöst), muss `action_future` danach noch
    // separat abgewartet werden, damit sein `rx.await` tatsächlich
    // zurückkehrt.
    tokio::select! {
        _ = &mut action_future => {}
        _ = &mut responder => {
            action_future.await;
        }
    }

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-proposed")
        .expect("chat-action-proposed muss gesendet worden sein")
        .clone();
    let decision: Decision = serde_json::from_value(proposed_payload["decision"].clone())
        .expect("decision muss deserialisierbar sein");
    (decision, proposed_payload)
}

/// Hilfsfunktion für Tests, die eine `Confirm`-Aktion ablehnen wollen,
/// sobald sie im Event-Log auftaucht.
pub(crate) fn deny_first_proposed_action<'a>(
    emitter: &'a TestEmitter,
    confirmations: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> impl std::future::Future<Output = ()> + 'a {
    respond_to_first_proposed_action(emitter, confirmations, ActionUserDecision::Deny)
}

/// Wie [`deny_first_proposed_action`], aber genehmigend.
pub(crate) fn approve_first_proposed_action<'a>(
    emitter: &'a TestEmitter,
    confirmations: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> impl std::future::Future<Output = ()> + 'a {
    respond_to_first_proposed_action(emitter, confirmations, ActionUserDecision::Approve)
}

pub(crate) async fn respond_to_first_proposed_action(
    emitter: &TestEmitter,
    confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    decision: ActionUserDecision,
) {
    loop {
        let action_id = {
            let events = emitter.events.lock().unwrap();
            events.iter().find_map(|(name, payload)| {
                (name == "chat-action-proposed")
                    .then(|| payload["actionId"].as_str().unwrap().to_string())
            })
        };
        if let Some(action_id) = action_id {
            let action_id: ActionId = action_id.parse().unwrap();
            confirmations.resolve(&action_id, decision).unwrap();
            break;
        }
        tokio::task::yield_now().await;
    }
}

// --- Spec 0034: Persistenz-Verdrahtung in die Kernschleife -------------

/// Baut einen echten, migrierten In-Memory-`SqliteChatSessionStore`
/// samt zugehöriger `servers`-Zeile (FK-Pflicht, s.
/// `persistence_sqlite::chat_session_store`-Testsuite) und einer bereits
/// angelegten `chat_sessions`-Zeile — für Tests, die `push_history`s
/// tatsächliche DB-Anbindung end-to-end prüfen wollen, nicht nur den
/// In-Memory-`SessionContext`.
pub(crate) async fn session_with_real_chat_persistence(
    ai_events: Vec<AiEvent>,
    transport: MockSshTransport,
) -> (
    Session,
    persistence_sqlite::SqliteChatSessionStore,
    Uuid,
    tempfile::TempDir,
) {
    let (session, chat_store, chat_session_id, tmp_dir, _ledger_store) =
        session_with_real_chat_and_ledger_persistence(ai_events, transport).await;
    (session, chat_store, chat_session_id, tmp_dir)
}

/// Wie [`session_with_real_chat_persistence`], zusätzlich mit einem
/// echten, migrierten In-Memory-`SqliteLedgerStore` (Spec 0057, §1) —
/// derselbe Verschlüsselungs-Cipher wie für `chat_session_store`
/// (Spec 0057, §1.3: "wie die Chat-Historie", kein zweiter
/// Mechanismus), auf dieselbe `chat_sessions`-Zeile gebunden (Migration
/// 0011: `ledger_entries.session_id` referenziert `chat_sessions(id)`).
pub(crate) async fn session_with_real_chat_and_ledger_persistence(
    ai_events: Vec<AiEvent>,
    transport: MockSshTransport,
) -> (
    Session,
    persistence_sqlite::SqliteChatSessionStore,
    Uuid,
    tempfile::TempDir,
    persistence_sqlite::SqliteLedgerStore,
) {
    // Nur die öffentliche `connect(db_path)`-API steht app-shell zur
    // Verfügung (`connect_with`/`:memory:` sind `pub(crate)` in
    // `persistence-sqlite`, s. dortiger Doc-Kommentar) — eine echte,
    // temporäre Datei statt `:memory:`. Das `TempDir` wird an den
    // Aufrufer zurückgegeben, damit es nicht vor Testende gedroppt (und
    // damit die Datei gelöscht) wird.
    let tmp_dir = tempfile::tempdir().expect("TempDir konnte nicht angelegt werden");
    let db_path = tmp_dir.path().join("test.sqlite3");
    let profile_store = persistence_sqlite::SqliteProfileStore::connect(&db_path)
        .await
        .expect("frische DB sollte immer aufbaubar sein");
    let server_id = ServerId::new();
    let now = chrono::Utc::now();
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
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    let test_cipher: std::sync::Arc<dyn ssh_manager_core::crypto::ContentCipher> =
        std::sync::Arc::new(ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(
            &[13u8; 32],
        ));
    let chat_store = profile_store.chat_session_store(test_cipher.clone());
    let chat_session_id = chat_store.create_session(&server_id, None).await.unwrap();
    let ledger_store = profile_store.ledger_store(test_cipher);

    let mut session = session_with_ai_provider(MockAiProvider::new(ai_events), transport);
    session.parts_mut_for_tests().server_id = server_id;
    session.parts_mut_for_tests().chat_session_store = Some(chat_store.clone());
    session.parts_mut_for_tests().ledger_store = Some(ledger_store.clone());
    session.parts_mut_for_tests().chat_session_id = AsyncMutex::new(Some(chat_session_id));

    (session, chat_store, chat_session_id, tmp_dir, ledger_store)
}

/// Beantwortet den ersten Bestätigungsdialog (Spec 0068: Secret-Pfade
/// verlangen immer eine Bestätigung).
pub(crate) async fn resolve_first_confirm(
    emitter: &TestEmitter,
    confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    decision: ActionUserDecision,
) {
    loop {
        let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
            (n == "chat-action-proposed" && p["decision"].get("Confirm").is_some())
                .then(|| p["actionId"].as_str().unwrap().to_string())
        });
        if let Some(id) = pending {
            confirmations
                .resolve(&id.parse().unwrap(), decision)
                .unwrap();
            return;
        }
        tokio::task::yield_now().await;
    }
}
