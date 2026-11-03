//! Spec 0007/0017/0031/0034/0068/0069: Verbindungsaufbau (inkl. Host-Key-
//! Bestätigung), persistente Chat-Sitzungen — Teil der Spec-0083-Aufteilung
//! von `commands.rs`.

mod progress;
mod system_context;
mod transport;

pub(super) use system_context::build_session_system_context;

pub(crate) use progress::ConnectProgress;
use progress::{cancelled_error, or_cancelled};
use transport::establish_transport;

use super::scope::extension_scope_lookup;
use std::sync::Arc;

use tauri::{AppHandle, State};

use ssh_manager_core::ai::OutputRedactor;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{resolve_connection_target, ConnectLog, SshError};

use app_logic::ai_provider_factory::build_ai_provider_from_config_async;
use app_logic::connect::session_builder::{
    build_session, redaction_for_reconnect, BuiltSession, SessionBuildInput,
};
use app_logic::dto::HostKeyUserDecision;
use app_logic::error::{
    secret_store_error, ssh_command_error, CommandError, CommandResult, StoreResultExt,
};
use app_logic::events::{emit_connection_status_changed, ConnectionStatus};
use app_logic::state::{AppState, SessionId};
// Spec 0084, §4: die Konstante liegt in `app_logic::test_connection` (s.
// dortiger Kommentar) — `test_connection` ist Tauri-frei in `app-logic`,
// `commands::connect` bleibt Tauri-gebunden in `app-shell`; beide nutzen
// dieselben Grenzen (Issue #97).
use app_logic::test_connection::SSH_CONNECT_LIMITS;

use super::ai_providers::active_ai_provider_config;

/// Spec 0007, Abschnitt 4/6. `session_id` wird **vor** dem eigentlichen
/// Verbindungsaufbau vergeben (nicht erst bei Erfolg): Abschnitt 4 sieht
/// vor, dass während des Aufbaus ein `host-key-verification-needed`-Event
/// mit derselben `session_id` ausgelöst werden kann, auf das das Frontend
/// mit `confirm_host_key(session_id, ...)` reagiert, **bevor** dieser
/// Befehl selbst zurückkehrt — das Frontend kennt die `SessionId` an dieser
/// Stelle also nur aus dem Event, nicht aus dem (noch ausstehenden)
/// Rückgabewert von `connect()`.
///
/// Host-Key-Bestätigung: `ssh_transport::connect()` liefert bei
/// `Unknown`/`Mismatch` sofort `ConnectOutcome::PendingHostKeyConfirmation`
/// zurück, statt den Handshake anzuhalten (s.
/// `docs/adr/0007-connect-outcome-and-arc-host-keys.md` — `russh` kennt
/// keinen "Handshake pausieren und später fortsetzen"-Mechanismus). Das
/// "Blockieren bis `confirm_host_key`" aus der Aufgabenstellung wird
/// deshalb hier drumherum gebaut: ein `oneshot`-Kanal pro `session_id`
/// (`state.pending_host_key_confirmations`), auf den dieser Befehl wartet;
/// nach `Trust` wird `connect()` mit demselben `ConnectionTarget` erneut
/// aufgerufen (ein frischer Verbindungsversuch, keine buchstäbliche
/// Fortsetzung — ebenfalls in ADR 0007 begründet).
#[tauri::command]
pub async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    server_id: ServerId,
    attempt_id: String,
) -> CommandResult<SessionId> {
    let session_id: SessionId = uuid::Uuid::new_v4();
    // Issue #546: live `connect-progress` events and `cancel_connect` for
    // this attempt id.
    let progress = ConnectProgress::new(
        &state,
        attempt_id,
        Arc::new(crate::event_emitter::TauriEventEmitter(app.clone())),
    );
    connect_session_with_progress(
        &app,
        &state,
        server_id,
        session_id,
        None,
        true,
        Some(&progress),
    )
    .await
}

