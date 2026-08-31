//! Kernschleife eines Chat-Turns (Spec 0007, Abschnitt 6) — bewusst als
//! reine, von Tauri unabhängige `async fn` gehalten (nimmt `&Session` +
//! `&dyn EventEmitter` + `&dyn ProfileStore`, keinen `tauri::AppHandle`
//! direkt), damit sie ohne laufende Tauri-Runtime gegen
//! `MockAiProvider`/`MockSshTransport` testbar ist (Aufgabenstellung Teil
//! 2, Punkt 5).
//!
//! `run_chat_turn` besteht aus 1..n Runden gegen `AiProvider::send()`
//! (`run_one_round`, je genau eine KI-Antwort: 0..n `TextDelta`s, 0..n
//! `ActionProposed`s, dann `Done`/`Error`). **Wurde in einer Runde
//! mindestens eine Aktion tatsächlich ausgeführt** (AutoExec oder vom
//! Nutzer per `respond_to_action` bestätigt — nicht bei `Deny` oder einem
//! per Bearbeiten erneut auf `Deny` laufenden `EditThenApprove`), folgt
//! automatisch eine weitere Runde mit dem inzwischen um das
//! `CommandResult`/die Notiz-Zusammenfassung erweiterten Kontext (Spec
//! Abschnitt 6, Punkt 5: "... in den `SessionContext` für die nächste
//! KI-Runde übernommen"). Ohne diesen Automatismus bekäme der Nutzer nach
//! einem ausgeführten Kommando nie eine Antwort der KI, die dessen
//! Ergebnis tatsächlich interpretiert — nur den rohen Output. Siehe
//! ADR-Vorschlag am Ende der Aufgabe.
//!
//! Das widerspricht nicht der im Projekt durchgehaltenen
//! Transparenz-/Bestätigungs-Philosophie (Spec 0002, Spec 0007 Abschnitt 5:
//! selbst `AutoExec`/`Deny` werden dem Nutzer nur *angezeigt*, nie verborgen
//! weitergesponnen): jede in einer Folgerunde neu vorgeschlagene Aktion
//! durchläuft erneut dieselbe Filter-Engine/Bestätigungslogik wie jede
//! andere auch. Begrenzt auf [`MAX_AUTO_FOLLOWUP_ROUNDS`] Runden, damit eine
//! KI, die immer wieder neue Aktionen vorschlägt, nicht unbegrenzt
//! weiterläuft.

use futures::StreamExt;
use uuid::Uuid;

