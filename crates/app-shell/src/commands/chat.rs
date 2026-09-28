//! Spec 0010/0017/0021/0027/0065/0066: Chat-Nachrichten, laufende Sitzungen,
//! Trennen — Teil der Spec-0083-Aufteilung von `commands.rs`.

use tauri::{AppHandle, Manager, State};

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
use ssh_manager_core::profiles::ProfileStore;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionUserDecision, SessionSummaryDto};
use crate::error::CommandResult;
use crate::event_emitter::TauriEventEmitter;
use crate::events::{
    emit_chat_queued_messages_sent, emit_connection_status_changed, ConnectionStatus, EventEmitter,
};
use crate::orchestration::run_chat_turn;
use crate::session::Session;
use crate::state::{ActionId, AppState, SessionId};

use super::connect::build_session_system_context;
use super::servers::resolve_server_for_note_shrink;
use super::sftp::edit_session_dir;

#[tauri::command]
pub async fn send_chat_message(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    text: String,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    send_chat_message_impl(
        &app,
        &TauriEventEmitter(app.clone()),
        &session,
        session_id,
        text,
        state.prompt_history_store.as_ref(),
        state.profile_store.as_ref(),
        &state.policy_store,
        &state.pending_action_confirmations,
    )
    .await
}

/// Spec 0065, Teil 2: sinngemäß aus der Spec-Skizze übernommen (dort bereits
/// als "sinngemäß"-Formulierung vorgegeben) — analog zu
/// `orchestration::TITLE_GENERATION_INSTRUCTION`, dieselbe Konvention.
const CONTINUE_TRUNCATED_RESPONSE_INSTRUCTION: &str =
    "Deine letzte Antwort wurde wegen des Längenlimits abgeschnitten. Fahre exakt an der \
     Stelle fort, an der sie endete, ohne den bisherigen Teil zu wiederholen.";

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
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    send_chat_message_impl(
        &app,
        &TauriEventEmitter(app.clone()),
        &session,
        session_id,
        CONTINUE_TRUNCATED_RESPONSE_INSTRUCTION.to_string(),
        state.prompt_history_store.as_ref(),
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
async fn send_chat_message_impl<R: tauri::Runtime>(
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
    // Spec 0066, §2: läuft für diese Sitzung schon ein Turn, wird die
    // Nachricht nur eingereiht — sie geht mit der nächsten Anfrage an die KI
    // mit (s. `run_chat_turn`) bzw. nach Turn-Ende als neuer Turn (unten).
    //
    // Der Stopp-Reset passiert hier unter demselben Lock wie `running =
    // true` und VOR jedem `.await` (spec-reviewer-Fund, Spec 0066): ein
    // früh gedrückter Stopp für diesen Turn kann dadurch nicht mehr von
    // einem späteren Reset verschluckt werden.
    let queued_only = {
        let mut turn = session.chat_turn.lock().unwrap();
        if turn.running {
            turn.queued.push(text.clone());
            true
        } else {
            turn.running = true;
            session
                .auto_continue_stop
                .store(false, std::sync::atomic::Ordering::SeqCst);
            false
        }
    };
    let mut running_guard = ChatTurnRunningGuard {
        session,
        armed: !queued_only,
    };
    // Spec 0015, Abschnitt 3: Prompt-Historie ist eine Zusatzfunktion für
    // die Pfeiltasten-Navigation im Eingabefeld — ein Fehlschlag beim
    // Persistieren (z. B. kurzzeitig gesperrte DB) soll den eigentlichen
    // Chat-Versand nicht verhindern, deshalb best-effort statt `?`. Spec
    // 0040, Abschnitt 7: `None` (kein Verschlüsselungsschlüssel verfügbar,
    // s. `lib::build_app_state`) ist derselbe Fall — einfach überspringen.
    if let Some(store) = prompt_history_store {
        if let Err(err) = store.record(&session.server_id, &text).await {
            eprintln!("Prompt konnte nicht in der Historie gespeichert werden: {err}");
        }
    }

    if queued_only {
        return Ok(());
    }
    let mut texts = vec![text];

    loop {
        // Spec 0032: `profile_store.get_server` findet den lokalen
        // Pseudo-Server nie (keine `servers`-Zeile) — ohne diesen Zweig würde
        // der Servername in JEDER Chat-Nachricht auf das generische "Server"
        // degradieren (unabhängiger Review-Pass, s. docs/adr/0026).
        let (server_name, current_tags) = if crate::dto::is_local(session.server_id) {
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
        let (updated_system_context_parts, notes_present) = build_session_system_context(
            app,
            &server_name,
            &session.server_id,
            &current_tags,
            profile_store,
            policy_store,
        )
        .await;
        // Spec 0039, Abschnitt 5: der System-Prompt wird bei JEDER
        // Nutzer-Nachricht neu gebaut — enthält er gefencte Notizen (auch
        // wenn er das schon in einer früheren Nachricht tat), muss das Flag
        // spätestens jetzt gesetzt sein. Monoton: `store(true, ...)` nur bei
        // Bedarf, ein bereits gesetztes Flag wird nie zurückgesetzt.
        if notes_present {
            session
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        let updated_system_context = updated_system_context_parts.assemble();
        {
            let mut ctx = session.context.lock().await;
            ctx.system_context = updated_system_context;
        }
        *session.system_context_parts.lock().await = updated_system_context_parts;
        // Spec 0040, Abschnitt 2: über `push_history` statt eines direkten
        // `ctx.history.push(...)`, sonst umgeht die Nutzer-Nachricht die
        // Persistenz (Spec 0034, Abschnitt 4 verlangt ausdrücklich, dass auch
        // Nutzertext fortlaufend in `chat_messages` landet — verschlüsselt,
        // Spec 0036). Muss VOR `run_chat_turn` passieren, damit die Nachricht
        // in der DB steht, bevor der KI-Aufruf überhaupt startet.
        for text in texts {
            crate::orchestration::push_history(
                session,
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Text(text),
                },
            )
            .await;
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
            let mut turn = session.chat_turn.lock().unwrap();
            if turn.queued.is_empty() && !unanswered_injected_messages {
                turn.running = false;
                None
            } else {
                session
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
/// `crate::orchestration::propose_note_from_chat_content`-Doc-Kommentar).
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
    crate::orchestration::propose_note_from_chat_content(
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
    // `crate::orchestration::suggest_note_update_on_disconnect`) — zu
    // diesem Zeitpunkt ist die Session per Design bereits aus
    // `state.sessions` entfernt. Der frühere `state.sessions.get(session_id)`-
    // Check hätte diesen (gültigen) Aufruf fälschlich mit "Session nicht
    // gefunden" abgelehnt. `pending_action_confirmations.resolve()` prüft
    // die Gültigkeit von `action_id` bereits selbst (liefert einen eigenen
    // Fehler für eine unbekannte/bereits aufgelöste ID) — der zusätzliche
    // Session-Check war ohnehin redundant dazu, nicht die einzige
    // Absicherung.
    state
        .pending_action_confirmations
        .resolve(&action_id, decision)?;
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

#[tauri::command]
pub async fn disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, crate::elevated_sftp::ElevatedSftpRegistry>,
    session_id: SessionId,
) -> CommandResult<()> {
    // Spec 0084, A2.1: Sitzung UND erhöhter Kanal gehen in einer einzigen
    // Funktion weg — `SessionManager::remove` wird außer dort und in Tests
    // nirgends aufgerufen. Sie sperrt den Transport nicht; das Trennen
    // bleibt unten in diesem Befehl.
    let session = elevated
        .remove_session(&state.sessions, session_id)
        .ok_or("Session nicht gefunden")?;

    // Best-effort: ein Fehler beim Trennen selbst (z. B. Verbindung bereits
    // tot) soll `disconnect()` nicht scheitern lassen — die Session wird in
    // jedem Fall aus `state.sessions` entfernt.
    let _ = session.transport.lock().await.disconnect().await;
    // Droppt den Sender -> der Terminal-Aktor (falls einer läuft) beendet
    // sich selbst beim nächsten `commands.recv()` (s. dortiger Kommentar),
    // ohne hier ein zweites `connection-status-changed`-Event auszulösen.
    *session.terminal.lock().unwrap() = None;

    tracing::info!(session_id = %session_id, "session disconnected");
    emit_connection_status_changed(
        &TauriEventEmitter(app.clone()),
        session_id,
        ConnectionStatus::Disconnected,
        None,
    );

    // Spec 0054, Teil 4, Punkt 6: "Temp aufräumen (bei Session-Ende
    // spätestens)" — Fallback-Netz für einen "Lokal öffnen"-Flow, den der
    // Nutzer nie explizit über `close_edit_session` beendet hat (z. B.
    // Tab einfach geschlossen, während eine Datei noch offen war). Rein
    // best-effort: `edit_session_dir` existiert typischerweise gar nicht
    // (kein Datei-Editier-Flow in dieser Session genutzt), das ist kein
    // Fehler.
    if let Ok(dir) = edit_session_dir(session_id) {
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    // Spec 0034, Abschnitt 4: "endet bei `disconnect()`" — vor dem Spawn
    // unten, damit `ended_at` zuverlässig gesetzt ist, sobald `disconnect()`
    // selbst zurückkehrt, statt von der Fertigstellung des unabhängigen
    // Notiz-Vorschlag-Tasks abzuhängen. Best-effort wie der
    // Transport-Trennvorgang oben: ein Schreibfehler hier blockiert
    // `disconnect()` nicht.
    if let (Some(store), Some(chat_session_id)) = (
        &session.chat_session_store,
        *session.chat_session_id.lock().await,
    ) {
        if let Err(err) = store.mark_ended(chat_session_id).await {
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
    tokio::spawn(async move {
        let state = app_for_suggestion.state::<AppState>();
        // Spec 0034, Abschnitt 7: läuft vor dem Notiz-Vorschlag im selben
        // Hintergrund-Task (sequentiell, keine zweite parallele
        // `AiProvider::send()`-Anfrage auf demselben Provider) — beide
        // sind unabhängige, optionale "beim Trennen"-Extras, s. jeweilige
        // Doc-Kommentare zur genauen Auslösebedingung.
        crate::orchestration::generate_session_title_on_disconnect(
            &session,
            session_id,
            &TauriEventEmitter(app_for_suggestion.clone()),
        )
        .await;
        let note_update_suggested = crate::orchestration::suggest_note_update_on_disconnect(
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
        if crate::orchestration::should_suggest_note_shrink(note_update_suggested) {
            let server = resolve_server_for_note_shrink(
                &app_for_suggestion,
                state.profile_store.as_ref(),
                session.server_id,
            )
            .await;
            if let Some(server) = server {
                crate::orchestration::suggest_note_shrink_on_disconnect(
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
        result.push(SessionSummaryDto {
            session_id: entry.session_id,
            server_id: entry.server_id,
            server_name,
            status: entry.status,
            has_pending_action: entry.has_pending_action,
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
) -> CommandResult<Vec<crate::dto::ChatHistoryEntryDto>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    let history = session.context.lock().await.history.clone();
    Ok(history
        .into_iter()
        .map(crate::dto::ChatHistoryEntryDto::from)
        .collect())
}

#[cfg(test)]
mod send_chat_message_persistence_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use futures::StreamExt;
    use tokio::sync::Mutex as AsyncMutex;

    use chrono::Utc;

    use ssh_manager_core::ai::{
        default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, SessionContext,
    };
    use ssh_manager_core::filter::FilterEngine;
    use ssh_manager_core::profiles::Server;
    use ssh_manager_core::shared::ServerId;
    use ssh_manager_core::ssh::{
        CommandOutput, InteractiveShell, PtySize, SftpSession, SshError, SshTransport,
    };

    use crate::confirmation::ConfirmationRegistry;
    use crate::events::TestEmitter;
    use crate::first_run_notice::test_support::test_app;
    use crate::test_support::InMemoryProfileStore;

    use super::*;

    /// Nie tatsächlich aufgerufen — dieser Test führt kein Kommando aus,
    /// die KI schlägt keins vor (s. `NoopAiProvider`).
    struct UnusedTransport;
    #[async_trait]
    impl SshTransport for UnusedTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            unreachable!("dieser Test ruft SshTransport::execute nie auf")
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
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
            _context: SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
            Box::pin(futures::stream::iter(vec![AiEvent::Done]))
        }
    }

    fn test_session(server_id: ServerId) -> Session {
        Session {
            transport: AsyncMutex::new(Box::new(UnusedTransport)),
            ai_provider: Box::new(NoopAiProvider),
            ai_provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
            context: AsyncMutex::new(SessionContext {
                system_context: String::new(),
                history: Vec::new(),
                available_actions: default_action_schemas(),
                max_tokens_hint: None,
            }),
            filter_engine: Box::new(FilterEngine::new(crate::policy::NoRulesPolicyStore)),
            server_id,
            tags: Vec::new(),
            terminal: std::sync::Mutex::new(None),
            redactor: Box::new(DefaultOutputRedactor::new()),
            ai_provider_label: "test-provider".to_string(),
            ai_model: "test-model".to_string(),
            system_context_parts: AsyncMutex::new(crate::compaction::SystemContextParts::default()),
            model_context_window_tokens: usize::MAX / 1_000,
            summary: AsyncMutex::new(None),
            mcp_origin_flags: std::sync::Mutex::new(Vec::new()),
            sudo_password: None,
            status: std::sync::Mutex::new(crate::events::ConnectionStatus::Connected),
            pending_action: std::sync::Mutex::new(None),
            sftp: AsyncMutex::new(None::<Box<dyn SftpSession>>),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop_notify: tokio::sync::Notify::new(),
            chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
            risk_second_opinion_provider: None,
            risk_second_opinion_budget: None,
            running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            injection_check_provider: None,
            injection_check_budget: None,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            chat_session_store: None,
            ledger_store: None,
            chat_session_id: AsyncMutex::new(None),
            ai_request_paced_at: AsyncMutex::new(None),
        }
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
        let profile_store = persistence_sqlite::SqliteProfileStore::connect(&db_path)
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
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        let cipher: Arc<dyn ssh_manager_core::crypto::ContentCipher> = Arc::new(
            ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(&[21u8; 32]),
        );
        let chat_store = profile_store.chat_session_store(cipher);
        let chat_session_id = chat_store.create_session(&server_id, None).await.unwrap();

        let mut session = test_session(server_id);
        session.chat_session_store = Some(chat_store.clone());
        session.chat_session_id = AsyncMutex::new(Some(chat_session_id));

        (session, profile_store, chat_store, tmp_dir)
    }

    /// Der eigentliche Regressionstest: ruft `send_chat_message_impl`
    /// direkt auf (genau die Funktion, die vorher den Nutzertext nur in
    /// den In-Memory-Kontext schrieb) und prüft, dass die Nachricht
    /// tatsächlich in `chat_messages` landet — nicht nur in
    /// `session.context`.
    #[tokio::test]
    async fn test_send_chat_message_persists_user_text_via_push_history() {
        let (session, profile_store, chat_store, _tmp_dir) = session_with_real_persistence().await;
        let chat_session_id = session.chat_session_id.lock().await.unwrap();
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
            Some(&profile_store.prompt_history_store(Arc::new(
                ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(&[21u8; 32]),
            ))),
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

    /// Spec 0040, Abschnitt 7: ein gesperrter/verweigerter OS-Schlüsselbund
    /// beim App-Start lässt `AppState.prompt_history_store` `None` werden
    /// (s. `lib::build_app_state`) — `send_chat_message_impl` darf dadurch
    /// nicht scheitern, nur die Prompt-Historie bleibt für diesen App-Lauf
    /// leer. Regressionstest für genau diesen degradierten Zustand, nicht
    /// nur den Normalfall oben.
    #[tokio::test]
    async fn test_send_chat_message_without_prompt_history_store_still_succeeds() {
        let (session, profile_store, chat_store, _tmp_dir) = session_with_real_persistence().await;
        let chat_session_id = session.chat_session_id.lock().await.unwrap();
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
            "ohne Prompt-Historie".to_string(),
            None,
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
                MessageContent::Text(t) if t == "ohne Prompt-Historie"
            ) && m.role == Role::User),
            "die Chat-Persistenz selbst darf vom fehlenden Prompt-History-Store unbeeinflusst \
             bleiben: {loaded:?}"
        );
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
            context: SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
            let mut contexts = self.contexts.lock().unwrap();
            let first = contexts.is_empty();
            contexts.push(context);
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

    /// Spec 0066, §2: eine Nachricht, die während eines laufenden Turns
    /// gesendet wird, kehrt sofort zurück (Eingabe nie blockiert), wird
    /// eingereiht und nach Turn-Ende als normale Nachricht gesendet — über
    /// denselben Pfad, also auch mit Redaction vor dem Versand.
    #[tokio::test]
    async fn test_message_sent_during_running_turn_is_queued_then_sent_redacted() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = test_session(ServerId::new());
        session.ai_provider = Box::new(GatedRecordingProvider {
            gate: gate.clone(),
            contexts: contexts.clone(),
        });
        let dir = tempfile::tempdir().unwrap();
        let policy_store =
            persistence_sqlite::SqliteProfileStore::connect(&dir.path().join("t.db"))
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
        let turn = session.chat_turn.lock().unwrap();
        assert!(!turn.running && turn.queued.is_empty());
    }

    struct AllowEverythingPolicyStore;
    #[async_trait]
    impl ssh_manager_core::filter::PolicyStore for AllowEverythingPolicyStore {
        async fn rules_for(
            &self,
            _scope: &ssh_manager_core::filter::EffectiveScope,
        ) -> Vec<ssh_manager_core::filter::Rule> {
            vec![ssh_manager_core::filter::Rule {
                id: ssh_manager_core::filter::RuleId("allow-all".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Allow,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
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
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
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
            context: SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
            let mut contexts = self.contexts.lock().unwrap();
            let first = contexts.is_empty();
            contexts.push(context);
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
    async fn test_queued_message_injected_then_stopped_before_send_is_still_answered() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = test_session(ServerId::new());
        session.transport = AsyncMutex::new(Box::new(EchoTransport));
        session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
        session.ai_provider = Box::new(GatedActionProvider {
            gate: gate.clone(),
            contexts: contexts.clone(),
        });
        let dir = tempfile::tempdir().unwrap();
        let policy_store =
            persistence_sqlite::SqliteProfileStore::connect(&dir.path().join("t.db"))
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
        assert!(!session.chat_turn.lock().unwrap().running);
    }

    /// Spec 0066, spec-reviewer-Fund: ein Stopp, der vor dem eigentlichen
    /// Rundenbeginn eintrifft, darf nicht verschluckt werden — `run_chat_turn`
    /// setzt das Flag nicht mehr selbst zurück.
    #[tokio::test]
    async fn test_run_chat_turn_does_not_swallow_an_early_stop() {
        let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = test_session(ServerId::new());
        session.ai_provider = Box::new(GatedRecordingProvider {
            gate: Arc::new(tokio::sync::Notify::new()),
            contexts: contexts.clone(),
        });
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
        let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = test_session(ServerId::new());
        let gate = Arc::new(tokio::sync::Notify::new());
        session.ai_provider = Box::new(GatedRecordingProvider {
            gate: gate.clone(),
            contexts: contexts.clone(),
        });
        session.request_auto_continue_stop();
        let dir = tempfile::tempdir().unwrap();
        let policy_store =
            persistence_sqlite::SqliteProfileStore::connect(&dir.path().join("t.db"))
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
}