/// Issue #546: aborts the running `connect` with this attempt id (also while
/// it waits for the host key decision). It then returns
/// `SSH_CONNECT_CANCELLED` without retrying. An unknown or finished attempt
/// is a no-op.
#[tauri::command]
pub fn cancel_connect(state: State<'_, AppState>, attempt_id: String) {
    state.connect_attempts.cancel(&attempt_id);
}

#[cfg(test)]
use app_logic::connect::session_builder::should_create_chat_session;
#[cfg(test)]
use app_logic::session::Session;

/// Spec 0047, Fund D2: `SshError` trägt seit Spec 0024 einen stabilen
/// `code()` fürs Frontend-Mapping — der blanket `?` auf
/// `ssh_transport::connect(...)` (über `CommandError`s
/// `impl<E: Display> From<E>`) verwarf ihn bislang und lieferte
/// `code: None`. Ein Tester mit nicht erreichbarem Server sah dadurch nur
/// den rohen, hart-deutschen `Display`-Text inkl. OS-Fehlertext, auch im
/// englischen UI. Eigene, kleine Funktion statt eines Inline-`.map_err`
/// in `connect_session`, damit dieser eine Mapping-Schritt (anders als
/// `connect_session` als Ganzes) isoliert testbar ist.
/// Spec 0101, A9.1: Der Code kommt vollständig aus [`SshError::code`] —
/// der Zustand des Schlüsselbunds spielt für ihn keine Rolle mehr (seit A9
/// liegen die Secrets in der Datenbank, s.
/// `app_logic::error::ssh_command_error`).
fn map_connect_result(
    result: Result<ssh_transport::ConnectOutcome, SshError>,
) -> CommandResult<ssh_transport::ConnectOutcome> {
    result.map_err(|err| ssh_command_error(&err))
}

/// Kern von `connect()` (s. dessen Doc-Kommentar zur Host-Key-Logik),
/// herausgelöst aus dem `#[tauri::command]`-Wrapper, damit Spec 0028
/// (`crate::mcp_backend`) denselben Verbindungsaufbau nutzen kann, den auch
/// ein manueller Klick in der Sidebar auslöst — **keine vorherige manuelle
/// Verbindung nötig**, ein Aufruf über MCP an einen noch nie verbundenen
/// Server baut die Verbindung selbst auf (Spec 0028, Abschnitt 9a).
/// `session_id` kommt vom Aufrufer (statt hier neu generiert zu werden),
/// damit `crate::mcp_backend` sie bereits **vor** diesem Aufruf kennt und
/// dem Frontend darüber sofort einen Tab zuordnen kann — sonst würde ein
/// währenddessen auftretender Host-Key-Dialog (s. unten) an eine noch gar
/// nicht sichtbare Session hängen.
///
/// `resume` (Spec 0034, Abschnitt 8): `Some(chat_session_id)`, wenn dieser
/// Verbindungsaufbau eine bereits gespeicherte Sitzung fortsetzt
/// (`resume_chat_session`) statt eine neue anzulegen (`connect`/`connect_
/// session` mit `None`). Der SSH-Verbindungsaufbau selbst (inkl. möglicher
/// Host-Key-Bestätigung) läuft in beiden Fällen identisch — nur die
/// `chat_sessions`-Behandlung und die initiale `SessionContext.history`
/// unterscheiden sich, s. unten.
///
/// `persist_chat_session` (Spec 0040, Abschnitt 4, erster Fix-Punkt):
/// `false` für MCP-ausgelöste Verbindungsaufbauten
/// (`mcp_backend::AppMcpBackend::ensure_session`) — eine rein MCP-
/// ausgelöste Sitzung erzeugt dann gar keine `chat_sessions`-Zeile, statt
/// (wie vorher) eine inhaltsleere, aber existierende Zeile anzulegen, die
/// dennoch im "Sitzungen fortsetzen"-Screen aufgetaucht wäre. Bewusst
/// unabhängig von `resume` geprüft: `resume: Some(..)` lädt ohnehin nur
/// eine bereits bestehende Zeile (legt nie eine neue an), MCP ruft
/// `connect_session` aber nie mit `resume: Some(..)` auf (kein MCP-Tool
/// dafür) — die Fälle überschneiden sich also nicht.
pub(crate) async fn connect_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    server_id: ServerId,
    session_id: SessionId,
    resume: Option<uuid::Uuid>,
    persist_chat_session: bool,
) -> CommandResult<SessionId> {
    connect_session_with_progress(
        app,
        state,
        server_id,
        session_id,
        resume,
        persist_chat_session,
        None,
    )
    .await
}

