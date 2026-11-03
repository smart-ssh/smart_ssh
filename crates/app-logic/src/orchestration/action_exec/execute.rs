//! Split out of `action_exec.rs` (issue #859, pure move).

use super::*;

use ssh_manager_core::ai::{ChatMessage, MessageContent, OutputRedactor, Role};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::profiles::{AiAction, ProfileStore};
use ssh_manager_core::ssh::{CommandOutput, ExecOutcome};

use crate::events::{emit_chat_action_result, emit_chat_error, ActionResultPayload, EventEmitter};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::super::approval::{ApprovedAction, ApprovedCommand};
use super::super::chat_turn::{push_history_scoped, write_ledger_entry};
use super::super::notes::execute_note_update;
use super::super::remote_files::{execute_read_remote_file, execute_write_remote_file};

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde (d.h. ob ihr
/// Ergebnis in `context.history` gelandet ist) — bei einem Fehlschlag
/// (`SshError`/`ProfileError`) ist der Kontext unverändert, eine
/// automatische Folgerunde (s. Moduldoc) würde dann nur denselben
/// Vorschlag erneut auslösen, statt der KI etwas Neues mitzuteilen.
#[expect(clippy::too_many_arguments, reason = "one argument per input")]
pub(super) async fn execute_action(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: ApprovedAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    persist: bool,
    // spec-reviewer-Fund (Review dieses Schritts): vorher leitete
    // `execute_suggested_command` die `LedgerSource` seines
    // `CommandExecuted`-Eintrags aus `persist` ab (`persist == false` <=>
    // MCP) — funktioniert nur zufällig, weil `persist` heute dieselbe
    // Bedeutung trägt, UND schreibt ein vom Menschen *editiertes*
    // Kommando (`EditThenApprove`) fälschlich `Ai` statt `User` zu. Explizit
    // durchgereicht statt aus einem semantisch anderen Flag abgeleitet —
    // nur der `SuggestCommand`-Zweig unten braucht ihn (Etappe-1-Scope,
    // s. ADR 0047 Punkt 3), die anderen Aktionstypen ignorieren ihn.
    ledger_source: LedgerSource,
    // Spec 0068, Teil 3: s. `handle_user_decision`.
    sudo_fallback_announced: bool,
) -> bool {
    let action = match action {
        ApprovedAction::Command(command) => {
            return execute_suggested_command(
                session,
                session_id,
                action_id,
                command,
                emitter,
                profile_store,
                persist,
                ledger_source,
            )
            .await;
        }
        ApprovedAction::FileWrite(approved) => {
            return execute_write_remote_file(
                session,
                session_id,
                action_id,
                approved,
                emitter,
                profile_store,
                persist,
                sudo_fallback_announced,
            )
            .await;
        }
        ApprovedAction::Other(action) => action,
    };
    match action {
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
                persist,
            )
            .await
        }
        AiAction::GenerateDocument { .. } => {
            unreachable!("GenerateDocument braucht nie eine Bestätigung, s. evaluate_action")
        }
        AiAction::AskUser { .. } => {
            unreachable!("AskUser is never executed, see run_one_round")
        }
        AiAction::ReadRemoteFile { path } => {
            execute_read_remote_file(session, session_id, action_id, path, emitter, persist).await
        }
        AiAction::SuggestCommand { .. } | AiAction::WriteRemoteFile { .. } => {
            unreachable!("approved commands and file writes are wrapped by ApprovedAction")
        }
    }
}

