//! Spec 0010/0017/0021/0027/0065/0066: Chat-Nachrichten, laufende Sitzungen,
//! Trennen — Teil der Spec-0083-Aufteilung von `commands.rs`.

use tauri::{AppHandle, Manager, State};

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
use ssh_manager_core::profiles::ProfileStore;

use crate::event_emitter::TauriEventEmitter;
use app_logic::confirmation::ConfirmationRegistry;
use app_logic::dto::{ActionUserDecision, SessionSummaryDto};
use app_logic::error::{CommandError, CommandResult};
use app_logic::events::{emit_chat_queued_messages_sent, EventEmitter};
use app_logic::orchestration::run_chat_turn;
use app_logic::session::Session;
use app_logic::state::{ActionId, AppState, SessionId};

use super::connect::build_session_system_context;
use super::servers::resolve_server_for_note_shrink;

#[tauri::command]
pub async fn send_chat_message(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    text: String,
    // Issue #469: opake Anhang-IDs einer Edition (nie Pfade aus dem
    // Webview). `None`/leer verhält sich exakt wie bisher.
    attachment_ids: Option<Vec<String>>,
) -> CommandResult<()> {
    // Spec 0104, §4: Der Nutzer-Chat kommt nie in eine MCP-Sitzung.
    state.mcp.sessions.ensure_user_session(session_id)?;
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    // Resolved and checked before anything else: an error sends nothing.
    // Registered chat content extensions take the ids (Issue #505, ADR 0141).
    let ids = attachment_ids.as_deref().unwrap_or(&[]);
    let extension_parts = app_logic::orchestration::accept_extension_content(
        &state.chat_content,
        session.redactor.as_ref(),
        session.extension_content_key(session_id).await,
        ids,
    )?;
    send_chat_message_with_attachments_impl(
        &app,
        &TauriEventEmitter(app.clone()),
        &session,
        session_id,
        text,
        extension_parts,
        Some(&state.prompt_history_store),
        state.profile_store.as_ref(),
        &state.policy_store,
        &state.pending_action_confirmations,
    )
    .await
}

/// Spec 0065, Teil 2: „Weiter"-Knopf nach einem `chat-response-truncated`-
/// Event — schickt die Fortsetzungs-Anweisung als GANZ NORMALE Nachricht
/// durch exakt denselben Pfad wie [`send_chat_message`] (Kompaktierung,
/// Rate-Limit-Gate, Caching, Redaction, Filter-Engine) — keine Sonderbahn,
/// wie in Spec 0065 §2 explizit gefordert. Bewusst kein automatisches
/// Fortsetzen: dieses Kommando läuft nur, wenn der Nutzer aktiv den Knopf
/// klickt.
#[tauri::command]
pub async fn continue_truncated_response(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    // Spec 0104, §4: wie `send_chat_message`.
    state.mcp.sessions.ensure_user_session(session_id)?;
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    send_chat_message_impl(
        &app,
        &TauriEventEmitter(app.clone()),
        &session,
        session_id,
        app_logic::system_prompt::continue_truncated_response_instruction(
            crate::ui_language::session_prompt_language(&app),
        ),
        Some(&state.prompt_history_store),
        state.profile_store.as_ref(),
        &state.policy_store,
        &state.pending_action_confirmations,
    )
    .await
}

/// Issue #600: "Try again" on a retryable AI error card.
#[tauri::command]
pub async fn retry_chat_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    state.mcp.sessions.ensure_user_session(session_id)?;
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    retry_chat_turn_impl(
        &app,
        &TauriEventEmitter(app.clone()),
        &session,
        session_id,
        state.profile_store.as_ref(),
        &state.policy_store,
        &state.pending_action_confirmations,
    )
    .await
}