pub(crate) async fn connect_session_with_progress<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    server_id: ServerId,
    session_id: SessionId,
    resume: Option<uuid::Uuid>,
    persist_chat_session: bool,
    progress: Option<&ConnectProgress>,
) -> CommandResult<SessionId> {
    // Spec 0031, Abschnitt 4, letzter Punkt: serverseitige Durchsetzung
    // zusätzlich zur Frontend-Sperre in `ServerList.tsx` — eine reine
    // Frontend-Sperre wäre umgehbar (z. B. ein direkter
    // `invoke("connect", ...)`-Aufruf ohne den UI-Umweg über
    // `ServerList`). Greift für **jeden** Aufrufer dieser Funktion
    // gleichermaßen, also auch für `crate::mcp_backend`s automatischen
    // Verbindungsaufbau (Spec 0028) — dieselbe "neue Vertrauensgrenze
    // verdient strengere Behandlung"-Logik wie dort.
    ensure_first_run_notice_acknowledged(app)?;

    // Issue #242: ab hier (vor Verbindungsaufbau und Laden der Historie) gilt
    // die Chat-Sitzung als in Benutzung, bis dieser Aufruf endet — nach dem
    // `sessions.insert` unten übernimmt die registrierte `Session` (s.
    // `is_chat_session_active`). Jeder frühe Rückgabepfad gibt den Anspruch
    // per `Drop` frei.
    let _resume_claim = match resume {
        Some(existing_id) => Some(
            state
                .sessions
                .claim_chat_session_for_resume(existing_id)
                .map_err(|_| {
                    CommandError::from(
                        "Diese Chat-Sitzung wird gerade gelöscht und kann nicht fortgesetzt \
                         werden.",
                    )
                })?,
        ),
        None => None,
    };

    let is_local = app_logic::dto::is_local(server_id);
    let server = if is_local {
        crate::local_server::synthetic_server(app)
    } else {
        state
            .profile_store
            .get_server(&server_id)
            .await
            .store_err()?
    };
    let active_config = active_ai_provider_config(state).await?;
    // Issue #162: Einstellung des Providers (Default an).
    let (ai_provider, ai_provider_budget) = build_ai_provider_from_config_async(
        &state.rate_limit_registry,
        state.credential_store.clone(),
        &active_config,
        active_config.web_research_enabled,
    )
    .await
    .map_err(secret_store_error)?;

    // Spec 0032, Abschnitt 2/3: der lokale Pseudo-Server hat keinen
    // Verbindungszustand, keinen Host-Key und keine Credentials — "Verbinden"
    // ist hier nur die Konstruktion eines `LocalTransport`, ohne die
    // `russh`/Host-Key-Schleife unten zu durchlaufen.
    let mut transport =
        establish_transport(app, state, server_id, session_id, &server, true, progress).await?;
    // Issue #881: the transport's own tasks run in the session scope.
    transport.set_task_spawner(crate::background_tasks::session_spawner(app, session_id));

    // The Tauri-bound inputs of the session assembly (issue #839): resolved
    // here, handed to `app_logic::connect::session_builder` as plain values.
    let scope_lookup = extension_scope_lookup(app, state.profile_store.as_ref(), server_id).await;
    let prompt_language = crate::ui_language::session_prompt_language(app);
    // Issue #102: "on but not set up" is reported to the session, "off" is silent.
    let (second_opinion, second_opinion_setup_failed) =
        crate::risk_second_opinion::resolve_second_opinion_provider(app, state)
            .await
            .into_parts();
    let red_risk_always_confirm = crate::risk_second_opinion::red_risk_always_confirm(app);
    let injection_check = if server.ai_injection_check_enabled {
        crate::risk_second_opinion::resolve_second_opinion_provider(app, state)
            .await
            .into_parts()
            .0
    } else {
        None
    };

    let built = build_session(
        state,
        transport,
        SessionBuildInput {
            server_id,
            session_id,
            server,
            resume,
            persist_chat_session,
            active_config,
            ai_provider,
            ai_provider_budget,
            scope_lookup,
            prompt_language,
            second_opinion,
            second_opinion_setup_failed,
            injection_check,
            red_risk_always_confirm,
        },
    )
    .await?;
    let BuiltSession {
        session,
        chat_session_id,
        loss_watch,
    } = built;
    let session = Arc::new(session);
    state.sessions.insert(session_id, Arc::clone(&session));
    crate::auto_reconnect::install_hook(app, &session);
    if let Some(watch) = loss_watch {
        app_logic::session::spawn_connection_watch(
            session_id,
            &session,
            watch,
            Arc::new(crate::event_emitter::TauriEventEmitter(app.clone())),
            &crate::background_tasks::session_scope(app, session_id),
        );
    }

    tracing::info!(session_id = %session_id, server_id = %server_id.0, "session connected");
    emit_connection_status_changed(
        &crate::event_emitter::TauriEventEmitter(app.clone()),
        session_id,
        ConnectionStatus::Connected,
        None,
    );
    // Issue #611: the frontend learns the chat session id of the new session.
    app_logic::events::emit_chat_session_changed(
        &crate::event_emitter::TauriEventEmitter(app.clone()),
        session_id,
        chat_session_id,
    );
    Ok(session_id)
}