/// Spec 0018, Abschnitt 3: erkennt einen `sudo`/`doas`-Aufruf, der das
/// **gesamte** Kommando bildet (kein Treffer mitten in einer Kommandokette,
/// s. Spec-Dokument für die bewusste Einschränkung). Liefert bei Treffer
/// das Präfix (`"sudo"`/`"doas"`) zurück.
pub(super) fn detect_elevation_prefix(command: &str) -> Option<&'static str> {
    let trimmed = command.trim_start();
    ["sudo", "doas"].into_iter().find(|&prefix| {
        trimmed
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// Spec 0018, Abschnitt 5: liefert das für `-S`+Stdin-Passworteingabe
/// vorbereitete Kommando, falls `command` elevation-fähig ist und noch kein
/// eigenes `-S`/`-A`-Flag enthält (dann haben KI/Nutzer die
/// Passworteingabe bereits selbst vorgesehen, nicht gegensteuern).
pub(super) fn command_with_stdin_password_flag(command: &str) -> Option<String> {
    let prefix = detect_elevation_prefix(command)?;
    if command.contains("-S") || command.contains("-A") {
        return None;
    }
    let trimmed = command.trim_start();
    let rest = &trimmed[prefix.len()..];
    Some(format!("{prefix} -S{rest}"))
}

/// Spec 0018, Abschnitt 7 / Spec 0068, Teil 3: ob die Aktion das hinterlegte Sudo-Passwort nutzt bzw. nutzen KANN —
/// steuert die Ankündigung im Bestätigungsdialog (`usesStoredSudoPassword`)
/// und die Eskalation auf `Confirm`.
///
/// Spec 0068, Teil 3 (Release-Gate C): für `WriteRemoteFile` auch dann,
/// wenn Sudo erst als Fallback nach einem Rechte-Fehler nötig würde
/// (`execute_write_remote_file`) — sonst schöbe die App Sudo nach der
/// Bestätigung still nach. Der Fallback dort bekommt genau den im Dialog
/// gesendeten Wert durchgereicht, ist also an die Ankündigung gebunden.
pub(super) fn uses_stored_sudo_password(session: &Session, action: &AiAction) -> bool {
    session.safety.sudo_password.is_some()
        && match action {
            AiAction::SuggestCommand { command } => detect_elevation_prefix(command).is_some(),
            AiAction::WriteRemoteFile { .. } => true,
            _ => false,
        }
}

/// Meldet einen bei der Ausführung einer Aktion aufgetretenen Fehler sowohl
/// als `chat-error`-Event (sofort sichtbare Meldung im UI) als auch als
/// `ActionResult`-Eintrag im Kontext (Spec 0021, Abschnitt 3, analog zum
/// `Decision::Deny`-Fall) — ohne Letzteres bekäme die KI den Fehlschlag nie
/// zu sehen und der Turn bräche ohne Folgerunde ab, obwohl aus ihrer Sicht
/// offen bliebe, was aus der Aktion geworden ist (das war der aus einem
/// Nutzer-Bugreport bekannte "Chat hängt nach SFTP-Fehler"-Fall). Gibt immer
/// `true` zurück, zur direkten Verwendung als `return`-Ausdruck an den
/// bisherigen `false`-Rückgabestellen.
pub(crate) async fn emit_action_error(
    session: &Session,
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    message: String,
    code: Option<&'static str>,
    persist: bool,
) -> bool {
    emit_chat_error(emitter, session_id, message.clone(), code);
    push_history_scoped(
        session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(message),
        },
        persist,
    )
    .await;
    true
}

/// A command prepared for the transport call (sudo stdin handling and the
/// edition's outgoing transform applied). `command` and `effective_command`
/// are what is shown and stored; `wire_*` exist only for the transport call.
pub(super) struct PreparedCommand {
    command: String,
    /// `command` with the stdin password flag, when the stored sudo password
    /// applies (Spec 0018, Abschnitt 5).
    effective_command: Option<String>,
    /// Issue #464 (Spec 0038): the edition's transform of the approved command.
    wire_command: Option<secrecy::SecretString>,
    wire_effective_command: Option<String>,
    sudo_password: Option<std::sync::Arc<secrecy::SecretString>>,
}

impl PreparedCommand {
    /// Pure part of the preparation: derive the sudo-rewritten variants.
    pub(super) fn new(
        command: String,
        wire_command: Option<secrecy::SecretString>,
        sudo_password: Option<std::sync::Arc<secrecy::SecretString>>,
    ) -> Self {
        use secrecy::ExposeSecret;
        // Spec 0018, Abschnitt 5: only rewrite/feed stdin when a password is
        // actually stored.
        let effective_command = sudo_password
            .as_ref()
            .and_then(|_| command_with_stdin_password_flag(&command));
        let wire_effective_command = match &wire_command {
            Some(prepared) => sudo_password
                .as_ref()
                .and_then(|_| command_with_stdin_password_flag(prepared.expose_secret())),
            None => effective_command.clone(),
        };
        Self {
            command,
            effective_command,
            wire_command,
            wire_effective_command,
            sudo_password,
        }
    }

    /// The text for the transport call and the stdin bytes: the stored sudo
    /// password goes to stdin only when the command was rewritten to read it.
    pub(super) fn wire_text_and_stdin(&self) -> (std::borrow::Cow<'_, str>, Option<Vec<u8>>) {
        use secrecy::ExposeSecret;
        let stdin = match (&self.wire_effective_command, &self.sudo_password) {
            (Some(_), Some(password)) => {
                let mut stdin = password.expose_secret().as_bytes().to_vec();
                stdin.push(b'\n');
                Some(stdin)
            }
            _ => None,
        };
        let text = match (&self.wire_effective_command, &stdin) {
            (Some(rewritten), Some(_)) => rewritten.as_str(),
            _ => match &self.wire_command {
                Some(prepared) => prepared.expose_secret(),
                None => self.command.as_str(),
            },
        };
        (std::borrow::Cow::Borrowed(text), stdin)
    }
}

/// Applies the outgoing transform and the sudo stdin handling to the approved
/// command. `Err` carries the message for `emit_action_error`.
async fn prepare_command(
    session: &Session,
    profile_store: &dyn ProfileStore,
    command: String,
) -> Result<PreparedCommand, String> {
    // Issue #464 (Spec 0038): the edition's transform runs on the final
    // approved command, right before the transport call.
    let wire_command = super::super::outgoing::prepare_for_send(
        session,
        profile_store,
        ssh_manager_core::outgoing::OutgoingKind::Command,
        &command,
    )
    .await?;
    Ok(PreparedCommand::new(
        command,
        wire_command,
        session.safety.sudo_password.get(),
    ))
}

/// Starts the command, streams its live output and waits for the result.
/// Also returns the "waiting for input" flag set by the live forwarder.
async fn run_prepared_command(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    emitter: &dyn EventEmitter,
    prepared: &PreparedCommand,
) -> (
    Result<ExecOutcome, ssh_manager_core::ssh::SshError>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    // Spec 0027, Abschnitt 3: registered before the call so that
    // `commands::cancel_running_command` can hit this `action_id` at any time
    // during execution. The call below consumes `cancel_rx`; the `resolve()`
    // afterwards removes the registry entry in every case (otherwise each
    // regularly finished command would leave an entry behind).
    let cancel_rx = session
        .conn
        .running_command_cancellations
        .register(action_id);

    // Issue #325 (Spec 0106): the transport additionally hands every output
    // chunk to `live_sink` while the command runs; `forward_live_output`
    // redacts and throttles it into `chat-action-output` events. Display
    // only. The forwarder ends when the transport drops the sink.
    let (live_sink, live_chunks) = tokio::sync::mpsc::unbounded_channel();
    // Issue #327: set by the forwarder while the "probably waiting for
    // input" hint is shown. Backend-owned.
    let waiting_for_input = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let execution = async {
        // Issue #328: AI commands of one session stay serial through the
        // command slot, but the transport lock is only held to open the exec
        // channel.
        let Some(_slot) = session.transport.begin_command(action_id).await else {
            return Err(ssh_manager_core::ssh::SshError::ChannelError(
                "session is disconnecting".to_string(),
            ));
        };
        let (run_command, stdin) = prepared.wire_text_and_stdin();
        // Spec 0114 §4: keep the device awake while the command runs.
        let _activity = crate::background_keepalive::background_activity();
        let mut transport = session.transport.lock().await;
        match transport.start_exec(&run_command, stdin.as_deref()).await? {
            Some(running) => {
                drop(transport);
                running.run(cancel_rx, live_sink).await
            }
            // Transport without split start/run: keeps the lock for the
            // whole command, as before.
            None => {
                transport
                    .execute_streaming(&run_command, stdin.as_deref(), cancel_rx, live_sink)
                    .await
            }
        }
    };
    let (raw_outcome, ()) = tokio::join!(
        execution,
        super::super::live_output::forward_live_output(
            live_chunks,
            session.redactor.as_ref(),
            emitter,
            session_id,
            action_id,
            waiting_for_input.clone(),
        )
    );
    let _ = session
        .conn
        .running_command_cancellations
        .resolve(&action_id, ());
    (raw_outcome, waiting_for_input)
}

/// Where the result of a finished command is reported to.
pub(super) struct CommandReport<'a> {
    pub(super) session: &'a Session,
    pub(super) session_id: SessionId,
    pub(super) action_id: ActionId,
    pub(super) emitter: &'a dyn EventEmitter,
    pub(super) persist: bool,
    pub(super) ledger_source: LedgerSource,
}