/// Kern von `send_chat_message` — herausgelöst, damit Spec 0040 Abschnitt 2
/// einen Regressionstest schreiben kann, der tatsächlich HIER einsteigt
/// (nicht erst bei `run_chat_turn`/`push_history`, s. dortiger Spec-Text:
/// "genau diese Test-Einstiegslücke hat den Fund verdeckt"). Generisch über
/// `R: tauri::Runtime` (wie `build_session_system_context`/`local_server::
/// synthetic_server`), damit Tests `tauri::test::MockRuntime` statt der
/// echten `Wry`-Runtime verwenden können. `emitter` ist bewusst ein
/// eigener Parameter statt aus `app` abgeleitet: `EventEmitter` ist nur für
/// die konkrete `AppHandle<Wry>` implementiert (s. `events.rs`), ein Test
/// mit `MockRuntime` braucht daher einen separaten `TestEmitter` statt
/// `app` doppelt zu verwenden — in Produktion sind `app`/`emitter` einfach
/// derselbe Wert (s. Aufrufer oben).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_chat_message_impl<R: tauri::Runtime>(
    app: &AppHandle<R>,
    emitter: &dyn EventEmitter,
    session: &Session,
    session_id: SessionId,
    text: String,
    prompt_history_store: Option<&persistence_sqlite::SqlitePromptHistoryStore>,
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> CommandResult<()> {
    send_chat_message_with_attachments_impl(
        app,
        emitter,
        session,
        session_id,
        text,
        Default::default(),
        prompt_history_store,
        profile_store,
        policy_store,
        action_confirmations,
    )
    .await
}

/// Wie [`send_chat_message_impl`], mit bereits aufgelösten Anhängen
/// (Issue #469). Die Grenzen werden hier erneut geprüft, bevor irgendein
/// Zustand verändert wird.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_chat_message_with_attachments_impl<R: tauri::Runtime>(
    app: &AppHandle<R>,
    emitter: &dyn EventEmitter,
    session: &Session,
    session_id: SessionId,
    text: String,
    extension_parts: app_logic::orchestration::PreparedExtensionParts,
    prompt_history_store: Option<&persistence_sqlite::SqlitePromptHistoryStore>,
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> CommandResult<()> {
    send_turn_impl(
        app,
        emitter,
        session,
        session_id,
        Some(text),
        extension_parts,
        prompt_history_store,
        profile_store,
        policy_store,
        action_confirmations,
    )
    .await
}

/// Issue #600: "Try again" after a failed AI turn. Runs a new turn over the
/// current history WITHOUT adding the user message again. Only allowed when
/// the history ends with the user's own message (so no tool call, command or
/// partial answer follows it) and no turn is running.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn retry_chat_turn_impl<R: tauri::Runtime>(
    app: &AppHandle<R>,
    emitter: &dyn EventEmitter,
    session: &Session,
    session_id: SessionId,
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> CommandResult<()> {
    send_turn_impl(
        app,
        emitter,
        session,
        session_id,
        None,
        Default::default(),
        None,
        profile_store,
        policy_store,
        action_confirmations,
    )
    .await
}