// --- Spec 0034, Abschnitt 8: persistente Chat-Sitzungen ------------------

/// Spec 0034, Abschnitt 6/8: Liste vergangener Sitzungen für den
/// Auswahl-Screen beim Verbinden, neueste zuerst.
/// Issue #355, Spec 0105: reconnects a lost session in place. Tab, session
/// id, chat history and AI context stay; the transport is replaced by a
/// fresh connection that went through the same path as a first connect
/// (`establish_transport`: jump hosts, keychain, host key dialog).
#[tauri::command]
pub async fn reconnect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, crate::elevated_sftp::ElevatedSftpRegistry>,
    grants: State<'_, app_logic::local_path_grants::LocalPathGrants>,
    session_id: SessionId,
) -> CommandResult<()> {
    // Issue #356: a manual reconnect takes over from the automatic attempts.
    if let Some(session) = state.sessions.get(session_id) {
        session.conn.auto_reconnect.stop();
    }
    reconnect_session_inner(&app, &state, &elevated, &grants, session_id, true).await
}

/// `interactive = false` is the automatic attempt (Issue #356): it never
/// opens the host key dialog.
pub(crate) async fn reconnect_session_inner<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    elevated: &crate::elevated_sftp::ElevatedSftpRegistry,
    grants: &app_logic::local_path_grants::LocalPathGrants,
    session_id: SessionId,
    interactive: bool,
) -> CommandResult<()> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    let emitter = crate::event_emitter::TauriEventEmitter(app.clone());
    // Only one reconnect at a time, and only for a lost session: a second
    // trigger finds the state `Reconnecting` and does nothing.
    let Some(claim) = session.begin_reconnect() else {
        return Err(CommandError::with_code(
            "Die Sitzung ist nicht getrennt oder wird bereits neu verbunden.",
            "SESSION_NOT_RECONNECTABLE",
        ));
    };
    emit_connection_status_changed(&emitter, session_id, ConnectionStatus::Reconnecting, None);

    // From here every exit that is not a success reports `Disconnected`
    // again (the claim resets the session state when dropped).
    let result = async {
        // A confirmation waiting at the time of the drop is rejected, never
        // executed on the new connection (fail closed, as on close).
        session.discard_pending_confirmation_for_reconnect(&state.pending_action_confirmations);
        if let Some(action_id) = session.transport.running_action() {
            let _ = state.running_command_cancellations.resolve(&action_id, ());
        }
        session.retire_connection().await;
        // Elevated channel and local path grants belong to the old
        // connection and are not carried over.
        elevated.revoke_session(session_id);
        grants.remove_session(session_id);

        let server_id = session.server_id;
        let server = if app_logic::dto::is_local(server_id) {
            crate::local_server::synthetic_server(app)
        } else {
            state
                .profile_store
                .get_server(&server_id)
                .await
                .store_err()?
        };
        let mut transport = establish_transport(
            app,
            state,
            server_id,
            session_id,
            &server,
            interactive,
            None,
        )
        .await?;
        transport.set_task_spawner(crate::background_tasks::session_spawner(app, session_id));
        let lookup = extension_scope_lookup(app, state.profile_store.as_ref(), server_id).await;
        let (redactor, sudo_password) =
            redaction_for_reconnect(state, &session, server_id, &lookup).await;
        Ok::<_, CommandError>((transport, redactor, sudo_password))
    }
    .await;

    match result {
        Ok((transport, redactor, sudo_password)) => {
            let loss_watch = transport.connection_loss_watch();
            claim.succeed(transport, redactor, sudo_password).await;
            // Issue #565: the new connection gets its own loss watch; the
            // one of the replaced transport no longer reports.
            if let Some(watch) = loss_watch {
                app_logic::session::spawn_connection_watch(
                    session_id,
                    &session,
                    watch,
                    Arc::new(crate::event_emitter::TauriEventEmitter(app.clone())),
                    &crate::background_tasks::session_scope(app, session_id),
                );
            }
            tracing::info!(session_id = %session_id, "session reconnected");
            emit_connection_status_changed(&emitter, session_id, ConnectionStatus::Connected, None);
            let current = *session.persistence.chat_session_id.lock().await;
            app_logic::events::emit_chat_session_changed(&emitter, session_id, current);
            Ok(())
        }
        Err(err) => {
            drop(claim);
            tracing::warn!(session_id = %session_id, "reconnect failed");
            emit_connection_status_changed(
                &emitter,
                session_id,
                ConnectionStatus::Disconnected,
                None,
            );
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn list_chat_sessions(
    state: State<'_, AppState>,
    server_id: ServerId,
) -> CommandResult<Vec<app_logic::dto::ChatSessionSummaryDto>> {
    Ok(state
        .chat_session_store
        .list_sessions_for_server(&server_id)
        .await
        .store_err()?
        .into_iter()
        .map(app_logic::dto::ChatSessionSummaryDto::from)
        .collect())
}

/// Spec 0034, Abschnitt 8: "baut wie ein normaler `connect()`-Aufruf die
/// SSH-Verbindung auf (inkl. ggf. Host-Key-Bestätigung), lädt zusätzlich
/// die gespeicherte Historie in den `SessionContext`". Reiner dünner
/// Wrapper um `connect_session` mit `resume: Some(session_id)` — die
/// eigentliche Resume-Logik (Historie laden, `ended_at` zurücksetzen,
/// `untrusted_content_ingested` aus der Historie rekonstruieren, Kontext-
/// Kürzung) lebt dort, s. dortige Kommentare.
#[tauri::command]
pub async fn resume_chat_session(
    app: AppHandle,
    state: State<'_, AppState>,
    server_id: ServerId,
    session_id: uuid::Uuid,
    attempt_id: String,
) -> CommandResult<SessionId> {
    let tab_session_id: SessionId = uuid::Uuid::new_v4();
    // Issue #568: same live progress / cancel as `connect`.
    let progress = ConnectProgress::new(
        &state,
        attempt_id,
        Arc::new(crate::event_emitter::TauriEventEmitter(app.clone())),
    );
    resume_with_progress(
        &app,
        &state,
        server_id,
        tab_session_id,
        session_id,
        &progress,
    )
    .await
}

/// Issue #568: the body of `resume_chat_session`, separate so a test drives
/// exactly the path the command takes.
pub(crate) async fn resume_with_progress<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    server_id: ServerId,
    tab_session_id: SessionId,
    session_id: uuid::Uuid,
    progress: &ConnectProgress,
) -> CommandResult<SessionId> {
    connect_session_with_progress(
        app,
        state,
        server_id,
        tab_session_id,
        Some(session_id),
        true,
        Some(progress),
    )
    .await
}

/// Spec 0034, Abschnitt 8: `rename_chat_session` — manuelles Umbenennen,
/// überschreibt einen ggf. automatisch gesetzten Titel dauerhaft (anders
/// als die Auto-Titel-Generierung, s. `orchestration::generate_session_
/// title_on_disconnect`, die einen bereits vorhandenen Titel nie anfasst).
#[tauri::command]
pub async fn rename_chat_session(
    state: State<'_, AppState>,
    session_id: uuid::Uuid,
    new_title: String,
) -> CommandResult<()> {
    state
        .chat_session_store
        .rename_session(session_id, &new_title)
        .await
        .store_err()
}

/// Spec 0034, Abschnitt 8: `delete_chat_session` — zugehörige Nachrichten
/// verschwinden automatisch über `ON DELETE CASCADE`.
///
/// Spec 0040, Abschnitt 7: verweigert das Löschen, solange irgendein
/// gerade verbundener Tab diese Sitzung noch aktiv nutzt (s.
/// `SessionManager::is_chat_session_active`-Doc-Kommentar für die
/// Begründung) — klare Fehlermeldung statt stillschweigend eine
/// Fremdschlüssel-Lücke unter einer laufenden Sitzung aufzureißen.
#[tauri::command]
pub async fn delete_chat_session(
    state: State<'_, AppState>,
    session_id: uuid::Uuid,
) -> CommandResult<()> {
    // Issue #242: Prüfung und Löschen unter einem Anspruch — ein Resume, das
    // gerade läuft (noch nicht registriert), zählt ebenfalls als "in Benutzung".
    let _claim = match state
        .sessions
        .claim_chat_session_for_delete(session_id)
        .await
    {
        Ok(claim) => claim,
        Err(app_logic::session::ChatSessionClaimError::InUse) => {
            return Err(
                "Diese Chat-Sitzung ist gerade in einem offenen Tab aktiv — erst trennen, dann \
                 löschen."
                    .into(),
            );
        }
        Err(app_logic::session::ChatSessionClaimError::BeingDeleted) => {
            return Err("Diese Chat-Sitzung wird gerade gelöscht.".into());
        }
    };
    state
        .chat_session_store
        .delete_session(session_id)
        .await
        .store_err()
}

/// Spec 0031, Abschnitt 4: der eigentliche Türsteher vor
/// `connect_session` — als eigene, kleine, generische (über `R:
/// tauri::Runtime`, damit sie sich mit `tauri::test::MockRuntime` statt
/// nur der echten `Wry`-Runtime testen lässt) Funktion ausgelagert, statt
/// nur inline in `connect_session` zu leben: `connect_session` selbst
/// bräuchte für einen Test einen vollständigen `AppState` (echte
/// SQLite-Stores, Keyring, Host-Key-Datei) — dieser Türsteher-Schritt
/// passiert aber nachweislich, bevor irgendetwas davon angefasst wird,
/// und lässt sich isoliert prüfen.
fn ensure_first_run_notice_acknowledged<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> CommandResult<()> {
    if crate::first_run_notice::is_acknowledged(app) {
        Ok(())
    } else {
        Err(CommandError::with_code(
            "Erststart-Hinweis muss zuerst bestätigt werden",
            "FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED",
        ))
    }
}

#[cfg(test)]
mod tests_session_gate;

#[cfg(test)]
mod tests_chat_session;

#[cfg(test)]
mod tests_map_result;

#[cfg(test)]
mod tests_flow;
#[tauri::command]
pub async fn confirm_host_key(
    state: State<'_, AppState>,
    session_id: SessionId,
    decision: HostKeyUserDecision,
) -> CommandResult<()> {
    state
        .pending_host_key_confirmations
        .resolve(&session_id, decision)?;
    Ok(())
}