impl CommandReport<'_> {
    /// Ledger, log, result event and chat history for a finished command.
    /// `output` is still unredacted; every sink redacts on its own.
    pub(super) async fn success(
        &self,
        command: String,
        output: CommandOutput,
        cancelled: bool,
        waiting_for_input: bool,
    ) {
        let session = self.session;
        // Spec 0057, §1.1: deliberately called with the UNREDACTED `output`;
        // `write_ledger_entry` redacts centrally. `ledger_source` comes from
        // the caller, not derived from `persist` (which means something else:
        // MCP chat-persistence exclusion).
        write_ledger_entry(
            session,
            self.ledger_source,
            LedgerEntryContent::CommandExecuted {
                command: command.clone(),
                output: output.clone(),
                cancelled,
            },
        )
        .await;
        let redacted = session.redactor.redact(&output);
        // Spec 0016 Abschnitt 2/3: the command itself (e.g. `mysql
        // --password=hunter2 ...`) is redacted for the log; the event and the
        // history entry keep the executed command (Spec 0018 Abschnitt 5).
        log_command_execution(
            self.session_id,
            &session.redactor.redact_text(&command),
            &redacted,
        );
        emit_chat_action_result(
            self.emitter,
            self.session_id,
            self.action_id,
            ActionResultPayload::Command {
                command: command.clone(),
                stdout: String::from_utf8_lossy(&redacted.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&redacted.stderr).into_owned(),
                exit_code: redacted.exit_code,
                cancelled,
                truncated: redacted.truncated,
            },
        );
        let combined_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&redacted.stdout),
            String::from_utf8_lossy(&redacted.stderr)
        );
        push_history_scoped(
            session,
            ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::CommandResult {
                    command,
                    output: redacted,
                    cancelled,
                    // The flag is only meaningful for a stop; a command
                    // that ended on its own never gets the note.
                    waiting_for_input: cancelled && waiting_for_input,
                },
            },
            self.persist,
        )
        .await;
        // Spec 0039, Abschnitt 5: counts as untrusted content entering the AI
        // context already here (see `history_contains_untrusted_content`).
        session
            .safety
            .untrusted_content_ingested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        check_for_injected_instructions(
            session,
            self.session_id,
            self.emitter,
            self.action_id,
            &combined_output,
            "command_output",
        )
        .await;
    }

    /// Ledger, log and error report for a transport failure.
    async fn failure(&self, command: String, err: ssh_manager_core::ssh::SshError) -> bool {
        let session = self.session;
        // A transport/channel error says nothing about whether the command
        // reached the server, so the ledger still gets a `CommandExecuted`
        // entry. `exit_code: None` means "result unknown"; the message goes
        // into `stderr` so the central redaction in `write_ledger_entry`
        // treats it like real command output.
        write_ledger_entry(
            session,
            self.ledger_source,
            LedgerEntryContent::CommandExecuted {
                command: command.clone(),
                output: CommandOutput {
                    stdout: Vec::new(),
                    stderr: err.to_string().into_bytes(),
                    exit_code: None,
                    truncated: false,
                },
                cancelled: false,
            },
        )
        .await;
        // Spec 0016/0027: redact the command for the log as in the success case.
        log_command_execution_failed(
            self.session_id,
            &session.redactor.redact_text(&command),
            &err,
        );
        let code = err.code();
        emit_action_error(
            session,
            self.emitter,
            self.session_id,
            format!("Kommando '{command}' konnte nicht ausgeführt werden: {err}"),
            Some(code),
            self.persist,
        )
        .await
    }
}

#[expect(clippy::too_many_arguments, reason = "one argument per input")]
pub(super) async fn execute_suggested_command(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    approved: ApprovedCommand,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    persist: bool,
    ledger_source: LedgerSource,
) -> bool {
    let prepared = match prepare_command(session, profile_store, approved.as_str().to_owned()).await
    {
        Ok(prepared) => prepared,
        Err(message) => {
            return emit_action_error(session, emitter, session_id, message, None, persist).await;
        }
    };
    let (raw_outcome, waiting_for_input) =
        run_prepared_command(session, session_id, action_id, emitter, &prepared).await;
    // Spec 0018, Abschnitt 5: the command that actually ran (with `-S`,
    // without the password) goes into event/log/context.
    let command = prepared.effective_command.unwrap_or(prepared.command);
    let report = CommandReport {
        session,
        session_id,
        action_id,
        emitter,
        persist,
        ledger_source,
    };
    match raw_outcome {
        Ok(ExecOutcome { output, cancelled }) => {
            let waiting = waiting_for_input.load(std::sync::atomic::Ordering::SeqCst);
            report.success(command, output, cancelled, waiting).await;
            true
        }
        Err(err) => report.failure(command, err).await,
    }
}