/// Shared turn entry. `text == None` means "resend the last user turn".
#[allow(clippy::too_many_arguments)]
async fn send_turn_impl<R: tauri::Runtime>(
    app: &AppHandle<R>,
    emitter: &dyn EventEmitter,
    session: &Session,
    session_id: SessionId,
    text: Option<String>,
    extension_parts: app_logic::orchestration::PreparedExtensionParts,
    prompt_history_store: Option<&persistence_sqlite::SqlitePromptHistoryStore>,
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> CommandResult<()> {
    let resend = text.is_none();
    if resend {
        let last_is_user_text = {
            let ctx = session.ai.context.lock().await;
            matches!(
                ctx.history.last(),
                Some(ChatMessage {
                    role: Role::User,
                    content: MessageContent::Text(_),
                })
            )
        };
        if !last_is_user_text {
            return Err(CommandError::with_code(
                "Nothing to retry: the last turn already produced a reply or a command.",
                app_logic::orchestration::RETRY_NOT_POSSIBLE,
            ));
        }
    }
    let text = text.unwrap_or_default();
    // Spec 0066, §2: läuft für diese Sitzung schon ein Turn, wird die
    // Nachricht nur eingereiht — sie geht mit der nächsten Anfrage an die KI
    // mit (s. `run_chat_turn`) bzw. nach Turn-Ende als neuer Turn (unten).
    //
    // Der Stopp-Reset passiert hier unter demselben Lock wie `running =
    // true` und VOR jedem `.await` (spec-reviewer-Fund, Spec 0066): ein
    // früh gedrückter Stopp für diesen Turn kann dadurch nicht mehr von
    // einem späteren Reset verschluckt werden.
    let queued_only = {
        let mut turn = session.conn.chat_turn.lock().unwrap();
        // Issue #315: während „Neuer Chat" den Chat austauscht, nichts
        // einreihen — die Nachricht würde sonst unbemerkt der ersten Anfrage
        // des neuen Chats anhängen. Abgelehnt, der Text bleibt beim Nutzer.
        if turn.swapping {
            return Err(app_logic::orchestration::chat_switching_error());
        }
        if turn.running && resend {
            return Err(CommandError::with_code(
                "Nothing to retry: a turn is already running.",
                app_logic::orchestration::RETRY_NOT_POSSIBLE,
            ));
        }
        if turn.running {
            // Eingereiht werden nur Texte; Anhänge gehen nie still verloren.
            if !extension_parts.is_empty() {
                return Err(CommandError::with_code(
                    "Die Nachricht wurde nicht gesendet: Anhänge können nicht an einen \
                     laufenden Turn angehängt werden. Bitte nach der Antwort erneut senden.",
                    app_logic::orchestration::ATTACHMENTS_WHILE_BUSY,
                ));
            }
            turn.queued.push(text.clone());
            true
        } else {
            turn.running = true;
            session
                .conn
                .auto_continue_stop
                .store(false, std::sync::atomic::Ordering::SeqCst);
            false
        }
    };
    let mut running_guard = ChatTurnRunningGuard {
        session,
        armed: !queued_only,
    };
    // Issue #505: hand the checked blocks to their extension only now, after
    // the busy check; an error sends nothing (the guard resets the turn).
    if !queued_only {
        extension_parts.persist(session.extension_content_key(session_id).await)?;
    }
    let mut extension_parts = Some(extension_parts);
    // Spec 0015, Abschnitt 3: Prompt-Historie ist eine Zusatzfunktion für
    // die Pfeiltasten-Navigation im Eingabefeld — ein Fehlschlag beim
    // Persistieren (z. B. kurzzeitig gesperrte DB) soll den eigentlichen
    // Chat-Versand nicht verhindern, deshalb best-effort statt `?`. `None`
    // gibt es nur in Tests ohne Datenbank; die App übergibt immer den Store.
    if let Some(store) = prompt_history_store.filter(|_| !resend) {
        if let Err(err) = store.record(&session.server_id, &text).await {
            eprintln!("Prompt konnte nicht in der Historie gespeichert werden: {err}");
        }
    }

    if queued_only {
        return Ok(());
    }
    let mut texts = if resend { Vec::new() } else { vec![text] };

    loop {
        // Spec 0032: `profile_store.get_server` findet den lokalen
        // Pseudo-Server nie (keine `servers`-Zeile) — ohne diesen Zweig würde
        // der Servername in JEDER Chat-Nachricht auf das generische "Server"
        // degradieren (unabhängiger Review-Pass, s. docs/adr/0026).
        let (server_name, current_tags) = if app_logic::dto::is_local(session.server_id) {
            let local = crate::local_server::synthetic_server(app);
            (local.name, local.tags)
        } else {
            match profile_store.get_server(&session.server_id).await {
                Ok(s) => (s.name, s.tags),
                Err(_) => ("Server".to_string(), session.tags.clone()),
            }
        };

        // Spec 0064: der `uname`-Banner lebt seit diesem Schritt als eigene,
        // einmalig bei `connect()` eingefügte Verlaufs-Nachricht (s.
        // `os_banner_message`-Kommentar in `connect_session`), nicht mehr in
        // `SystemContextParts` — hier also nichts mehr zu übernehmen.
        let (mut updated_system_context_parts, notes_present) = build_session_system_context(
            app,
            &server_name,
            &session.server_id,
            &current_tags,
            profile_store,
            policy_store,
        )
        .await;
        // Spec 0038, section 2a: a registered session extension adds its own
        // fenced section; it counts as untrusted content like notes.
        let session_extension = app
            .try_state::<AppState>()
            .and_then(|state| state.session_extension.clone());
        let notes_present = if let Some(extension) = session_extension.as_deref() {
            let scope = super::scope::extension_scope(app, profile_store, session.server_id).await;
            let present = app_logic::session_extension::append_extension_section(
                &mut updated_system_context_parts,
                Some(extension),
                scope.as_ref(),
                &session.redactor,
            )
            .await;
            notes_present || present
        } else {
            notes_present
        };
        // Spec 0039, Abschnitt 5: der System-Prompt wird bei JEDER
        // Nutzer-Nachricht neu gebaut — enthält er gefencte Notizen (auch
        // wenn er das schon in einer früheren Nachricht tat), muss das Flag
        // spätestens jetzt gesetzt sein. Monoton: `store(true, ...)` nur bei
        // Bedarf, ein bereits gesetztes Flag wird nie zurückgesetzt.
        if notes_present {
            session
                .safety
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        let updated_system_context = updated_system_context_parts.assemble();
        {
            let mut ctx = session.ai.context.lock().await;
            ctx.system_context = updated_system_context;
            // Issue #323: ein Wechsel der UI-Sprache gilt ab der nächsten
            // Nachricht auch für die `suggest_command`-Beschreibung — sonst
            // stünde der englische Prompt neben der deutschen Beschreibung.
            ctx.available_actions = app_logic::system_prompt::session_action_schemas(
                updated_system_context_parts.language,
            );
        }
        *session.ai.system_context_parts.lock().await = updated_system_context_parts;
        // Spec 0040, Abschnitt 2: über `push_history` statt eines direkten
        // `ctx.history.push(...)`, sonst umgeht die Nutzer-Nachricht die
        // Persistenz (Spec 0034, Abschnitt 4 verlangt ausdrücklich, dass auch
        // Nutzertext fortlaufend in `chat_messages` landet — verschlüsselt,
        // Spec 0036). Muss VOR `run_chat_turn` passieren, damit die Nachricht
        // in der DB steht, bevor der KI-Aufruf überhaupt startet.
        for text in texts {
            app_logic::orchestration::push_history(
                session,
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Text(text),
                },
            )
            .await;
        }
        if let Some(parts) = extension_parts.take() {
            app_logic::orchestration::push_extension_parts(session, parts).await;
        }

        let unanswered_injected_messages = run_chat_turn(
            session,
            session_id,
            emitter,
            profile_store,
            action_confirmations,
        )
        .await;

        // Prüfen und Zurücksetzen unter demselben Lock wie das Einreihen —
        // eine gleichzeitig ankommende Nachricht landet entweder noch in
        // `queued` (und wird hier abgeholt) oder startet selbst einen Turn.
        // Ein Folge-Turn gilt als neuer Turn: das Stopp-Flag wird (wie beim
        // Start oben) atomar zurückgesetzt — eingereihte Nachrichten werden
        // auch nach einem Stopp noch beantwortet (Spec 0066, §1).
        let next = {
            let mut turn = session.conn.chat_turn.lock().unwrap();
            if turn.queued.is_empty() && !unanswered_injected_messages {
                turn.running = false;
                None
            } else {
                session
                    .conn
                    .auto_continue_stop
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                Some(std::mem::take(&mut turn.queued))
            }
        };
        match next {
            None => {
                running_guard.armed = false;
                break;
            }
            Some(queued) => {
                if !queued.is_empty() {
                    emit_chat_queued_messages_sent(emitter, session_id);
                }
                texts = queued;
            }
        }
    }
    Ok(())
}

/// Spec 0066, §2: setzt `ChatTurnState::running` zurück, falls
/// `send_chat_message_impl` vorzeitig endet (Panic/abgebrochenes Future) —
/// sonst würde jede weitere Nachricht dieser Sitzung für immer nur
/// eingereiht. Im Normalfall entschärft (`armed = false`), weil das
/// Zurücksetzen dort atomar mit der Warteschlangen-Prüfung passiert.
struct ChatTurnRunningGuard<'a> {
    session: &'a Session,
    armed: bool,
}