use ssh_manager_core::ai::{AiError, ChatMessage, MessageContent, Role};
use ssh_manager_core::filter::{Decision, EvalContext};
use ssh_manager_core::profiles::{AiAction, NoteEditor, NoteTarget, ProfileError, ProfileStore};
use ssh_manager_core::ssh::SshError;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::ActionUserDecision;
use crate::events::{
    emit_chat_action_proposed, emit_chat_action_result, emit_chat_error, emit_chat_text_delta,
    ActionResultPayload, EventEmitter,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

/// Sicherheitsgrenze gegen eine KI, die in jeder Folgerunde erneut eine
/// (automatisch ausgeführte) Aktion vorschlägt — ohne diese Grenze könnte
/// `run_chat_turn` sonst unbegrenzt weiterlaufen. Ein einzelner
/// Nutzer-Turn, der tatsächlich so viele aufeinanderfolgende Kommandos
/// braucht, ist in der Praxis nicht zu erwarten.
const MAX_AUTO_FOLLOWUP_ROUNDS: usize = 8;

/// Die Nutzer-Nachricht muss bereits vom Aufrufer in
/// `session.context.history` eingetragen worden sein (s.
/// `crate::commands::send_chat_message`). Läuft so lange in Folgerunden
/// weiter, wie die jeweils letzte Runde mindestens eine Aktion tatsächlich
/// ausgeführt hat (s. Moduldoc), höchstens aber [`MAX_AUTO_FOLLOWUP_ROUNDS`]
/// Runden.
pub async fn run_chat_turn(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) {
    for _ in 0..MAX_AUTO_FOLLOWUP_ROUNDS {
        let executed_action = run_one_round(
            session,
            session_id,
            emitter,
            profile_store,
            action_confirmations,
        )
        .await;
        if !executed_action {
            return;
        }
    }

    emit_chat_error(
        emitter,
        session_id,
        format!(
            "Abgebrochen nach {MAX_AUTO_FOLLOWUP_ROUNDS} aufeinanderfolgenden Aktionen in einer \
             Antwort. Bitte in einer neuen Nachricht nachfragen."
        ),
    );
}

/// Genau eine KI-Antwortrunde. Gibt zurück, ob dabei mindestens eine
/// Aktion tatsächlich ausgeführt wurde (und damit eine weitere Runde
/// folgen sollte).
async fn run_one_round(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> bool {
    let request_context = session.context.lock().await.clone();
    let mut stream = session.ai_provider.send(request_context);

    let mut text_buffer = String::new();
    let mut executed_action = false;

    while let Some(event) = stream.next().await {
        match event {
            ssh_manager_core::ai::AiEvent::TextDelta(delta) => {
                emit_chat_text_delta(emitter, session_id, delta.clone());
                text_buffer.push_str(&delta);
            }
            ssh_manager_core::ai::AiEvent::ActionProposed(action) => {
                flush_text_buffer(session, &mut text_buffer).await;
                if handle_action_proposed(
                    session,
                    session_id,
                    action,
                    emitter,
                    profile_store,
                    action_confirmations,
                )
                .await
                {
                    executed_action = true;
                }
            }
            ssh_manager_core::ai::AiEvent::Done => {
                flush_text_buffer(session, &mut text_buffer).await;
                break;
            }
            ssh_manager_core::ai::AiEvent::Error(err) => {
                flush_text_buffer(session, &mut text_buffer).await;
                emit_chat_error(emitter, session_id, describe_ai_error(&err));
                break;
            }
        }
    }

    executed_action
}

fn describe_ai_error(err: &AiError) -> String {
    err.to_string()
}

async fn flush_text_buffer(session: &Session, buffer: &mut String) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    session.context.lock().await.history.push(ChatMessage {
        role: Role::Assistant,
        content: MessageContent::Text(text),
    });
}

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
async fn handle_action_proposed(
    session: &Session,
    session_id: SessionId,
    action: AiAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> bool {
    let action_id: ActionId = Uuid::new_v4();
    let decision = evaluate_action(session, &action);

    emit_chat_action_proposed(
        emitter,
        session_id,
        action_id,
        action.clone(),
        decision.clone(),
    );

    match decision {
        Decision::AutoExec => {
            execute_action(
                session,
                session_id,
                action_id,
                action,
                emitter,
                profile_store,
            )
            .await
        }
        Decision::Deny { .. } => {
            // Spec 0007 Abschnitt 5: informiert nur, keine Ausführung, kein
            // Warten auf `respond_to_action` — das Event oben ist bereits
            // die vollständige Reaktion.
            false
        }
        Decision::Confirm { .. } => {
            let rx = action_confirmations.register(action_id);
            let Ok(user_decision) = rx.await else {
                // Sender wurde gedroppt (z. B. App beendet, bevor der
                // Nutzer reagiert hat) — kein Absturz, einfach nichts
                // ausführen.
                return false;
            };
            handle_user_decision(
                session,
                session_id,
                action_id,
                action,
                user_decision,
                emitter,
                profile_store,
            )
            .await
        }
    }
}

/// `AiAction::SuggestCommand` läuft durch die Filter-Engine;
/// `AiAction::ProposeNoteUpdate` verlangt **immer** eine Bestätigung,
/// unabhängig von der Filter-Engine (Spec 0003, Abschnitt 5.2 — explizit
/// wiederholt in Spec 0007, Abschnitt 6, letzter Punkt).
fn evaluate_action(session: &Session, action: &AiAction) -> Decision {
    match action {
        AiAction::SuggestCommand { command } => {
            let ctx = EvalContext {
                server_id: session.server_id,
                tags: session.tags.clone(),
            };
            session.filter_engine.evaluate(command, &ctx)
        }
        AiAction::ProposeNoteUpdate { .. } => Decision::Confirm {
            reason: "Notiz-Aktualisierungen erfordern immer eine manuelle Bestätigung".to_string(),
        },
    }
}

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
async fn handle_user_decision(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    user_decision: ActionUserDecision,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
) -> bool {
    match user_decision {
        ActionUserDecision::Deny => {
            // Nutzer hat selbst abgelehnt — nichts weiter zu tun, das
            // Frontend weiß es bereits (es hat den Aufruf selbst gemacht).
            false
        }
        ActionUserDecision::Approve => {
            execute_action(
                session,
                session_id,
                action_id,
                action,
                emitter,
                profile_store,
            )
            .await
        }
        ActionUserDecision::EditThenApprove { command: edited } => {
            let effective_action = match &action {
                // Ein editiertes Kommando darf die Prüfung nicht umgehen
                // (Aufgabenstellung Teil 1, Punkt 4): erneut durch die
                // Filter-Engine schicken. Nur ein *hartes* `Deny`
                // überstimmt die bereits erteilte Nutzer-Freigabe — ein
                // erneutes `Confirm` (z. B. weil der bearbeitete Text
                // wieder auf die Blacklist trifft, die selbst nur auf
                // `Confirm` abbildet) nicht: der Klick auf "Ausführen" im
                // Bearbeiten-Dialog *ist* bereits die verlangte
                // Bestätigung.
                AiAction::SuggestCommand { .. } => {
                    let ctx = EvalContext {
                        server_id: session.server_id,
                        tags: session.tags.clone(),
                    };
                    let re_decision = session.filter_engine.evaluate(&edited, &ctx);
                    if let Decision::Deny { reason } = re_decision {
                        let blocked = AiAction::SuggestCommand {
                            command: edited.clone(),
                        };
                        emit_chat_action_proposed(
                            emitter,
                            session_id,
                            Uuid::new_v4(),
                            blocked,
                            Decision::Deny { reason },
                        );
                        return false;
                    }
                    AiAction::SuggestCommand { command: edited }
                }
                // `ProposeNoteUpdate` hat kein editierbares "Kommando" —
                // das Frontend bietet für diesen Aktionstyp gar kein
                // Editierfeld an (s. `crate::orchestration`-Moduldoc und
                // Frontend-Bestätigungsdialog). Träfe `EditThenApprove`
                // trotzdem ein, wird die ursprünglich vorgeschlagene
                // Aktion unverändert ausgeführt statt den (hier
                // bedeutungslosen) `command`-Text zu verwenden.
                AiAction::ProposeNoteUpdate { .. } => action,
            };
            execute_action(
                session,
                session_id,
                action_id,
                effective_action,
                emitter,
                profile_store,
            )
            .await
        }
    }
}

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde (d.h. ob ihr
/// Ergebnis in `context.history` gelandet ist) — bei einem Fehlschlag
/// (`SshError`/`ProfileError`) ist der Kontext unverändert, eine
/// automatische Folgerunde (s. Moduldoc) würde dann nur denselben
/// Vorschlag erneut auslösen, statt der KI etwas Neues mitzuteilen.
async fn execute_action(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
) -> bool {
    match action {
        AiAction::SuggestCommand { command } => {
            execute_suggested_command(session, session_id, action_id, command, emitter).await
        }
        AiAction::ProposeNoteUpdate {
            target,
            new_content,
        } => {
            execute_note_update(
                session,
                session_id,
                action_id,
                target,
                new_content,
                emitter,
                profile_store,
            )
            .await
        }
    }
}

async fn execute_suggested_command(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    command: String,
    emitter: &dyn EventEmitter,
) -> bool {
    let raw_output = {
        let mut transport = session.transport.lock().await;
        transport.execute(&command).await
    };

    match raw_output {
        Ok(output) => {
            let redacted = session.redactor.redact(&output);
            emit_chat_action_result(
                emitter,
                session_id,
                action_id,
                ActionResultPayload::Command {
                    command: command.clone(),
                    stdout: String::from_utf8_lossy(&redacted.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&redacted.stderr).into_owned(),
                    exit_code: redacted.exit_code,
                },
            );
            session.context.lock().await.history.push(ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::CommandResult {
                    command,
                    output: redacted,
                },
            });
            true
        }
        Err(err) => {
            emit_command_execution_failed(emitter, session_id, &command, &err);
            false
        }
    }
}

fn emit_command_execution_failed(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    command: &str,
    err: &SshError,
) {
    emit_chat_error(
        emitter,
        session_id,
        format!("Kommando '{command}' konnte nicht ausgeführt werden: {err}"),
    );
}

async fn execute_note_update(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    target: NoteTarget,
    new_content: String,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
) -> bool {
    let revision = ssh_manager_core::profiles::record_revision(
        target,
        new_content,
        NoteEditor::Ai {
            provider: session.ai_provider_label.clone(),
            model: session.ai_model.clone(),
        },
    );

    match profile_store.record_note_revision(&revision).await {
        Ok(()) => {
            let summary = note_update_summary(target);
            emit_chat_action_result(
                emitter,
                session_id,
                action_id,
                ActionResultPayload::NoteUpdate {
                    summary: summary.clone(),
                },
            );
            session.context.lock().await.history.push(ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::Text(summary),
            });
            true
        }
        Err(err) => {
            emit_note_update_failed(emitter, session_id, &err);
            false
        }
    }
}

fn note_update_summary(target: NoteTarget) -> String {
    match target {
        NoteTarget::Server(id) => format!("Notiz für Server {} aktualisiert.", id.0),
        NoteTarget::Group(id) => format!("Notiz für Gruppe {} aktualisiert.", id.0),
    }
}

fn emit_note_update_failed(emitter: &dyn EventEmitter, session_id: SessionId, err: &ProfileError) {
    emit_chat_error(
        emitter,
        session_id,
        format!("Notiz konnte nicht aktualisiert werden: {err}"),
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use tokio::sync::Mutex as AsyncMutex;

    use ssh_manager_core::ai::{
        default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, SessionContext,
    };
    use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyStore, Rule};
    use ssh_manager_core::profiles::{Group, GroupId, NoteRevision, ProfileResult, Server};
    use ssh_manager_core::shared::ServerId;
    use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize};

    use super::*;
    use crate::events::TestEmitter;
    use crate::session::Session;

    /// Konfigurierbar mit einer Sequenz von Runden (je ein `Vec<AiEvent>`
    /// pro `send()`-Aufruf) — nötig, um die automatische Folgerunde aus
    /// dem Moduldoc zu testen: Runde 1 schlägt z. B. ein Kommando vor,
    /// Runde 2 (nach dessen Ausführung) liefert die eigentliche
    /// Antwort-Text. Ruft `send()` öfter auf als Runden konfiguriert sind
    /// (weil eine Runde nichts ausgeführt hat und die Schleife eigentlich
    /// hätte stoppen sollen), liefert jeder weitere Aufruf nur `[Done]` —
    /// bequemer Default für Tests, die nur den ersten Round-Trip prüfen
    /// wollen, ohne dafür jede Folgerunde einzeln angeben zu müssen.
    struct MockAiProvider {
        rounds: StdMutex<std::collections::VecDeque<Vec<AiEvent>>>,
    }

    impl MockAiProvider {
        fn new(events: Vec<AiEvent>) -> Self {
            Self::with_rounds(vec![events])
        }

        fn with_rounds(rounds: Vec<Vec<AiEvent>>) -> Self {
            Self {
                rounds: StdMutex::new(rounds.into()),
            }
        }
    }

    impl AiProvider for MockAiProvider {
        fn send(
            &self,
            _context: SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
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
    struct MockSshTransport {
        responses: HashMap<String, CommandOutput>,
    }

    impl MockSshTransport {
        fn with_response(mut self, command: impl Into<String>, output: CommandOutput) -> Self {
            self.responses.insert(command.into(), output);
            self
        }
    }

    #[async_trait]
    impl ssh_manager_core::ssh::SshTransport for MockSshTransport {
        async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
            self.responses.get(command).cloned().ok_or_else(|| {
                SshError::ChannelError(format!("kein Mock-Response für '{command}'"))
            })
        }

        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            Err(SshError::ChannelError(
                "in diesem Test nicht unterstützt".to_string(),
            ))
        }

        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    use crate::policy::NoRulesPolicyStore;

    #[derive(Default)]
    struct InMemoryProfileStore {
        note_revisions: StdMutex<Vec<NoteRevision>>,
    }

    #[async_trait]
    impl ProfileStore for InMemoryProfileStore {
        async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
            Err(ssh_manager_core::profiles::ProfileError::ServerNotFound(
                *id,
            ))
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

    fn test_session(ai_events: Vec<AiEvent>, transport: MockSshTransport) -> Session {
        session_with_ai_provider(MockAiProvider::new(ai_events), transport)
    }

    fn session_with_ai_provider(
        ai_provider: MockAiProvider,
        transport: MockSshTransport,
    ) -> Session {
        Session {
            transport: AsyncMutex::new(Box::new(transport)),
            ai_provider: Box::new(ai_provider),
            context: AsyncMutex::new(SessionContext {
                system_context: "Testkontext".to_string(),
                history: Vec::new(),
                available_actions: default_action_schemas(),
            }),
            filter_engine: Box::new(FilterEngine::new(NoRulesPolicyStore)),
            server_id: ServerId::new(),
            tags: Vec::new(),
            terminal: StdMutex::new(None),
            redactor: Box::new(DefaultOutputRedactor::new()),
            ai_provider_label: "test-provider".to_string(),
            ai_model: "test-model".to_string(),
        }
    }

    fn output(stdout: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
        }
    }

    /// `NoRulesPolicyStore` (der `test_session`-Standard) landet für jedes
    /// Kommando ohne passende Regel auf `Confirm` (s. `core::filter::engine`:
    /// "keine Regel gefunden" ist der Default-Fallback) — für einen
    /// AutoExec-Test wird deshalb eine explizite `Allow`-Regel gebraucht,
    /// sonst würde der Test denselben Confirm-Wartepfad wie
    /// `test_confirm_path_waits_for_respond_to_action_before_executing`
    /// nehmen (und ohne Responder-Task ewig auf eine nie eintreffende
    /// Bestätigung hängen bleiben).
    struct AllowEverythingPolicyStore;
    impl PolicyStore for AllowEverythingPolicyStore {
        fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: "allow-all".to_string(),
                pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Allow,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
            }]
        }
    }

    #[tokio::test]
    async fn test_autoexec_path_runs_command_and_records_result() {
        let mut session = test_session(
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "ls -la".to_string(),
                }),
                AiEvent::Done,
            ],
            MockSshTransport::default().with_response("ls -la", output("total 0")),
        );
        session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        run_chat_turn(
            &session,
            Uuid::new_v4(),
            &emitter,
            &profile_store,
            &confirmations,
        )
        .await;

        let events = emitter.events.lock().unwrap().clone();
        let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            event_names,
            vec!["chat-action-proposed", "chat-action-result"]
        );
        let (_, proposed_payload) = &events[0];
        assert_eq!(proposed_payload["decision"], serde_json::json!("AutoExec"));

        let history = session.context.lock().await.history.clone();
        assert_eq!(history.len(), 1);
        assert!(matches!(
            history[0].content,
            MessageContent::CommandResult { .. }
        ));
    }

    #[tokio::test]
    async fn test_confirm_path_waits_for_respond_to_action_before_executing() {
        let session = test_session(
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    // Kein Mock-Response konfiguriert für den *originalen*
                    // Befehl — nur für den editierten (s. unten). Würde die
                    // Ausführung fälschlich vor der Bestätigung starten,
                    // schlägt der Test mit einem `ChannelError` fehl statt
                    // einfach nur zu spät zu sein.
                    command: "rm -rf /tmp/build".to_string(),
                }),
                AiEvent::Done,
            ],
            MockSshTransport::default()
                .with_response("rm -rf /tmp/build-edited", output("removed")),
        );
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();

        let turn = run_chat_turn(
            &session,
            session_id,
            &emitter,
            &profile_store,
            &confirmations,
        );
        let responder = async {
            // Simuliert das Frontend: wartet, bis `chat-action-proposed`
            // sichtbar ist, editiert dann das Kommando und bestätigt.
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
                    confirmations
                        .resolve(
                            &action_id,
                            ActionUserDecision::EditThenApprove {
                                command: "rm -rf /tmp/build-edited".to_string(),
                            },
                        )
                        .unwrap();
                    break;
                }
                tokio::task::yield_now().await;
            }
        };

        tokio::join!(turn, responder);

        let events = emitter.events.lock().unwrap();
        let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            event_names,
            vec!["chat-action-proposed", "chat-action-result"]
        );
        let (_, result_payload) = &events[1];
        assert_eq!(
            result_payload["result"]["command"],
            "rm -rf /tmp/build-edited"
        );
    }

    #[tokio::test]
    async fn test_edited_command_that_hits_deny_rule_is_blocked_not_executed() {
        // Trifft absichtlich **nur** die editierte Fassung ("*-edited"),
        // nicht das Original ("echo hi") — sonst würde schon der
        // ursprüngliche Vorschlag mit `Deny` beantwortet und der
        // Confirm-Wartepfad (den dieser Test eigentlich prüfen soll) nie
        // erreicht.
        struct DenyEditedPolicyStore;
        impl PolicyStore for DenyEditedPolicyStore {
            fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
                vec![Rule {
                    id: "deny-edited".to_string(),
                    pattern: ssh_manager_core::filter::Pattern::Glob("*-edited".to_string()),
                    action: ssh_manager_core::filter::RuleAction::Deny,
                    scope: ssh_manager_core::filter::Scope::Global,
                    priority: 0,
                }]
            }
        }

        let mut session = test_session(
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "echo hi".to_string(),
                }),
                AiEvent::Done,
            ],
            MockSshTransport::default().with_response("echo hi-edited", output("hi-edited")),
        );
        session.filter_engine = Box::new(FilterEngine::new(DenyEditedPolicyStore));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();

        let turn = run_chat_turn(
            &session,
            session_id,
            &emitter,
            &profile_store,
            &confirmations,
        );
        let responder = async {
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
                    confirmations
                        .resolve(
                            &action_id,
                            ActionUserDecision::EditThenApprove {
                                command: "echo hi-edited".to_string(),
                            },
                        )
                        .unwrap();
                    break;
                }
                tokio::task::yield_now().await;
            }
        };

        tokio::join!(turn, responder);

        let events = emitter.events.lock().unwrap().clone();
        // Zwei `chat-action-proposed` (Original + editierte, geblockte
        // Fassung), aber **kein** `chat-action-result` — nichts wurde
        // ausgeführt.
        let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            event_names,
            vec!["chat-action-proposed", "chat-action-proposed"]
        );
        assert!(events[1].1["decision"]["Deny"].is_object());
        assert!(session.context.lock().await.history.is_empty());
    }

    #[tokio::test]
    async fn test_deny_path_executes_nothing_and_does_not_block_further_events() {
        let session = test_session(
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "curl evil.example".to_string(),
                }),
                AiEvent::TextDelta("weiterer Text nach der Ablehnung".to_string()),
                AiEvent::Done,
            ],
            MockSshTransport::default(),
        );
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();

        // Hard-Blacklist matcht "curl" nicht automatisch auf Deny (nur
        // Confirm, s. core::filter), daher hier eine explizite
        // Deny-Regel, um den reinen Deny-Pfad ohne Warten zu testen.
        struct DenyCurlPolicyStore;
        impl PolicyStore for DenyCurlPolicyStore {
            fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
                vec![Rule {
                    id: "deny-curl".to_string(),
                    pattern: ssh_manager_core::filter::Pattern::Glob("curl*".to_string()),
                    action: ssh_manager_core::filter::RuleAction::Deny,
                    scope: ssh_manager_core::filter::Scope::Global,
                    priority: 0,
                }]
            }
        }
        let mut session = session;
        session.filter_engine = Box::new(FilterEngine::new(DenyCurlPolicyStore));

        run_chat_turn(
            &session,
            session_id,
            &emitter,
            &profile_store,
            &confirmations,
        )
        .await;

        let events = emitter.events.lock().unwrap().clone();
        let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        // Deny blockiert nur die Ausführung, nicht den weiteren
        // Stream-Verlauf: das TextDelta danach kommt trotzdem an.
        assert_eq!(event_names, vec!["chat-action-proposed", "chat-text-delta"]);
        assert!(session
            .context
            .lock()
            .await
            .history
            .iter()
            .any(|m| matches!(
                &m.content,
                MessageContent::Text(t) if t.contains("weiterer Text")
            )));
        assert!(!session
            .context
            .lock()
            .await
            .history
            .iter()
            .any(|m| matches!(m.content, MessageContent::CommandResult { .. })));
    }

    /// Spec 0003 Abschnitt 5.2 / Spec 0007 Abschnitt 6, letzter Punkt:
    /// `ProposeNoteUpdate` wartet **immer** auf Bestätigung, unabhängig von
    /// der Filter-Engine — hier absichtlich mit `AllowEverythingPolicyStore`
    /// (die für ein `SuggestCommand` sofort `AutoExec` ergäbe), um zu
    /// zeigen, dass die Filter-Engine für diesen Aktionstyp gar nicht erst
    /// gefragt wird.
    #[tokio::test]
    async fn test_propose_note_update_always_waits_for_confirmation_and_persists() {
        let target = ssh_manager_core::profiles::NoteTarget::Server(ServerId::new());
        let mut session = test_session(
            vec![
                AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                    target,
                    new_content: "neuer Kontext".to_string(),
                }),
                AiEvent::Done,
            ],
            MockSshTransport::default(),
        );
        session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();

        let turn = run_chat_turn(
            &session,
            session_id,
            &emitter,
            &profile_store,
            &confirmations,
        );
        let responder = async {
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
                    confirmations
                        .resolve(&action_id, ActionUserDecision::Approve)
                        .unwrap();
                    break;
                }
                tokio::task::yield_now().await;
            }
        };

        tokio::join!(turn, responder);

        let events = emitter.events.lock().unwrap().clone();
        let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            event_names,
            vec!["chat-action-proposed", "chat-action-result"]
        );
        let (_, proposed_payload) = &events[0];
        assert!(
            proposed_payload["decision"]["Confirm"].is_object(),
            "ProposeNoteUpdate muss immer Confirm sein, nie AutoExec"
        );

        assert_eq!(profile_store.note_revisions.lock().unwrap().len(), 1);
        assert!(session
            .context
            .lock()
            .await
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("aktualisiert"))));
    }

    /// Kern des ADR-Vorschlags in diesem Modul-Doc: nach einem
    /// tatsächlich ausgeführten Kommando bekommt die KI automatisch eine
    /// Folgerunde, um dessen Ergebnis in eine Antwort zu fassen — vorher
    /// endete `run_chat_turn` stattdessen wortlos nach dem
    /// `chat-action-result`.
    #[tokio::test]
    async fn test_executed_action_triggers_automatic_followup_round_with_final_answer() {
        let mut session = session_with_ai_provider(
            MockAiProvider::with_rounds(vec![
                vec![
                    AiEvent::ActionProposed(AiAction::SuggestCommand {
                        command: "uptime".to_string(),
                    }),
                    AiEvent::Done,
                ],
                vec![
                    AiEvent::TextDelta("Der Server läuft seit 3 Tagen.".to_string()),
                    AiEvent::Done,
                ],
            ]),
            MockSshTransport::default().with_response("uptime", output("up 3 days")),
        );
        session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        run_chat_turn(
            &session,
            Uuid::new_v4(),
            &emitter,
            &profile_store,
            &confirmations,
        )
        .await;

        let events = emitter.events.lock().unwrap().clone();
        let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            event_names,
            vec![
                "chat-action-proposed",
                "chat-action-result",
                "chat-text-delta"
            ]
        );

        let history = session.context.lock().await.history.clone();
        assert!(history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::CommandResult { .. })));
        assert!(history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("3 Tagen"))));
    }

    /// Sicherheitsgrenze: eine KI, die in jeder Runde erneut ein Kommando
    /// vorschlägt, läuft nicht unbegrenzt weiter, sondern bricht nach
    /// [`MAX_AUTO_FOLLOWUP_ROUNDS`] Runden mit einer `chat-error`-Meldung
    /// ab.
    #[tokio::test]
    async fn test_runaway_followup_rounds_are_bounded() {
        struct RepeatingAiProvider;
        impl AiProvider for RepeatingAiProvider {
            fn send(
                &self,
                _context: SessionContext,
            ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
                Box::pin(futures::stream::iter(vec![
                    AiEvent::ActionProposed(AiAction::SuggestCommand {
                        command: "echo again".to_string(),
                    }),
                    AiEvent::Done,
                ]))
            }
        }

        let mut session = session_with_ai_provider(
            MockAiProvider::new(Vec::new()),
            MockSshTransport::default().with_response("echo again", output("again")),
        );
        session.ai_provider = Box::new(RepeatingAiProvider);
        session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        run_chat_turn(
            &session,
            Uuid::new_v4(),
            &emitter,
            &profile_store,
            &confirmations,
        )
        .await;

        let events = emitter.events.lock().unwrap().clone();
        let proposed_count = events
            .iter()
            .filter(|(name, _)| name == "chat-action-proposed")
            .count();
        assert_eq!(proposed_count, MAX_AUTO_FOLLOWUP_ROUNDS);
        assert_eq!(events.last().unwrap().0, "chat-error");
    }
}
