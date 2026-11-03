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
#[allow(clippy::too_many_arguments)]
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
    session.sudo_password.is_some()
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

#[allow(clippy::too_many_arguments)]
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
    let command = approved.as_str().to_owned();
    // Spec 0018, Abschnitt 5: nur umschreiben/Stdin füttern, wenn tatsächlich
    // ein Passwort hinterlegt ist — sonst unverändertes Verhalten
    // (`execute()` wie bisher, scheitert wie gewohnt ohne TTY).
    let sudo_password = session.sudo_password.get();
    // Issue #464 (Spec 0038): the edition's transform runs on the final
    // approved command, right before the transport call. Everything that
    // is shown or stored below uses `command`; `wire_command` exists only
    // for the transport call.
    let wire_command = match super::super::outgoing::prepare_for_send(
        session,
        profile_store,
        ssh_manager_core::outgoing::OutgoingKind::Command,
        &command,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(message) => {
            return emit_action_error(session, emitter, session_id, message, None, persist).await;
        }
    };
    let effective_command = sudo_password
        .as_ref()
        .and_then(|_| command_with_stdin_password_flag(&command));
    let wire_effective_command = match &wire_command {
        Some(prepared) => {
            use secrecy::ExposeSecret;
            sudo_password
                .as_ref()
                .and_then(|_| command_with_stdin_password_flag(prepared.expose_secret()))
        }
        None => effective_command.clone(),
    };

    // Spec 0027, Abschnitt 3: vor dem eigentlichen Aufruf registriert, damit
    // `commands::cancel_running_command` diese `action_id` jederzeit
    // während der Ausführung treffen kann. Der Aufruf unten (egal welcher
    // Zweig) konsumiert `cancel_rx` vollständig — der abschließende
    // `resolve()`-Aufruf danach räumt den Registry-Eintrag in **jedem**
    // Fall auf (regulär beendet: Empfänger bereits gedroppt, `send()`
    // schlägt harmlos fehl; abgebrochen: Eintrag existiert dann schon
    // nicht mehr, weil genau dieser `resolve()`-Aufruf — aus
    // `cancel_running_command` — die Ausführung erst beendet hat) — ohne
    // dieses Aufräumen bliebe für jedes regulär beendete Kommando ein
    // nie entfernter Eintrag in der Registry zurück.
    let cancel_rx = session.running_command_cancellations.register(action_id);

    // Issue #325 (Spec 0106): the transport additionally hands every output
    // chunk to `live_sink` while the command runs; `forward_live_output`
    // redacts and throttles it into `chat-action-output` events. Display
    // only — the outcome below (history, ledger, AI) is unchanged. The
    // forwarder ends when the transport drops the sink, i.e. before the
    // final result is emitted.
    let (live_sink, live_chunks) = tokio::sync::mpsc::unbounded_channel();
    // Issue #327: set by the forwarder while the "probably waiting for
    // input" hint is shown. Backend-owned, so the cancel note does not
    // depend on anything the frontend claims.
    let waiting_for_input = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let execution = async {
        // Issue #328: AI commands of one session stay serial through the
        // command slot, but the transport lock is only held to open the exec
        // channel. While the command runs, file browser, terminal and
        // disconnect can use the transport.
        let Some(_slot) = session.transport.begin_command(action_id).await else {
            return Err(ssh_manager_core::ssh::SshError::ChannelError(
                "session is disconnecting".to_string(),
            ));
        };
        let stdin = match (&wire_effective_command, &sudo_password) {
            (Some(_), Some(password)) => {
                use secrecy::ExposeSecret;
                let mut stdin = password.expose_secret().as_bytes().to_vec();
                stdin.push(b'\n');
                Some(stdin)
            }
            _ => None,
        };
        let run_command = match (&wire_effective_command, &stdin) {
            (Some(rewritten), Some(_)) => rewritten.as_str(),
            _ => match &wire_command {
                Some(prepared) => {
                    use secrecy::ExposeSecret;
                    prepared.expose_secret()
                }
                None => command.as_str(),
            },
        };
        // Spec 0114 §4: keep the device awake while the command runs.
        let _activity = crate::background_keepalive::background_activity();
        let mut transport = session.transport.lock().await;
        match transport.start_exec(run_command, stdin.as_deref()).await? {
            Some(running) => {
                drop(transport);
                running.run(cancel_rx, live_sink).await
            }
            // Transport without split start/run: keeps the lock for the
            // whole command, as before.
            None => {
                transport
                    .execute_streaming(run_command, stdin.as_deref(), cancel_rx, live_sink)
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
        .running_command_cancellations
        .resolve(&action_id, ());
    // Spec 0018, Abschnitt 5: das tatsächlich ausgeführte Kommando (mit
    // `-S`, ohne Passwort) landet in Ergebnis-Event/Log/Kontext — voll
    // transparent, da nie das Passwort selbst enthalten.
    let command = effective_command.unwrap_or(command);

    match raw_outcome {
        Ok(ExecOutcome { output, cancelled }) => {
            // Spec 0057, §1.1, dritter Punkt: "Kommando ausgeführt +
            // Ergebnis (stdout/stderr/exit)". Bewusst mit dem noch
            // UNREDIGIERTEN `output` aufgerufen — `write_ledger_entry`
            // redigiert selbst zentral (s. dortiger Doc-Kommentar), eine
            // hier vorab redigierte Fassung würde nur doppelt redigieren,
            // ohne einen Sicherheitsgewinn. `ledger_source` kommt jetzt
            // explizit vom Aufrufer (`handle_action_proposed`/
            // `handle_user_decision`), nicht mehr aus `persist` abgeleitet
            // — spec-reviewer-Fund (Review dieses Schritts): `persist`
            // beschreibt einen ganz anderen Sachverhalt
            // (Chat-Persistenz-Ausschluss für MCP) und hätte ein vom
            // Menschen editiertes Kommando (`EditThenApprove`) fälschlich
            // `Ai` zugeschrieben.
            write_ledger_entry(
                session,
                ledger_source,
                LedgerEntryContent::CommandExecuted {
                    command: command.clone(),
                    output: output.clone(),
                    cancelled,
                },
            )
            .await;
            let redacted = session.redactor.redact(&output);
            // Unabhängiger Review-Pass (Spec 0016): Spec 0016 Abschnitt 2/3
            // verlangt dieselbe Redaction-Regel für Logs wie für den
            // tatsächlichen API-Request — bisher lief nur `output` durch
            // den Redactor, das Kommando selbst (kann z. B. `mysql
            // --password=hunter2 ...` sein) landete roh im Log. Nur der
            // LOG-Aufruf bekommt die redigierte Fassung; das Event/der
            // Kontext-Eintrag unten bleibt bewusst unverändert (Spec 0018
            // Abschnitt 5: "voll transparent" für das tatsächlich
            // ausgeführte Kommando gegenüber Nutzer/KI).
            log_command_execution(
                session_id,
                &session.redactor.redact_text(&command),
                &redacted,
            );
            emit_chat_action_result(
                emitter,
                session_id,
                action_id,
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
                        waiting_for_input: cancelled
                            && waiting_for_input.load(std::sync::atomic::Ordering::SeqCst),
                    },
                },
                persist,
            )
            .await;
            // Spec 0039, Abschnitt 5: `CommandResult` wird erst beim
            // tatsächlichen Versand an den KI-Provider über `ai::
            // fence_untrusted` in <stdout>/<stderr>-Tags gepackt
            // (`ai-providers::format_command_result`) — zählt aber schon
            // hier als "Inhalt aus einer nicht vertrauenswürdigen Quelle
            // in den KI-Kontext gelangt", s. `session::history_contains_
            // untrusted_content`, das denselben Fall genauso behandelt.
            session
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
            check_for_injected_instructions(
                session,
                session_id,
                emitter,
                action_id,
                &combined_output,
                "command_output",
            )
            .await;
            true
        }
        Err(err) => {
            // spec-reviewer-Fund (Review dieses Schritts): vorher blieb der
            // Fehlerfall ganz ohne `CommandExecuted`-Eintrag — im Ledger
            // sah es dann so aus, als sei das Kommando nie ausgeführt
            // worden, obwohl es den Server ggf. bereits erreicht hat (ein
            // Transport-/Kanalfehler sagt nichts darüber aus, ob es dort
            // angekommen ist). `exit_code: None` markiert bewusst "Ergebnis
            // unbekannt", nicht "erfolgreich beendet"; die Fehlermeldung
            // selbst landet in `stderr`, damit sie beim (zentralen)
            // Redigieren in `write_ledger_entry` denselben Behandlung
            // durchläuft wie eine echte Kommando-Ausgabe.
            write_ledger_entry(
                session,
                ledger_source,
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
            // Unabhängiger Review-Pass (Spec 0016/0027): derselbe Fund wie im
            // Erfolgsfall oben (`redact_text` vor dem Log) - ohne das würde
            // z. B. `mysql --password=hunter2 ...` bei einem Kanalfehler im
            // Klartext im Logfile landen, obwohl der Erfolgspfad dasselbe
            // Kommando korrekt redigiert.
            log_command_execution_failed(session_id, &session.redactor.redact_text(&command), &err);
            let code = err.code();
            emit_action_error(
                session,
                emitter,
                session_id,
                format!("Kommando '{command}' konnte nicht ausgeführt werden: {err}"),
                Some(code),
                persist,
            )
            .await
        }
    }
}