impl Drop for ChatTurnRunningGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let mut turn = self
                .session
                .conn
                .chat_turn
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            turn.running = false;
        }
    }
}

/// Spec 0040, Abschnitt 6: "In Notiz übernehmen" — startet denselben
/// `ProposeNoteUpdate`-Bestätigungsablauf wie ein KI-Vorschlag, nur mit dem
/// Inhalt einer bestehenden Chat-/Ergebnis-Zeile vorbefüllt (s.
/// `app_logic::orchestration::propose_note_from_chat_content`-Doc-Kommentar).
/// Wie `send_chat_message` löst dieses Promise erst auf, wenn die Aktion
/// abgeschlossen ist (Bestätigen/Ablehnen über `respond_to_action`) — kein
/// Problem für die Tauri-IPC (nicht blockierend für den Rest der App), das
/// Frontend zeigt in der Zwischenzeit ganz normal den Bestätigungsdialog.
#[tauri::command]
pub async fn take_chat_content_into_note(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    content: String,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    app_logic::orchestration::propose_note_from_chat_content(
        &session,
        session_id,
        content,
        &TauriEventEmitter(app.clone()),
        state.profile_store.as_ref(),
        &state.pending_action_confirmations,
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn respond_to_action(
    state: State<'_, AppState>,
    // Bewusst weiterhin Teil der Signatur (Spec 0007 Abschnitt 4) und
    // *ohne* führenden Unterstrich — ein `_session_id` würde Tauris
    // camelCase-Ableitung für den vom Frontend erwarteten JSON-Schlüssel
    // verändern und den bestehenden `invoke("respond_to_action", {
    // sessionId, ... })`-Aufruf brechen. Nicht mehr geprüft (s.
    // Funktionskörper), das Frontend übergibt es aber ohnehin an jeder
    // Aufrufstelle, und ein künftiger Bedarf (Logging, gezielte Events)
    // ließe sich ohne Signaturänderung nachrüsten.
    session_id: SessionId,
    action_id: ActionId,
    decision: ActionUserDecision,
) -> CommandResult<()> {
    let _ = session_id;

    // Spec 0010: dieser Command wird jetzt auch für die Bestätigung eines
    // Notiz-Vorschlags nach `disconnect()` verwendet (s.
    // `app_logic::orchestration::suggest_note_update_on_disconnect`) — zu
    // diesem Zeitpunkt ist die Session per Design bereits aus
    // `state.sessions` entfernt. Der frühere `state.sessions.get(session_id)`-
    // Check hätte diesen (gültigen) Aufruf fälschlich mit "Session nicht
    // gefunden" abgelehnt. `pending_action_confirmations.resolve()` prüft
    // die Gültigkeit von `action_id` bereits selbst (liefert einen eigenen
    // Fehler für eine unbekannte/bereits aufgelöste ID) — der zusätzliche
    // Session-Check war ohnehin redundant dazu, nicht die einzige
    // Absicherung.
    app_logic::confirmation::resolve_action_confirmation(
        &state.pending_action_confirmations,
        &action_id,
        decision,
    )?;
    Ok(())
}

/// Spec 0027, Abschnitt 3: bricht ein aktuell laufendes, abbrechbares
/// `SuggestCommand` ab (schließt nur dessen Exec-Kanal, nicht die
/// SSH-Verbindung/Session — s. `orchestration::execute_suggested_command`).
/// Kein Fehler, falls für `action_id` gerade nichts (mehr) wartet: das
/// Kommando ist dann entweder bereits regulär beendet (Race zwischen Klick
/// und Fertigstellung) oder war nie als abbrechbar registriert — in beiden
/// Fällen wäre ein Fehler an den Nutzer für einen harmlosen zeitlichen
/// Zufall nicht angemessen.
#[tauri::command]
pub async fn cancel_running_command(
    state: State<'_, AppState>,
    action_id: ActionId,
) -> CommandResult<()> {
    let _ = state.running_command_cancellations.resolve(&action_id, ());
    Ok(())
}

/// Spec 0021, Abschnitt 5 / Spec 0066, §1: "Stopp" — bricht einen gerade
/// laufenden KI-Request sofort ab und verhindert weitere automatische
/// Runden. Ein bereits offener Bestätigungsdialog bleibt stehen (der
/// Nutzer entscheidet selbst), ein laufendes Remote-Kommando läuft zu
/// Ende.
#[tauri::command]
pub async fn stop_auto_continuation(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    session.request_auto_continue_stop();
    Ok(())
}

/// Issue #356: "Stop" link of the header while automatic reconnect attempts
/// run.
#[tauri::command]
pub async fn stop_auto_reconnect(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    if let Some(session) = state.sessions.get(session_id) {
        session.conn.auto_reconnect.stop();
    }
    Ok(())
}

/// Issue #271, Spec 0034 §11: "Neuer Chat" innerhalb der bestehenden
/// Verbindung — s. `app_logic::orchestration::start_new_chat`. Liefert die
/// ID der neuen `chat_sessions`-Zeile (`None` für einen lokalen Tab).
#[tauri::command]
pub async fn start_new_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<Option<String>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    // Der aktuell aktive Anbieter, wie beim Verbinden; ohne aktiven Anbieter
    // bleibt die Spalte leer, statt den Chat zu verhindern.
    let provider_id = super::ai_providers::active_ai_provider_config(&state)
        .await
        .ok()
        .map(|config| config.id.0);
    let new_id = app_logic::orchestration::start_new_chat(
        &session,
        session_id,
        &TauriEventEmitter(app.clone()),
        provider_id,
    )
    .await?;
    // Issue #611: tell extensions (session header items) the new chat id.
    if let Some(id) = new_id {
        app_logic::events::emit_chat_session_changed(&TauriEventEmitter(app), session_id, Some(id));
    }
    Ok(new_id.map(|id| id.to_string()))
}

#[tauri::command]
pub async fn disconnect(app: AppHandle, session_id: SessionId) -> CommandResult<()> {
    // Issue #816: the one complete close path (elevated channel, grants,
    // pending confirmation, running command, transport, status event, edit
    // copies) lives in `crate::sessions`; only disconnect-specific work
    // follows here.
    let session = crate::sessions::close_session_returning(&app, session_id)
        .await
        .ok_or("Session nicht gefunden")?;

    // Spec 0034, Abschnitt 4: "endet bei `disconnect()`" — vor dem Spawn
    // unten, damit `ended_at` zuverlässig gesetzt ist, sobald `disconnect()`
    // selbst zurückkehrt, statt von der Fertigstellung des unabhängigen
    // Notiz-Vorschlag-Tasks abzuhängen. Best-effort wie der
    // Transport-Trennvorgang oben: ein Schreibfehler hier blockiert
    // `disconnect()` nicht.
    if let (Some(store), Some(chat_session_id)) = (
        &session.persistence.chat_session_store,
        *session.persistence.chat_session_id.lock().await,
    ) {
        if let Err(err) = store.end_or_discard(chat_session_id).await {
            tracing::warn!(error = %err, "chat session mark_ended failed");
        }
    }

    // Spec 0010: läuft als eigener Hintergrund-Task, **nicht** vom
    // `disconnect()`-Command selbst awaitet — der Trennvorgang oben ist
    // bereits vollständig abgeschlossen und das Event bereits gesendet,
    // bevor dieser Task überhaupt startet. `app.state::<AppState>()` statt
    // des ursprünglichen `state`-Parameters: Letzterer ist an die Lebenszeit
    // dieses einen Command-Aufrufs gebunden, der spawnte Task läuft aber
    // potenziell noch, nachdem `disconnect()` selbst längst zurückgekehrt
    // ist (wartet auf eine KI-Antwort plus ggf. auf die Nutzerbestätigung).
    let app_for_suggestion = app.clone();
    // Issue #879: app scope, not session scope — outlives the session, but a
    // pending confirmation is dropped (rejected) when the app exits.
    crate::app_scope(&app).spawn("on-disconnect-suggestions", async move {
        let state = app_for_suggestion.state::<AppState>();
        // Spec 0034, Abschnitt 7: läuft vor dem Notiz-Vorschlag im selben
        // Hintergrund-Task (sequentiell, keine zweite parallele
        // `AiProvider::send()`-Anfrage auf demselben Provider) — beide
        // sind unabhängige, optionale "beim Trennen"-Extras, s. jeweilige
        // Doc-Kommentare zur genauen Auslösebedingung.
        app_logic::orchestration::generate_session_title_on_disconnect(
            &session,
            session_id,
            &TauriEventEmitter(app_for_suggestion.clone()),
        )
        .await;
        let note_update_suggested = app_logic::orchestration::suggest_note_update_on_disconnect(
            &session,
            session_id,
            &TauriEventEmitter(app_for_suggestion.clone()),
            state.profile_store.as_ref(),
            &state.pending_action_confirmations,
        )
        .await;
        // Spec 0057, §4.2 (Etappe 4): läuft nur, wenn der Update-Vorschlag
        // oben KEINEN eigenen Vorschlag gemacht hat — s. `orchestration::
        // should_suggest_note_shrink`-Doc-Kommentar für das
        // Zusammenspiel-Design (nie zwei konkurrierende Notiz-Dialoge am
        // selben Verbindungsende).
        if app_logic::orchestration::should_suggest_note_shrink(note_update_suggested) {
            let server = resolve_server_for_note_shrink(
                &app_for_suggestion,
                state.profile_store.as_ref(),
                session.server_id,
            )
            .await;
            if let Some(server) = server {
                app_logic::orchestration::suggest_note_shrink_on_disconnect(
                    &TauriEventEmitter(app_for_suggestion.clone()),
                    server.id,
                    server.name,
                    &server.notes,
                );
            }
        }
    });

    Ok(())
}

// --- Spec 0017: Multi-Tab-Sessions -----------------------------------------

/// Spec 0017, Abschnitt 2: maßgebliche Quelle dafür, welche Sessions
/// tatsächlich offen sind — dient dem Wiederherstellen der Tab-Leiste beim
/// Frontend-Neuladen (Dev-Modus/Hot-Reload), statt von einem leeren
/// Frontend-State auszugehen. `server_name` wird hier (nicht in
/// `SessionManager::snapshot`) aufgelöst, da `SessionManager` bewusst keinen
/// `ProfileStore`-Zugriff hat (reines Session-Bookkeeping). Schlägt die
/// Auflösung fehl (Server inzwischen gelöscht, während die Session noch
/// offen ist), wird ein Platzhaltername verwendet statt den ganzen Aufruf
/// mit `?` scheitern zu lassen — eine einzelne verwaiste Session soll nicht
/// die gesamte Tab-Leisten-Wiederherstellung blockieren.
#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> CommandResult<Vec<SessionSummaryDto>> {
    let mut result = Vec::new();
    for entry in state.sessions.snapshot() {
        let server_name = state
            .profile_store
            .get_server(&entry.server_id)
            .await
            .map(|s| s.name)
            .unwrap_or_else(|_| "Unbekannter Server".to_string());
        let chat_session_id = match state.sessions.get(entry.session_id) {
            Some(session) => session
                .persistence
                .chat_session_id
                .lock()
                .await
                .map(|id| id.to_string()),
            None => None,
        };
        result.push(SessionSummaryDto {
            chat_session_id,
            session_id: entry.session_id,
            server_id: entry.server_id,
            server_name,
            status: entry.status,
            has_pending_action: entry.has_pending_action,
            mcp: state.mcp.sessions.info(entry.session_id).map(|info| {
                app_logic::dto::McpSessionDto {
                    client_name: info.client_name,
                }
            }),
        });
    }
    Ok(result)
}

/// Spec 0034, Abschnitt 6/8: die bereits geladene Historie eines Tabs — für
/// `connect()` immer leer, für `resume_chat_session()` die VOLLSTÄNDIGE aus
/// der DB geladene Historie (Spec 0057, §3.2: Kompaktierung ist
/// budgetbewusst und läuft erst vor dem nächsten `send()`, nicht schon
/// beim Laden — s. `connect_session`s `resume`-Zweig). Liest
/// direkt aus der laufenden `Session` (nicht erneut aus der DB), damit das
/// Frontend exakt das sieht, womit die Session tatsächlich gestartet ist.
#[tauri::command]
pub async fn get_chat_history(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<Vec<app_logic::dto::ChatHistoryEntryDto>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    let history = session.ai.context.lock().await.history.clone();
    Ok(history
        .into_iter()
        .map(app_logic::dto::ChatHistoryEntryDto::from)
        .collect())
}

#[cfg(test)]
mod tests_send_persistence;
