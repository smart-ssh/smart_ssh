//! Spec 0007/0017/0031/0034/0068/0069: Verbindungsaufbau (inkl. Host-Key-
//! Bestätigung), persistente Chat-Sitzungen — Teil der Spec-0083-Aufteilung
//! von `commands.rs`.

use std::sync::Arc;

use tauri::{AppHandle, State};

use ssh_manager_core::ai::{ChatMessage, OutputRedactor, SessionContext};
use ssh_manager_core::filter::{EffectiveScope, EvalContext, FilterEngine, PolicyStore};
use ssh_manager_core::profiles::effective_notes_sections;
use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{resolve_connection_target, ConnectLog, HostKeyDecision, SshError};

use app_logic::ai_provider_factory::build_ai_provider_from_config;
use app_logic::confirmation::{ConfirmationRegistry, RegistrationGeneration};
use app_logic::dto::HostKeyUserDecision;
use app_logic::error::{secret_store_error, ssh_command_error, CommandError, CommandResult};
use app_logic::events::{
    emit_connect_progress, emit_connection_status_changed, emit_host_key_verification_ended,
    emit_host_key_verification_needed, ConnectionStatus, EventEmitter, HostKeyKind,
    HostKeyPromptEndReason,
};
use app_logic::session::{history_contains_untrusted_content, Session, SessionParts};
use app_logic::state::{AppState, SessionId};
// Spec 0084, §4: die Konstante liegt in `app_logic::test_connection` (s.
// dortiger Kommentar) — `test_connection` ist Tauri-frei in `app-logic`,
// `commands::connect` bleibt Tauri-gebunden in `app-shell`; beide nutzen
// dieselben Grenzen (Issue #97).
use app_logic::test_connection::SSH_CONNECT_LIMITS;

use super::ai_providers::active_ai_provider_config;
use super::diagnostics_export::build_os_banner_message;

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

/// Issue #546: what a running `connect` needs to report its progress and to
/// notice a cancel.
pub(crate) struct ConnectProgress {
    attempt_id: String,
    emitter: Arc<dyn EventEmitter>,
    guard: app_logic::connect_attempts::ConnectAttemptGuard,
}

impl ConnectProgress {
    pub(crate) fn new(
        state: &AppState,
        attempt_id: String,
        emitter: Arc<dyn EventEmitter>,
    ) -> Self {
        let guard = state.connect_attempts.register(&attempt_id);
        Self {
            attempt_id,
            emitter,
            guard,
        }
    }

    fn emit(
        &self,
        server_id: ServerId,
        attempt: u32,
        steps: Vec<ssh_manager_core::ssh::ConnectStepSnapshot>,
    ) {
        emit_connect_progress(
            self.emitter.as_ref(),
            &self.attempt_id,
            server_id,
            attempt,
            steps,
        );
    }

    /// A step log that reports every change as a `connect-progress` event.
    fn log(&self, server_id: ServerId, attempt: u32) -> ConnectLog {
        let emitter = self.emitter.clone();
        let attempt_id = self.attempt_id.clone();
        ConnectLog::with_sink(Arc::new(move |steps| {
            emit_connect_progress(emitter.as_ref(), &attempt_id, server_id, attempt, steps);
        }))
    }
}

fn cancelled_error() -> CommandError {
    CommandError::with_code("Verbindungsaufbau abgebrochen", "SSH_CONNECT_CANCELLED")
}

/// Runs `fut` unless the attempt is cancelled first (`None`). Without a
/// progress handle there is nothing to cancel. `fut` is dropped on cancel,
/// which releases its sockets and tunnel channels.
async fn or_cancelled<F: std::future::Future>(
    progress: Option<&ConnectProgress>,
    fut: F,
) -> Option<F::Output> {
    match progress {
        Some(p) => tokio::select! {
            out = fut => Some(out),
            () = p.guard.cancelled() => None,
        },
        None => Some(fut.await),
    }
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
///
/// Als reine, isolierte Funktion herausgezogen (statt der Bedingung inline
/// im `if`), damit die Entscheidung — die Spec-0040-Abschnitt-4-
/// Anforderung "MCP-ausgelöste Aktionen erzeugen keine `chat_sessions`-
/// Zeile" — direkt geprüft werden kann. Seit Issue #259 läuft zusätzlich
/// `connect_session` selbst gegen einen eingespeisten `Connector` (s.
/// `AppState::connector`).
fn should_create_chat_session(is_local: bool, persist_chat_session: bool) -> bool {
    !is_local && persist_chat_session
}

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

/// Opens the SSH connection of a session, including jump hosts, credential
/// resolution and the host key dialog (ADR 0007). Shared by the first connect
/// and by `reconnect_session` (Issue #355), so a reconnect is verified exactly
/// like a first connect: no cached "accepted" decision, a changed key shows
/// the dialog again.
async fn establish_transport<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    server_id: ServerId,
    session_id: SessionId,
    server: &ssh_manager_core::profiles::Server,
    interactive: bool,
    progress: Option<&ConnectProgress>,
) -> CommandResult<Box<dyn ssh_manager_core::ssh::SshTransport>> {
    let is_local = app_logic::dto::is_local(server_id);
    Ok(if is_local {
        Box::new(ssh_transport::LocalTransport::new())
    } else {
        let target = match resolve_connection_target(server, state.profile_store.as_ref()).await {
            Ok(target) => target,
            Err(err) => {
                // Fund (2026-09): ein fehlgeschlagener Verbindungs-
                // aufbau (hier: schon das Auflösen der Jump-Host-Kette,
                // unten der eigentliche `ssh_transport::connect`) landete
                // bislang NIRGENDS im Log — das `?` reichte den Fehler nur
                // ans Frontend durch, ohne dass je ein `tracing::`-Aufruf
                // dazwischenlag (anders als der erfolgreiche Fall, s.
                // "session connected" unten). Kein Zugangsdaten-Leck: nur
                // `code()`/die technische Fehlermeldung, keine der drei
                // `SshError`-Varianten mit `String`-Payload
                // (`ConnectionFailed`/`ChannelError`/
                // `CredentialResolutionFailed`) baut ihren Text aus einem
                // Passwort/Schlüssel — derselbe Text geht ohnehin schon
                // unverändert ans Frontend (`CommandError::with_code`
                // unten), hier also keine neue Offenlegung.
                // Spec 0094, A1.7: Der obige Kommentar begründete, warum das
                // `Display` eines `SshError` hier vertretbar sei — keine der
                // `String`-Varianten baue ihren Text aus einem Passwort. Das
                // stimmt für die Varianten selbst, sagt aber nichts über den
                // freien Text der darunter liegenden Bibliothek, der in
                // `ConnectionFailed`/`ChannelError` landet. A1.7 schneidet
                // diese Abwägung ab: ab `warn` nur noch `code()`. Ans
                // Frontend geht der volle Text unverändert weiter — die
                // Transparenz gegenüber dem Nutzer bleibt, nur die Datei
                // nicht mehr die Senke.
                tracing::warn!(
                    session_id = %session_id,
                    server_id = %server_id.0,
                    code = err.code(),
                    "resolving the connection target (jump host chain) failed",
                );
                tracing::debug!(
                    session_id = %session_id,
                    server_id = %server_id.0,
                    code = err.code(),
                    error = %ssh_manager_core::ai::default_log_redactor()
                        .redact_text(&err.to_string()),
                    "resolving the connection target (jump host chain) failed (error text)",
                );
                // Spec 0098 A4 / Spec 0101 A9.1: derselbe Weg wie unten.
                // `resolve_connection_target` liest heute keine Credentials,
                // dieser Zweig kann also gar keinen Secret-Speicher-Fehler
                // tragen — die einheitliche Abbildung kostet nichts und hält
                // den Weg geschlossen, falls sich das einmal ändert.
                return Err(ssh_command_error(&err));
            }
        };
        let mut attempt_no: u32 = 0;
        loop {
            attempt_no += 1;
            // Issue #51: je Versuch ein frisches Schritt-Protokoll. Es geht
            // nur ans Frontend (angehängt an den `CommandError`), nie ins
            // Log — die `tracing`-Aufrufe unten nennen weiter nur Code und
            // Meldung. Issue #546: mit `progress` zusätzlich live als
            // `connect-progress`-Ereignis.
            let attempt_log = match progress {
                Some(p) => p.log(server_id, attempt_no),
                None => ConnectLog::new(),
            };
            // Spec 0069, Teil A3 (Issue #97): jeder Verbindungsversuch
            // (auch nach `Trust` erneut, s. Schleife) läuft unter den
            // Phasengrenzen `SSH_CONNECT_LIMITS` — je Hop eine für
            // Verbindung/Handshake, eine für die Anmeldung. Außen herum
            // liegt nur ein Sicherheitsnetz, das mit der Hop-Zahl wächst
            // und im normalen Ablauf nicht erreichbar ist. Beides
            // umschließt bewusst NUR diesen Aufruf, nicht das Warten auf
            // eine Host-Key-Entscheidung weiter unten.
            let attempt = or_cancelled(
                progress,
                ssh_transport::connect_with_timeout(
                    // Issue #259: über den `Connector` im `AppState` —
                    // Produktion verdrahtet `RealConnector` (derselbe Aufruf
                    // `connect_with_limits` wie zuvor), Tests einen Mock.
                    state.connector.connect(
                        &target,
                        state.credential_store.as_ref(),
                        // Spec 0076, §4.2: derselbe Leser wie auf jedem
                        // Verbindungspfad.
                        state.key_file_reader.as_ref(),
                        state.host_key_store.clone(),
                        &attempt_log,
                        SSH_CONNECT_LIMITS,
                    ),
                    SSH_CONNECT_LIMITS.overall(target.hops.len()),
                ),
            )
            .await;
            let Some(attempt) = attempt else {
                // Issue #546: cancelled mid-attempt; the dropped future
                // released sockets and tunnel channels.
                attempt_log.fail_running("SSH_CONNECT_CANCELLED");
                return Err(cancelled_error().with_connect_log(attempt_log.snapshot()));
            };
            // Greift das äußere Sicherheitsnetz, bricht es den Versuch
            // mitten im Schritt ab; den schließt hier niemand sonst.
            if let Err(err) = &attempt {
                attempt_log.fail_running(err.code());
            }
            let outcome = match map_connect_result(attempt) {
                Ok(outcome) => outcome,
                Err(err) => {
                    let last_hop = target.hops.last();
                    // Spec 0094, A1.7: `err.message` stammt aus
                    // `SshError::to_string()` (s. `map_connect_result`) und
                    // fällt damit unter dieselbe Regel wie ein direkt
                    // geloggtes `SshError` — ab `warn` nur noch der Code.
                    tracing::warn!(
                        session_id = %session_id,
                        server_id = %server_id.0,
                        host = last_hop.map(|h| h.host.as_str()).unwrap_or("?"),
                        port = last_hop.map(|h| h.port),
                        hop_count = target.hops.len(),
                        code = err.code.unwrap_or("UNKNOWN"),
                        "connection attempt failed",
                    );
                    tracing::debug!(
                        session_id = %session_id,
                        server_id = %server_id.0,
                        code = err.code.unwrap_or("UNKNOWN"),
                        error = %ssh_manager_core::ai::default_log_redactor()
                            .redact_text(&err.message),
                        "connection attempt failed (error text)",
                    );
                    return Err(err.with_connect_log(attempt_log.snapshot()));
                }
            };

            match outcome {
                ssh_transport::ConnectOutcome::Connected(transport) => break transport,
                ssh_transport::ConnectOutcome::PendingHostKeyConfirmation {
                    host,
                    port,
                    raw_key,
                    decision,
                } => {
                    if !interactive {
                        // Issue #356, Spec 0107 §8: an automatic attempt
                        // never opens the host key dialog and never trusts
                        // a key; the user decides with the Reconnect button.
                        tracing::info!(
                            session_id = %session_id,
                            host = %host,
                            port,
                            "automatic reconnect stops: host key needs confirmation",
                        );
                        return Err(CommandError::with_code(
                            format!(
                                "Verbindung zu {host}:{port} nicht möglich (Host-Key nicht vertraut)"
                            ),
                            "SSH_HOST_KEY_NOT_TRUSTED",
                        )
                        .with_connect_log(attempt_log.snapshot()));
                    }
                    let key_type = app_logic::host_key_store::offered_key_type(&raw_key);
                    let (kind, fingerprint, expected_fingerprint) = match decision {
                        HostKeyDecision::Unknown { fingerprint } => {
                            (HostKeyKind::Unknown, fingerprint, None)
                        }
                        HostKeyDecision::Mismatch {
                            expected_fingerprint,
                            actual_fingerprint,
                        } => (
                            HostKeyKind::Mismatch,
                            actual_fingerprint,
                            Some(expected_fingerprint),
                        ),
                        HostKeyDecision::Trusted => {
                            unreachable!(
                                "PendingHostKeyConfirmation wird nur für Unknown/Mismatch gebaut"
                            )
                        }
                    };

                    tracing::info!(
                        session_id = %session_id,
                        host = %host,
                        port,
                        kind = ?kind,
                        "host key verification needed",
                    );

                    let (generation, rx) = state
                        .pending_host_key_confirmations
                        .register_tracked(session_id);
                    // Spec 0017, Abschnitt 2: solange `connect()` hier auf die
                    // Nutzerentscheidung wartet, existiert `session_id` noch in
                    // keiner `Session` (die wird erst unten nach erfolgreichem
                    // Aufbau eingefügt) — ohne diesen Eintrag würde ein
                    // Frontend-Reload während eines offenen Host-Key-Dialogs den
                    // zugehörigen Tab in der wiederhergestellten Tab-Leiste
                    // verlieren.
                    state
                        .sessions
                        .register_pending_connection(session_id, server_id);
                    // Issue #546: the host key step waits for the decision.
                    if let Some(p) = progress {
                        p.emit(
                            server_id,
                            attempt_no,
                            ssh_manager_core::ssh::ConnectStepSnapshot::from_records(
                                &attempt_log.snapshot(),
                                true,
                            ),
                        );
                    }
                    emit_host_key_verification_needed(
                        &crate::event_emitter::TauriEventEmitter(app.clone()),
                        session_id,
                        generation.as_u64(),
                        host.clone(),
                        port,
                        kind,
                        fingerprint,
                        expected_fingerprint,
                        key_type,
                    );

                    let wait = or_cancelled(
                        progress,
                        wait_for_host_key_decision(
                            &state.pending_host_key_confirmations,
                            session_id,
                            generation,
                            rx,
                            app_logic::orchestration::PENDING_ACTION_CONFIRM_TIMEOUT,
                        ),
                    )
                    .await;
                    state.sessions.clear_pending_connection(session_id);
                    let Some(wait) = wait else {
                        // Issue #546: cancelled while the dialog is open.
                        // Release the registration and tell the frontend the
                        // prompt is over; the key is not trusted.
                        state
                            .pending_host_key_confirmations
                            .cancel_if_current(&session_id, generation);
                        let _ = finish_host_key_wait(
                            &crate::event_emitter::TauriEventEmitter(app.clone()),
                            session_id,
                            generation,
                            HostKeyWait::Abandoned,
                            &host,
                            port,
                        );
                        attempt_log.fail_running("SSH_CONNECT_CANCELLED");
                        if let Some(p) = progress {
                            p.emit(server_id, attempt_no, attempt_log.progress_snapshot());
                        }
                        return Err(cancelled_error().with_connect_log(attempt_log.snapshot()));
                    };
                    // Issue #37: jeder Ausgang des Wartens meldet dem
                    // Frontend das Ende genau dieser Abfrage (`generation`),
                    // damit ein offener Dialog nach Timeout/Abbruch von
                    // selbst schließt. Timeout/Abbruch bleiben Ablehnung.
                    let user_decision = finish_host_key_wait(
                        &crate::event_emitter::TauriEventEmitter(app.clone()),
                        session_id,
                        generation,
                        wait,
                        &host,
                        port,
                    )
                    .map_err(|err| err.with_connect_log(attempt_log.snapshot()));
                    // Issue #546: the dialog is over — a final, plain
                    // snapshot (the host key step shows its real result)
                    // unless the attempt is retried after Trust.
                    let user_decision = match user_decision {
                        Ok(d) => d,
                        Err(err) => {
                            if let Some(p) = progress {
                                p.emit(server_id, attempt_no, attempt_log.progress_snapshot());
                            }
                            return Err(err);
                        }
                    };
                    match user_decision {
                        HostKeyUserDecision::Trust => {
                            tracing::info!(session_id = %session_id, host = %host, port, "host key trusted");
                            state.host_key_store.trust(&host, port, &raw_key)?;
                            // Erneuter Versuch mit demselben `target` — s.
                            // Doc-Kommentar oben.
                        }
                        HostKeyUserDecision::Reject => {
                            tracing::warn!(
                                session_id = %session_id,
                                host = %host,
                                port,
                                "host key rejected, connection aborted",
                            );
                            if let Some(p) = progress {
                                p.emit(server_id, attempt_no, attempt_log.progress_snapshot());
                            }
                            // Spec 0069, Teil A4: Code zusätzlich zur
                            // bisherigen, host:port-tragenden Meldung
                            // (Text unverändert).
                            return Err(CommandError::with_code(
                                format!(
                                    "Verbindung zu {host}:{port} abgelehnt (Host-Key nicht vertraut)"
                                ),
                                "SSH_HOST_KEY_NOT_TRUSTED",
                            )
                            .with_connect_log(attempt_log.snapshot()));
                        }
                    }
                }
            }
        }
    })
}

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
        state.profile_store.get_server(&server_id).await?
    };
    let active_config = active_ai_provider_config(state).await?;
    // Issue #162: Einstellung des Providers (Default an).
    let (ai_provider, ai_provider_budget) = build_ai_provider_from_config(
        &state.rate_limit_registry,
        state.credential_store.as_ref(),
        &active_config,
        active_config.web_research_enabled,
    )
    .map_err(secret_store_error)?;

    // Spec 0032, Abschnitt 2/3: der lokale Pseudo-Server hat keinen
    // Verbindungszustand, keinen Host-Key und keine Credentials — "Verbinden"
    // ist hier nur die Konstruktion eines `LocalTransport`, ohne die
    // `russh`/Host-Key-Schleife unten zu durchlaufen.
    let mut transport =
        establish_transport(app, state, server_id, session_id, &server, true, progress).await?;

    let sanitized_os = if let Ok(uname_output) = transport.execute("uname -a").await {
        let uname_text = String::from_utf8_lossy(&uname_output.stdout);
        app_logic::orchestration::sanitize_uname_output(&uname_text)
    } else {
        None
    };

    // Issue #544: root probe for the file browser's elevation controls. Real
    // SSH sessions only; any failure means "unknown" (never fails connect).
    let is_root = if is_local {
        None
    } else {
        match transport.execute("id -u").await {
            Ok(out) => {
                app_logic::orchestration::parse_id_u_output(&String::from_utf8_lossy(&out.stdout))
            }
            Err(err) => {
                tracing::debug!(error = %err, "id -u probe failed");
                None
            }
        }
    };

    // Spec 0018, Abschnitt 6: einmalig bei `connect()` gelesen, wie
    // `ai_provider_label`/`ai_model` — ein fehlender Eintrag (kein Sudo-
    // Passwort hinterlegt) wird zu `None`, kein harter Verbindungsfehler.
    // Issue #36: über den gemeinsamen Lesepfad, der einen Lesefehler des
    // Schlüsselbunds (anders als `NotFound`) als `warn` protokolliert.
    let sudo_password = app_logic::server_redaction::read_sudo_password_for_redaction(
        state.credential_store.as_ref(),
        server_id,
    );

    // Unabhängiger Review-Pass (Spec 0018): `sudo -S` liest die per Stdin
    // eingespeiste Passwortzeile nur, wenn `sudo` tatsächlich einen Prompt
    // zeigt — bei einem `NOPASSWD`-Sudoers-Eintrag oder einem noch
    // gültigen Sudo-Timestamp liest `sudo` nie von Stdin, wodurch die
    // ganze Zeile stattdessen an das AUSGEFÜHRTE Programm durchgereicht
    // wird (`sudo tee datei` schreibt das Passwort in die Datei, `sudo
    // cat`/`sudo bash`/... geben es auf stdout/stderr aus). Ohne diesen
    // Zweig kannte der Redactor das Sitzungs-Passwort überhaupt nicht —
    // es hätte in genau diesem Fall unredigiert den KI-Kontext und das
    // strukturierte Log erreicht. `regex::escape` neutralisiert
    // Regex-Sonderzeichen im Passwort selbst.
    // Spec 0038, section 2a: known secrets of a registered session extension
    // are masked in addition to the sudo password.
    let extension_scope = extension_scope(app, state.profile_store.as_ref(), server_id).await;
    let extension_secrets = app_logic::session_extension::known_secrets(
        state.session_extension.as_deref(),
        extension_scope.as_ref(),
    );
    let redactor: Box<dyn OutputRedactor> = app_logic::session_extension::redactor_with_secrets(
        sudo_password.as_ref(),
        &extension_secrets,
    );

    let (mut system_context_parts, notes_present) = build_session_system_context(
        app,
        &server.name,
        &server_id,
        &server.tags,
        state.profile_store.as_ref(),
        &state.policy_store,
    )
    .await;
    let extension_present = app_logic::session_extension::append_extension_section(
        &mut system_context_parts,
        state.session_extension.as_deref(),
        extension_scope.as_ref(),
        redactor.as_ref(),
    )
    .await;
    let notes_present = notes_present || extension_present;
    let system_context = system_context_parts.assemble();
    // Issue #323: die `suggest_command`-Beschreibung folgt derselben Sprache
    // wie der System-Prompt.
    let available_actions =
        app_logic::system_prompt::session_action_schemas(system_context_parts.language);

    // Spec 0064 (Prompt-Caching): der `uname`-Banner geht als eigene,
    // gefencte Verlaufs-Nachricht rein, nicht mehr in den System-Prompt
    // (s. `SystemContextParts`-Doc-Kommentar zur Begründung). Nur gebaut,
    // NICHT direkt persistiert und erst weiter unten (nach der Resume-/
    // Frisch-Weiche) VOR `initial_history` eingefügt — dasselbe "bei jedem
    // `connect()` frisch"-Muster wie der System-Prompt selbst, damit auch
    // eine wiederaufgenommene Sitzung (deren persistierte Historie den
    // Banner naturgemäß nie enthielt, da er früher Teil des System-Prompts
    // war) ihn bekommt. Spec-reviewer-Fund (Follow-up-Review): "nie in die
    // DB" gilt nicht absolut — faltet die Kompaktierung (Spec 0057) diese
    // erste Runde später in eine rollierende Zusammenfassung, landet der
    // (zeichen-whitelistete, s. `sanitize_uname_output`) Bannertext
    // indirekt doch in der persistierten Summary. Geringe Tragweite (kein
    // Geheimnis), aber hier ehrlich benannt statt der zu absoluten
    // Formulierung von vorher.
    let os_banner_message: Option<ChatMessage> =
        sanitized_os.as_deref().map(build_os_banner_message);

    // Spec 0057, §3.1: einmalig bei `connect()` aufgelöst, wie
    // `ai_provider_label`/`ai_model` unten — s. `Session::
    // model_context_window_tokens`-Doc-Kommentar.
    let model_context_window_tokens = app_logic::compaction::model_context_window_tokens(
        active_config.provider_type,
        &active_config.model,
    );

    // Spec 0026, Abschnitt 3: einmalig bei `connect()` aufgelöst, s.
    // `Session::risk_second_opinion_provider`-Doc-Kommentar. Spec 0061:
    // liefert zusätzlich den (bei gleicher Provider-Identität mit
    // `injection_check_provider` unten geteilten) Budget-Wächter mit.
    // Issue #102: "eingeschaltet, aber nicht einrichtbar" wird der Sitzung
    // gemeldet (`second_opinion_setup_notice_pending`), "aus" bleibt still.
    let (second_opinion_resolved, second_opinion_setup_failed) =
        crate::risk_second_opinion::resolve_second_opinion_provider(app, state)
            .await
            .into_parts();
    let (risk_second_opinion_provider, risk_second_opinion_budget) = match second_opinion_resolved {
        Some((provider, budget)) => (Some(provider), Some(budget)),
        None => (None, None),
    };

    // Spec 0092, A1.3: einmalig gelesen, wie die Zweitmeinung oben — eine
    // Änderung greift erst bei der nächsten Verbindung. Fail-safe „an",
    // auch bei fehlendem Schlüssel oder fehlerhaftem Wert (A1.2).
    let red_risk_always_confirm = crate::risk_second_opinion::red_risk_always_confirm(app);

    // Spec 0039, Abschnitt 5.1: einmalig übernommen, wie `risk_second_
    // opinion_provider` oben.
    let post_ingest_policy = server.post_ingest_policy;
    // Spec 0102: Startverzeichnis für Terminal und Dateibrowser dieser
    // Sitzung. Der lokale Pseudo-Server bietet das Feld nicht an — sein
    // synthetisches Profil trägt immer `None`.
    let start_directory = server.start_directory.clone();

    // Spec 0039, Abschnitt 5.2: nur `Some`, wenn BEIDE Bedingungen
    // erfüllt sind — die serverspezifische Einstellung UND die app-weite
    // Zweitmeinungs-Konfiguration (Spec 0026, Abschnitt 3), sonst wäre die
    // Checkbox im Frontend wirkungslos, obwohl sie aktiviert wurde.
    // Issue #231: eingeschaltet, aber ohne Anbieter → Sitzungs-Hinweis
    // (`injection_check_inactive_notice_pending`), kein Zwang zur
    // Bestätigung. Ist die Prüfung am Server aus, bleibt es still.
    let (injection_check_provider, injection_check_budget, injection_check_inactive) =
        if server.ai_injection_check_enabled {
            match crate::risk_second_opinion::resolve_second_opinion_provider(app, state)
                .await
                .into_parts()
                .0
            {
                Some((provider, budget)) => (Some(provider), Some(budget), false),
                None => (None, None, true),
            }
        } else {
            (None, None, false)
        };

    // Spec 0034, Abschnitt 2: `chat_sessions.server_id` referenziert
    // `servers(id)` — der lokale Pseudo-Server hat (Spec 0032) bewusst
    // KEINE eigene `servers`-Zeile, ein `INSERT` würde die Fremdschlüssel-
    // Einschränkung verletzen. Genau die in `docs/architecture-overview`
    // beschriebene Grenze: die Sonderbehandlung gehört hierher (Session-
    // Konstruktion), nicht in Kernschleife/Filter-Engine — dort läuft der
    // lokale Pseudo-Server unverändert wie jeder echte Server.
    //
    // `resume`: die gespeicherte Historie MUSS ladbar sein (Spec 0034,
    // Abschnitt 5, Punkt 1: "Integritätsprüfung, keine korrupten Daten")
    // — anders als bei einer frischen Sitzung (unten) ist ein
    // Ladefehler hier ein harter `resume_chat_session`-Fehler, kein
    // Best-effort-Fallback auf eine leere Historie (das würde dem Nutzer
    // eine augenscheinlich "leere" Sitzung zeigen, obwohl tatsächlich
    // Verlauf existiert, aber nicht lesbar war).
    let (mut initial_history, chat_session_id, initial_summary) = if let Some(existing_id) = resume
    {
        let store = &state.chat_session_store;
        // Unabhängiger Review-Pass (Spec 0040, Abschnitt 7): die
        // SSH-Verbindung ist an dieser Stelle bereits aufgebaut (s. oben)
        // — ein `?` hier würde sie beim frühen Rückkehren nur fallen
        // lassen (impliziter `Drop`, kein `SshTransport::disconnect()`),
        // statt sie sauber zu schließen. Beide Fehlerzweige unten trennen
        // deshalb explizit, bevor sie den Fehler weiterreichen.
        let loaded = match store.load_session(existing_id).await {
            Ok(loaded) => loaded,
            Err(err) => {
                transport.disconnect().await.ok();
                return Err(err.into());
            }
        };
        if let Err(err) = store.mark_resumed(existing_id).await {
            transport.disconnect().await.ok();
            return Err(err.into());
        }
        // Spec 0057, §2.3: die zuletzt gespeicherte rollierende
        // Zusammenfassung wieder mitladen, sonst müsste jede
        // wiederaufgenommene Sitzung bei der ersten Kompaktierung wieder
        // bei `rounds_covered = 0` anfangen (Spec 0057, §2.1: "nicht jedes
        // Mal die ganze History von vorne"). Best-effort wie
        // `load_session`s Geschwister-Aufrufe hier — ein Ladefehler ist
        // kein harter Fehler wie bei der Historie selbst: ohne Summary
        // fällt die nächste Kompaktierung einfach auf einen frischen
        // Zusammenfassungs-Versuch (oder den Etappe-2-Platzhalter) zurück.
        let initial_summary = match store.load_summary(existing_id).await {
            Ok(Some((text, rounds_covered))) => Some(app_logic::compaction::RollingSummary {
                text,
                // spec-reviewer-Fund (Review dieses Schritts): auf die
                // tatsächlich geladene Rundenzahl geklemmt — ohne das
                // könnte eine gespeicherte `rounds_covered`, die (etwa
                // durch nicht persistierte MCP-Aktionen, Spec 0034 §10,
                // oder einen best-effort fehlgeschlagenen `append_message`)
                // nicht mehr zur tatsächlich geladenen Historie passt, mehr
                // Runden als "bereits abgedeckt" behandeln, als überhaupt
                // vorhanden sind.
                rounds_covered: (rounds_covered.max(0) as usize)
                    .min(app_logic::compaction::round_count(&loaded)),
            }),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(error = %err, "session summary could not be loaded on resume");
                None
            }
        };
        (
            // spec-reviewer-Fund (Review dieses Schritts): HIER bewusst
            // KEINE Rundenkürzung mehr — ein früherer Versuch, hier
            // `compaction::truncate_rounds_with_placeholder` unbedingt
            // aufzurufen, kürzte JEDE wiederaufgenommene Sitzung mit mehr
            // als `MIN_PRESERVED_ROUNDS` Runden sofort auf 3, unabhängig
            // vom tatsächlichen Token-Budget (Spec 0057 §3.2 definiert die
            // letzten N Runden als UNTERGRENZE, nicht als generelle
            // Obergrenze). Zwei konkrete Folgeschäden: (a) das Frontend
            // zeigte nach "Fortsetzen" nur noch die letzten 3 Runden statt
            // der vollständigen Historie, (b) `history_contains_untrusted_
            // content` unten lief auf der bereits gekürzten Fassung und
            // konnte einen früher eingeschleusten Inhalt übersehen, wodurch
            // `untrusted_content_ingested` fälschlich `false` startete und
            // die Post-Ingest-Eskalation (Spec 0039, Abschnitt 5.1) im
            // wiederaufgenommenen Tab ausblieb. `loaded` bleibt deshalb
            // unverändert — der vollständige, budgetbewusste
            // Kompaktierungslauf (`compaction::compact_for_send`) greift
            // ohnehin spätestens vor dem ersten `send()` dieser Sitzung.
            loaded,
            Some(existing_id),
            initial_summary,
        )
    } else if !should_create_chat_session(is_local, persist_chat_session) {
        (Vec::new(), None, None)
    } else {
        match state
            .chat_session_store
            .create_session(&server_id, Some(active_config.id.0))
            .await
        {
            Ok(id) => (Vec::new(), Some(id), None),
            Err(err) => {
                // Spec 0034 führt reine Persistenz ein, kein hartes
                // Zusatz-Erfordernis fürs Verbinden selbst — ein
                // Schreibfehler hier (z. B. volle Festplatte) soll den
                // eigentlichen SSH-Verbindungsaufbau nicht verhindern, nur
                // die Chat-Historie dieser einen Sitzung bleibt dann
                // unpersistiert.
                tracing::warn!(error = %err, "chat session creation failed");
                (Vec::new(), None, None)
            }
        }
    };
    // Spec 0064: vor die (Resume- oder frische) Historie gestellt — s.
    // `os_banner_message`-Kommentar oben. Nicht über `push_history_scoped`
    // (das würde ihn zusätzlich in die DB schreiben, jedes Mal aufs Neue
    // bei jedem Resume derselben Sitzung).
    if let Some(banner) = os_banner_message {
        initial_history.insert(0, banner);
    }

    // Spec 0039, Abschnitt 5: der System-Prompt oben enthält bereits
    // gefencte Notizen, falls vorhanden — die Sitzung startet dann mit
    // gesetztem Flag, nicht erst nach der ersten Kommando-Ausführung.
    // Zusätzlich (Spec 0034 mit diesem Schritt erstmals real, s.
    // `history_contains_untrusted_content`-Doc-Kommentar): eine
    // wiederaufgenommene Sitzung mit vorbelasteter Historie startet
    // ebenfalls mit gesetztem Flag — ein früher schon gelesener
    // Serverinhalt darf beim Fortsetzen nicht fälschlich als "sauber"
    // gelten.
    let starts_with_untrusted_content =
        notes_present || history_contains_untrusted_content(&initial_history);
    // MCP-Ausschluss aus der rollierenden Summary (Nachtrag zu Spec 0057
    // §2.1): `initial_history` kommt entweder leer (frische Sitzung) oder
    // aus `chat_messages` (Resume) — Letzteres enthält strukturell NIE
    // MCP-originierte Nachrichten (die werden dort nie persistiert, Spec
    // 0034 §10/`push_history_scoped`s `persist: false`) — deshalb
    // durchweg `false`, unabhängig vom Resume-/Frisch-Fall.
    let initial_mcp_origin_flags = vec![false; initial_history.len()];

    let loss_watch = transport.connection_loss_watch();
    let session = Arc::new(
        Session::new(SessionParts {
            transport: app_logic::session::SessionTransport::new(transport),
            ai_provider,
            ai_provider_budget,
            context: tokio::sync::Mutex::new(SessionContext {
                system_context,
                history: initial_history,
                available_actions,
                max_tokens_hint: None,
            }),
            filter_engine: Box::new(FilterEngine::new(state.policy_store.clone())),
            server_id,
            tags: server.tags,
            terminal: std::sync::Mutex::new(None),
            redactor: app_logic::session::SwappableRedactor::new(redactor),
            ai_provider_label: active_config.display_name,
            ai_model: active_config.model,
            system_context_parts: tokio::sync::Mutex::new(system_context_parts),
            model_context_window_tokens,
            summary: tokio::sync::Mutex::new(initial_summary),
            mcp_origin_flags: std::sync::Mutex::new(initial_mcp_origin_flags),
            sudo_password: app_logic::session::SwappableSudoPassword::new(sudo_password),
            status: std::sync::Mutex::new(app_logic::events::ConnectionStatus::Connected),
            pending_action: std::sync::Mutex::new(None),
            mcp_confirmation_claim: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop_notify: tokio::sync::Notify::new(),
            chat_turn: std::sync::Mutex::new(app_logic::session::ChatTurnState::default()),
            risk_second_opinion_provider,
            risk_second_opinion_budget,
            red_risk_always_confirm,
            running_command_cancellations: state.running_command_cancellations.clone(),
            outgoing_transform: state.outgoing_transform.clone(),
            chat_content: state.chat_content.clone(),
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(
                starts_with_untrusted_content,
            ),
            post_ingest_policy,
            injection_check_provider,
            injection_check_budget,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
            second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(
                second_opinion_setup_failed,
            ),
            injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(
                injection_check_inactive,
            ),
            chat_session_store: chat_session_id.map(|_| state.chat_session_store.clone()),
            // Spec 0057, §1: dieselbe Gating-Logik wie `chat_session_store`
            // direkt darüber — das Ledger braucht dieselbe `chat_sessions.id`
            // als FK (Migration 0011), kein unabhängiger Persistenz-Pfad.
            ledger_store: chat_session_id.map(|_| state.ledger_store.clone()),
            chat_session_id: tokio::sync::Mutex::new(chat_session_id),
            ai_request_paced_at: tokio::sync::Mutex::new(None),
            auto_reconnect: Default::default(),
        })
        .with_start_directory(start_directory)
        .with_is_root(is_root),
    );
    state.sessions.insert(session_id, Arc::clone(&session));
    crate::auto_reconnect::install_hook(app, &session);
    if let Some(watch) = loss_watch {
        app_logic::session::spawn_connection_watch(
            session_id,
            &session,
            watch,
            Arc::new(crate::event_emitter::TauriEventEmitter(app.clone())),
        );
    }

    tracing::info!(session_id = %session_id, server_id = %server_id.0, "session connected");
    emit_connection_status_changed(
        &crate::event_emitter::TauriEventEmitter(app.clone()),
        session_id,
        ConnectionStatus::Connected,
        None,
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
        session.auto_reconnect.stop();
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
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
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
        session.reject_pending_confirmation(&state.pending_action_confirmations);
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
            state.profile_store.get_server(&server_id).await?
        };
        let transport = establish_transport(
            app,
            state,
            server_id,
            session_id,
            &server,
            interactive,
            None,
        )
        .await?;
        // The sudo password is read again, so the redactor knows the
        // current value.
        let sudo_password = app_logic::server_redaction::read_sudo_password_for_redaction(
            state.credential_store.as_ref(),
            server_id,
        );
        let scope = extension_scope(app, state.profile_store.as_ref(), server_id).await;
        let extension_secrets = app_logic::session_extension::known_secrets(
            state.session_extension.as_deref(),
            scope.as_ref(),
        );
        let redactor = app_logic::session_extension::redactor_with_secrets(
            sudo_password.as_ref(),
            &extension_secrets,
        );
        Ok::<_, CommandError>((transport, redactor, sudo_password))
    }
    .await;

    match result {
        Ok((transport, redactor, sudo_password)) => {
            claim.succeed(transport, redactor, sudo_password).await;
            tracing::info!(session_id = %session_id, "session reconnected");
            emit_connection_status_changed(&emitter, session_id, ConnectionStatus::Connected, None);
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
        .await?
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
    connect_session_with_progress(
        &app,
        &state,
        server_id,
        tab_session_id,
        Some(session_id),
        true,
        Some(&progress),
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
    Ok(state
        .chat_session_store
        .rename_session(session_id, &new_title)
        .await?)
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
    Ok(state.chat_session_store.delete_session(session_id).await?)
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
mod connect_session_gate_tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, reset, test_app};
    use tauri_plugin_store::StoreExt;

    /// Spec 0031, Abschnitt 4: "`connect()` schlägt fehl/wird blockiert,
    /// solange `first_run_notice_acknowledged` `false` ist" — geprüft am
    /// exakten Türsteher-Schritt, den `connect_session` als Allererstes
    /// aufruft, bevor irgendein anderer Teil der Verbindungslogik läuft.
    /// `_guard`/`reset(...)`: s. `first_run_notice::test_support`-Moduldoc
    /// — dieser Test teilt sich denselben echten Store mit
    /// `first_run_notice::tests` und muss daher denselben Mutex halten und
    /// seinen eigenen Ausgangszustand explizit herstellen.
    #[test]
    fn test_connect_session_gate_blocks_when_not_acknowledged() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset(&handle);

        let err = ensure_first_run_notice_acknowledged(&handle)
            .expect_err("Erststart-Hinweis wurde nie bestätigt, muss fehlschlagen");
        assert_eq!(err.code, Some("FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED"));
    }

    #[test]
    fn test_connect_session_gate_passes_once_acknowledged() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset(&handle);
        let store = handle
            .store("settings.json")
            .expect("Store sollte sich öffnen lassen");
        store.set("first_run_notice_acknowledged", serde_json::json!(true));

        assert!(ensure_first_run_notice_acknowledged(&handle).is_ok());
        reset(&handle);
    }

    /// Spec 0031, Abschnitt 4: "Bestätigung setzt die Einstellung korrekt
    /// und dauerhaft (übersteht einen simulierten Neustart)" — eine zweite,
    /// unabhängig aufgebaute `App`-Instanz (= simulierter Neustart) muss
    /// den zuvor per `.save()` auf die Festplatte geschriebenen Wert
    /// wiederfinden, nicht nur innerhalb derselben `App`-Instanz.
    #[test]
    fn test_acknowledgement_persists_across_a_simulated_restart() {
        let _guard = lock();
        {
            let first_run_app = test_app();
            let handle = first_run_app.handle().clone();
            reset(&handle);
            let store = handle
                .store("settings.json")
                .expect("Store sollte sich öffnen lassen");
            store.set("first_run_notice_acknowledged", serde_json::json!(true));
            store.save().expect("Store sollte sich speichern lassen");
        }

        // Neue, komplett unabhängige `App`-Instanz mit eigenem
        // Store-Cache — simuliert einen App-Neustart, bei dem nichts mehr
        // im Speicher steht außer dem, was tatsächlich auf der Platte
        // gelandet ist (`store.save()` oben).
        let restarted_app = test_app();
        let restarted_handle = restarted_app.handle().clone();
        assert!(ensure_first_run_notice_acknowledged(&restarted_handle).is_ok());
        reset(&restarted_handle);
    }
}

/// Spec 0040, Abschnitt 4, erster Fix-Punkt: "MCP-ausgelöste Aktionen
/// erzeugen keine `chat_sessions`-Zeile". `should_create_chat_session` ist
/// die vollständige, isolierte Entscheidung dahinter — dieser Test deckt
/// damit den Fix ab, ohne den (hier nicht sinnvoll mockbaren) echten
/// SSH-Verbindungsaufbau in `connect_session` selbst zu brauchen (s.
/// dortiger Doc-Kommentar).
#[cfg(test)]
mod should_create_chat_session_tests {
    use super::*;

    #[test]
    fn test_mcp_triggered_new_connection_creates_no_chat_session() {
        assert!(!should_create_chat_session(
            /* is_local */ false, /* persist_chat_session */ false
        ));
    }

    #[test]
    fn test_regular_human_connection_creates_a_chat_session() {
        assert!(should_create_chat_session(
            /* is_local */ false, /* persist_chat_session */ true
        ));
    }

    #[test]
    fn test_local_pseudo_server_never_creates_a_chat_session_even_if_persist_requested() {
        assert!(!should_create_chat_session(
            /* is_local */ true, /* persist_chat_session */ true
        ));
    }
}

/// Spec 0047, Fund D2: der blanket `impl<E: Display> From<E> for
/// CommandError` (s. `error.rs`) setzt immer `code: None` — vor der
/// Extraktion von `map_connect_result` verwarf `connect_session` damit
/// stillschweigend `SshError::code()`, obwohl der Code existiert und im
/// Frontend registriert ist (`errorCodes.ts`s `KNOWN_ERROR_CODES`). Ohne
/// den Code fällt das Frontend auf den rohen, hart-deutschen
/// `Display`-Text zurück (inkl. eingebettetem OS-Fehlertext), auch im
/// englischen UI.
#[cfg(test)]
mod map_connect_result_tests {
    use super::*;
    use ssh_manager_core::ssh::{HopLabel, SecretKind};

    /// `ConnectOutcome` hat kein `Debug` (es trägt ein Trait-Objekt), also
    /// kein `expect_err` — der Fehlerfall wird von Hand ausgepackt.
    fn expect_error(
        result: CommandResult<ssh_transport::ConnectOutcome>,
        what: &str,
    ) -> CommandError {
        match result {
            Ok(_) => panic!("{what}"),
            Err(command_error) => command_error,
        }
    }

    #[test]
    fn test_connect_error_carries_its_stable_code_not_none() {
        let err = SshError::ConnectionFailed("Connection refused (os error 61)".to_string());
        let expected_code = err.code();
        let expected_message = err.to_string();

        let result = map_connect_result(Err(err));

        let command_error = match result {
            Ok(_) => panic!("erwarteter Fehler wurde nicht als Err geliefert"),
            Err(command_error) => command_error,
        };
        assert_eq!(command_error.code, Some(expected_code));
        assert_eq!(command_error.message, expected_message);
    }

    #[test]
    fn test_connect_success_passes_the_outcome_through_unchanged() {
        let transport: Box<dyn ssh_manager_core::ssh::SshTransport> =
            Box::new(ssh_transport::LocalTransport::new());
        let outcome = ssh_transport::ConnectOutcome::Connected(transport);

        let result = map_connect_result(Ok(outcome));

        assert!(matches!(
            result,
            Ok(ssh_transport::ConnectOutcome::Connected(_))
        ));
    }

    /// Spec 0098 T6 (A4, Verbindungs**aufbau**), seit Spec 0101 A9.1 mit
    /// dem neuen Code: Die Abbildung auf den Code — der Teil des Aufbaus,
    /// der ohne echte Verbindung prüfbar ist (§7). Ein Fehler des
    /// Secret-Speichers auf einem Hop erreicht das Frontend als
    /// `SECRET_STORE_FAILED`, nicht als `SSH_CREDENTIAL_RESOLUTION_FAILED`
    /// und **nicht** als `KEYCHAIN_*`.
    ///
    /// **Gegen den Stand vor Spec 0101 scheitert das:** dort kam
    /// `KEYCHAIN_ACCESS_FAILED` und die Meldung nannte den Schlüsselbund.
    #[test]
    fn test_spec_0101_a9_1_a_secret_store_failure_during_setup_names_the_store() {
        let err = SshError::CredentialStoreFailed {
            secret: SecretKind::Password,
            hop: Some(HopLabel {
                username: "deploy".to_string(),
                host: "jump.invalid".to_string(),
                port: 22,
            }),
        };

        let command_error = expect_error(
            map_connect_result(Err(err)),
            "ein Fehler des Secret-Speichers darf keinen Verbindungsaufbau liefern",
        );

        assert_eq!(
            command_error.code,
            Some(app_logic::error::SECRET_STORE_FAILED)
        );
        assert_ne!(command_error.code, Some("SSH_CREDENTIAL_RESOLUTION_FAILED"));
        assert_ne!(
            command_error.code,
            Some(app_logic::error::KEYCHAIN_ACCESS_FAILED)
        );
        assert_ne!(
            command_error.code,
            Some(app_logic::error::KEYCHAIN_UNAVAILABLE)
        );
        let lower = command_error.message.to_lowercase();
        assert!(
            !lower.contains("schlüsselbund") && !lower.contains("keychain"),
            "A9.1: kein „Schlüsselbund\" in der Meldung: {}",
            command_error.message
        );
        // Spec 0076, A-8: Welcher Hop es war, bleibt sichtbar.
        assert!(
            command_error.message.contains("deploy@jump.invalid:22"),
            "die Hop-Angabe gehört in die Meldung: {}",
            command_error.message
        );
    }

    /// Spec 0098, T7 (A4, `NotFound`): Ein fehlender Eintrag ist **keine**
    /// Störung des Speichers und behält `SSH_CREDENTIAL_RESOLUTION_FAILED`
    /// (Spec 0071 A14/I4: „unbekannt" ist nicht „nein").
    #[test]
    fn test_spec_0098_t7_a_missing_entry_is_not_reported_as_a_store_failure() {
        let err = SshError::CredentialResolutionFailed(
            "Passwort: kein Credential für Referenz 'server:1:password' gefunden".to_string(),
        );

        let command_error = expect_error(
            map_connect_result(Err(err)),
            "ein fehlender Eintrag bleibt ein Fehler",
        );

        assert_eq!(
            command_error.code,
            Some("SSH_CREDENTIAL_RESOLUTION_FAILED"),
            "ein fehlender Eintrag darf nicht als Störung des Speichers gelten"
        );
    }
}

/// Gibt die Rohbestandteile des System-Prompts (Spec 0057, §4.1:
/// [`app_logic::compaction::SystemContextParts`] — getrennt gehalten statt
/// direkt zusammengefügt, damit die Kompaktierung Notiz-Sektionen einzeln
/// nach Scope priorisiert kürzen kann) zurück, zusammen mit der Info, ob
/// die Notizen nicht-leer sind (Spec 0039, Abschnitt 5) — der System-Prompt
/// wird bei **jeder** Nutzer-Nachricht neu gebaut und in jede KI-Anfrage
/// eingebettet; enthält er Notizen, ist damit ab diesem Zeitpunkt bereits
/// Inhalt aus einer nicht vertrauenswürdigen Quelle in den KI-Kontext
/// gelangt. Der Aufrufer nutzt das, um `Session::untrusted_content_
/// ingested` entsprechend zu setzen (monoton, s. dortiger Kommentar). Den
/// fertig zusammengesetzten String liefert `SystemContextParts::assemble`.
pub(super) async fn build_session_system_context<R: tauri::Runtime>(
    app: &AppHandle<R>,
    server_name: &str,
    server_id: &ServerId,
    tags: &[String],
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
) -> (app_logic::compaction::SystemContextParts, bool) {
    // Spec 0032: der lokale Pseudo-Server hat keine `servers`-Zeile —
    // `profile_store.get_server` schlägt für ihn immer fehl, wodurch diese
    // Funktion sonst dauerhaft mit leeren Notizen liefe, obwohl über
    // `local_server::synthetic_server` tatsächlich welche hinterlegt sein
    // können (unabhängiger Review-Pass, s. docs/adr/0026).
    // `tracing::warn!` statt stillem `unwrap_or_default()`/leerem Fallback
    // (unabhängiger Review-Pass, Spec 0003/0004): ein Fehler hier bedeutet
    // nicht nur "keine Notizen geladen", sondern dass sicherheitsrelevanter
    // Kontext (z. B. "Produktionsserver, nur außerhalb des Wartungsfensters
    // anfassen") ohne jedes sichtbare Signal aus dem System-Prompt
    // verschwindet — die KI schlägt dann Kommandos vor, die sie mit
    // geladenen Notizen nicht vorschlagen würde.
    // Spec 0039, Abschnitt 3: unformatiert als (Quelle, Notiztext)-Paare
    // geladen statt als fertigen String — jeder Abschnitt muss einzeln
    // über `fence_untrusted` laufen, bevor er unten in den System-Prompt
    // eingebettet wird (der schwerwiegendste der vier in Spec 0039
    // genannten Befunde: Notizen persistieren über Sitzungen hinweg, eine
    // einmal eingeschleuste Anweisung wirkt also nicht nur einmalig).
    let note_sections: Vec<(String, String)> = if app_logic::dto::is_local(*server_id) {
        let local_notes = crate::local_server::synthetic_server(app).notes;
        if local_notes.trim().is_empty() {
            Vec::new()
        } else {
            vec![(format!("Server \"{server_name}\""), local_notes)]
        }
    } else {
        match profile_store.get_server(server_id).await {
            Ok(s) => match effective_notes_sections(&s, profile_store).await {
                Ok(sections) => sections,
                Err(err) => {
                    tracing::warn!(
                        server_id = %server_id.0,
                        error = %err,
                        "effective_notes_sections fehlgeschlagen — Session-Kontext enthält keine Notizen",
                    );
                    Vec::new()
                }
            },
            Err(err) => {
                tracing::warn!(
                    server_id = %server_id.0,
                    error = %err,
                    "get_server fehlgeschlagen — Session-Kontext enthält keine Notizen",
                );
                Vec::new()
            }
        }
    };

    // Issue #90: Sprache bei JEDEM Aufbau neu gelesen — ein Wechsel der
    // UI-Sprache gilt ab dem nächsten Aufbau, ohne Neustart.
    let language = crate::ui_language::session_prompt_language(app);
    let mut context = app_logic::system_prompt::base_prompt(language, server_name);

    let eval_ctx = EvalContext {
        server_id: *server_id,
        tags: tags.to_vec(),
    };
    let scope = EffectiveScope::from(&eval_ctx);
    let rules = policy_store.rules_for(&scope).await;
    let allow_rules = app_logic::system_prompt::allow_rule_lines(&rules);

    context.push_str(&app_logic::system_prompt::allow_rules_section(
        language,
        &allow_rules,
    ));

    let parts = app_logic::compaction::SystemContextParts {
        base: context,
        note_sections,
        language,
    };
    let has_notes = parts.has_notes();
    (parts, has_notes)
}

/// Spec 0038, section 2a: the scope handed to the session extension. The
/// local pseudo-server has no stored row, so its scope is built from the
/// synthetic server.
pub(crate) async fn extension_scope<R: tauri::Runtime>(
    app: &AppHandle<R>,
    profile_store: &dyn ProfileStore,
    server_id: ServerId,
) -> Option<app_logic::session_extension::ServerScope> {
    let local_tags = app_logic::dto::is_local(server_id)
        .then(|| crate::local_server::synthetic_server(app).tags);
    app_logic::session_extension::scope_for_id(profile_store, server_id, local_tags).await
}

/// Ausgang des Wartens auf die Host-Key-Entscheidung (Spec 0068, Teil 5b).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HostKeyWait {
    Decided(HostKeyUserDecision),
    /// Sender gedroppt (Neu-Registrierung, App-Ende) — Abbruch.
    Abandoned,
    /// Frist abgelaufen — gilt als Ablehnung, der Host-Key wird **nie**
    /// durch Zeitablauf vertraut.
    TimedOut,
}

/// Spec 0068, Teil 5b: wartet höchstens `timeout` auf `confirm_host_key`
/// (analog zum Confirm-Timeout aus Spec 0046, Fund 4). Geht ein Frontend
/// verloren, hängt `connect()` damit nicht mehr ewig. Beim Zeitablauf wird
/// nur der Eintrag DIESER Registrierung abgeräumt (`generation`), nie einer,
/// den ein `connect()`-Retry unter derselben `SessionId` neu angelegt hat.
pub(crate) async fn wait_for_host_key_decision(
    registry: &ConfirmationRegistry<SessionId, HostKeyUserDecision>,
    session_id: SessionId,
    generation: RegistrationGeneration,
    rx: tokio::sync::oneshot::Receiver<HostKeyUserDecision>,
    timeout: std::time::Duration,
) -> HostKeyWait {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(decision)) => HostKeyWait::Decided(decision),
        Ok(Err(_)) => HostKeyWait::Abandoned,
        Err(_elapsed) => {
            registry.cancel_if_current(&session_id, generation);
            HostKeyWait::TimedOut
        }
    }
}

/// Issue #37: schließt ein Warten auf die Host-Key-Entscheidung ab. Meldet
/// `host-key-verification-ended` für genau diese Registrierung
/// (`generation`) — bei jedem Ausgang, damit das Frontend die passende
/// Abfrage schließen kann — und bildet den Ausgang auf das Ergebnis von
/// `connect()` ab. Timeout und Abbruch bleiben Fehler (nie Vertrauen,
/// Spec 0068, Teil 5b); das Ereignis selbst entscheidet nichts.
pub(crate) fn finish_host_key_wait(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    generation: RegistrationGeneration,
    wait: HostKeyWait,
    host: &str,
    port: u16,
) -> CommandResult<HostKeyUserDecision> {
    let reason = match wait {
        HostKeyWait::Decided(_) => HostKeyPromptEndReason::Decided,
        HostKeyWait::Abandoned => HostKeyPromptEndReason::Abandoned,
        HostKeyWait::TimedOut => HostKeyPromptEndReason::TimedOut,
    };
    emit_host_key_verification_ended(emitter, session_id, generation.as_u64(), reason);
    match wait {
        HostKeyWait::Decided(decision) => Ok(decision),
        HostKeyWait::Abandoned => {
            // Spec 0069, Teil A5 (spec-reviewer-Fund, Review dieses
            // Schritts): der dritte Ausgang derselben Host-Key-Wartestelle
            // (neben Reject/TimedOut, die bereits einen Code tragen) zeigte
            // bislang rohen deutschen Text auch in der englischen UI. Neuer,
            // additiver Code (E3).
            Err(CommandError::with_code(
                "Verbindungsaufbau abgebrochen",
                "SSH_CONNECTION_ABANDONED",
            ))
        }
        HostKeyWait::TimedOut => {
            tracing::warn!(
                session_id = %session_id,
                host = %host,
                port,
                "host key confirmation timed out, treating as rejected",
            );
            // Spec 0069, Teil A4: Code zusätzlich zur bisherigen,
            // host:port-tragenden Meldung (Text unverändert).
            Err(CommandError::with_code(
                format!(
                    "Verbindung zu {host}:{port} abgebrochen: Host-Key-Bestätigung \
                     nicht rechtzeitig beantwortet (nicht vertraut)"
                ),
                "SSH_HOST_KEY_CONFIRM_TIMEOUT",
            ))
        }
    }
}

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

#[cfg(test)]
mod host_key_wait_tests {
    use app_logic::events::TestEmitter;
    use uuid::Uuid;

    use super::*;

    /// Spec 0068, Teil 5b: ein nie beantworteter Host-Key-Dialog (Frontend
    /// verloren) endet nach der Frist als Ablehnung — nie als Vertrauen —
    /// und räumt seinen eigenen Eintrag ab.
    #[tokio::test]
    async fn test_host_key_wait_times_out_as_rejection_and_cleans_up() {
        let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
            ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();
        let (generation, rx) = registry.register_tracked(session_id);

        let wait = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_host_key_decision(
                &registry,
                session_id,
                generation,
                rx,
                std::time::Duration::from_millis(50),
            ),
        )
        .await
        .expect("darf nicht ewig warten");

        assert_eq!(wait, HostKeyWait::TimedOut);
        assert!(!registry.contains(&session_id));
        assert!(
            registry
                .resolve(&session_id, HostKeyUserDecision::Trust)
                .is_err(),
            "ein spätes Trust darf nach Ablauf niemanden mehr erreichen"
        );
    }

    #[tokio::test]
    async fn test_host_key_wait_returns_the_user_decision() {
        let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
            ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();
        let (generation, rx) = registry.register_tracked(session_id);
        registry
            .resolve(&session_id, HostKeyUserDecision::Reject)
            .unwrap();

        let wait = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_host_key_decision(
                &registry,
                session_id,
                generation,
                rx,
                std::time::Duration::from_secs(60),
            ),
        )
        .await
        .expect("darf nicht hängen");
        assert_eq!(wait, HostKeyWait::Decided(HostKeyUserDecision::Reject));
    }

    fn recorded_events(emitter: &TestEmitter) -> Vec<(String, serde_json::Value)> {
        emitter.events.lock().unwrap().clone()
    }

    /// Issue #37: läuft das Warten in den Timeout, meldet das Backend das
    /// Ende genau dieser Abfrage (`sessionId` + `promptId`), und `connect()`
    /// scheitert weiterhin mit `SSH_HOST_KEY_CONFIRM_TIMEOUT` — vertraut
    /// wird nichts.
    #[tokio::test]
    async fn test_host_key_timeout_emits_ended_event_and_still_rejects() {
        let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
            ConfirmationRegistry::new();
        let emitter = TestEmitter::default();
        let session_id = Uuid::new_v4();
        let (generation, rx) = registry.register_tracked(session_id);

        let wait = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_host_key_decision(
                &registry,
                session_id,
                generation,
                rx,
                std::time::Duration::from_millis(50),
            ),
        )
        .await
        .expect("darf nicht ewig warten");
        let result = finish_host_key_wait(
            &emitter,
            session_id,
            generation,
            wait,
            "prod-1.internal",
            2222,
        );

        let err = result.expect_err("ein Timeout darf nie zu einer Entscheidung werden");
        assert_eq!(err.code, Some("SSH_HOST_KEY_CONFIRM_TIMEOUT"));
        assert_eq!(
            recorded_events(&emitter),
            vec![(
                "host-key-verification-ended".to_string(),
                serde_json::json!({
                    "sessionId": session_id,
                    "promptId": generation.as_u64(),
                    "reason": "timed_out",
                }),
            )]
        );
        assert!(
            registry
                .resolve(&session_id, HostKeyUserDecision::Trust)
                .is_err(),
            "ein spätes Trust darf nach Ablauf niemanden mehr erreichen"
        );
    }

    /// Issue #37: ein `connect()`-Retry registriert unter derselben
    /// `SessionId` neu und droppt damit den Sender der alten Abfrage. Das
    /// Ende-Ereignis der alten Abfrage trägt nur deren `promptId` — die neue
    /// Abfrage bleibt offen und beantwortbar.
    #[tokio::test]
    async fn test_host_key_abandoned_wait_emits_ended_event_for_its_registration_only() {
        let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
            ConfirmationRegistry::new();
        let emitter = TestEmitter::default();
        let session_id = Uuid::new_v4();
        let (old_generation, old_rx) = registry.register_tracked(session_id);
        let (new_generation, new_rx) = registry.register_tracked(session_id);
        assert_ne!(old_generation.as_u64(), new_generation.as_u64());

        let wait = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_host_key_decision(
                &registry,
                session_id,
                old_generation,
                old_rx,
                std::time::Duration::from_secs(60),
            ),
        )
        .await
        .expect("darf nicht hängen");
        assert_eq!(wait, HostKeyWait::Abandoned);
        let result = finish_host_key_wait(
            &emitter,
            session_id,
            old_generation,
            wait,
            "prod-1.internal",
            2222,
        );

        let err = result.expect_err("ein Abbruch darf nie zu einer Entscheidung werden");
        assert_eq!(err.code, Some("SSH_CONNECTION_ABANDONED"));
        assert_eq!(
            recorded_events(&emitter),
            vec![(
                "host-key-verification-ended".to_string(),
                serde_json::json!({
                    "sessionId": session_id,
                    "promptId": old_generation.as_u64(),
                    "reason": "abandoned",
                }),
            )]
        );
        // Die neue Abfrage ist unberührt und erreicht ihren Wartenden.
        assert!(registry.contains(&session_id));
        registry
            .resolve(&session_id, HostKeyUserDecision::Reject)
            .expect("die neue Abfrage muss noch beantwortbar sein");
        assert_eq!(new_rx.await, Ok(HostKeyUserDecision::Reject));
    }

    /// Issue #37: auch nach einer Nutzerentscheidung kommt das
    /// Ende-Ereignis; die Entscheidung selbst wird unverändert durchgereicht.
    #[test]
    fn test_host_key_decided_wait_emits_ended_event_and_passes_decision_through() {
        let emitter = TestEmitter::default();
        let registry: ConfirmationRegistry<SessionId, HostKeyUserDecision> =
            ConfirmationRegistry::new();
        let session_id = Uuid::new_v4();
        let (generation, _rx) = registry.register_tracked(session_id);

        let result = finish_host_key_wait(
            &emitter,
            session_id,
            generation,
            HostKeyWait::Decided(HostKeyUserDecision::Reject),
            "prod-1.internal",
            2222,
        );

        assert_eq!(result.ok(), Some(HostKeyUserDecision::Reject));
        let events = recorded_events(&emitter);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["reason"], "decided");
        assert_eq!(events[0].1["promptId"], generation.as_u64());
    }
}

/// Issue #395: the in-process SSH test server of `ssh-transport`, reused for
/// the real two-hop reconnect test in `connect_flow_tests`. Test-only.
#[cfg(test)]
#[allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
#[path = "../../../ssh-transport/tests/fixtures/test_server.rs"]
mod two_hop_fixture;

/// Issue #259: `connect_session` gegen einen eingespeisten `Connector` — der
/// echte Verbindungsaufbau (Verbinden, Fortsetzen, Fehlerpfad beim
/// Fortsetzen) ohne Netzwerk. Echte SQLite-Stores (je Test eigene
/// Datenbankdateien), `MockRuntime`, ein Mock-Connector mit Mock-Transport.
// Testcode-Ausnahme zum `deny` — s. `lib.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod connect_flow_tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use persistence_sqlite::{AiProviderConfig, SqliteProfileStore};
    use ssh_manager_core::ai::{ChatMessage, MessageContent, ProviderId, ProviderType, Role};
    use ssh_manager_core::profiles::{
        AuthMethod, CredentialRef, CredentialStore, PostIngestPolicy, ProfileStore, Server,
    };
    use ssh_manager_core::ssh::{
        CommandOutput, ConnectionTarget, HostKeyDecision, HostKeyStore, InteractiveShell,
        KeyFileReader, PtySize, SftpSession, SshTransport,
    };
    use ssh_transport::ConnectOutcome;
    use tauri::Listener as _;
    use tauri_plugin_store::StoreExt;

    use app_logic::state::AppState;
    use app_logic::test_connection::Connector;
    use app_logic::test_support::InMemoryCredentialStore;

    use super::*;
    use crate::first_run_notice::test_support::{lock_async, reset, test_app};

    const PROVIDER_SECRET_REF: &str = "test:ai";

    /// Zeichnet auf, ob `disconnect()` aufgerufen wurde.
    struct RecordingTransport {
        disconnected: Arc<AtomicBool>,
    }

    #[async_trait]
    impl SshTransport for RecordingTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            Ok(CommandOutput {
                stdout: b"Linux test 6.1 x86_64\n".to_vec(),
                stderr: Vec::new(),
                exit_code: Some(0),
                truncated: false,
            })
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            unreachable!("not used by connect_session")
        }
        async fn open_sftp_via_exec(
            &mut self,
            _command: &str,
        ) -> Result<Box<dyn SftpSession>, SshError> {
            unreachable!("not used by connect_session")
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            self.disconnected.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Liefert genau einmal das vorbereitete Ergebnis; `on_call` läuft vor
    /// der Rückgabe (z. B. um eine Datenbank zu schließen).
    struct ScriptedConnector {
        result: Mutex<Option<Result<ConnectOutcome, SshError>>>,
        calls: AtomicUsize,
        /// Hop count of the target of the latest connect.
        last_hops: AtomicUsize,
    }

    impl ScriptedConnector {
        /// Issue #355: queues the result for the next connect (a reconnect).
        fn push(&self, result: Result<ConnectOutcome, SshError>) {
            *self.result.lock().unwrap() = Some(result);
        }

        fn new(result: Result<ConnectOutcome, SshError>) -> Arc<Self> {
            Arc::new(Self {
                result: Mutex::new(Some(result)),
                calls: AtomicUsize::new(0),
                last_hops: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl Connector for ScriptedConnector {
        async fn connect(
            &self,
            target: &ConnectionTarget,
            _credentials: &(dyn CredentialStore + Send + Sync),
            _key_files: &(dyn KeyFileReader + Send + Sync),
            _host_keys: Arc<dyn HostKeyStore>,
            _log: &ConnectLog,
            _limits: ssh_transport::ConnectLimits,
        ) -> Result<ConnectOutcome, SshError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.last_hops.store(target.hops.len(), Ordering::SeqCst);
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("connector called more than once")
        }
    }

    struct NoHostKeys;
    impl HostKeyStore for NoHostKeys {
        fn check(&self, _host: &str, _port: u16, key: &[u8]) -> HostKeyDecision {
            HostKeyDecision::Unknown {
                fingerprint: format!("{}", key.len()),
            }
        }
        fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
            Ok(())
        }
    }

    fn server() -> Server {
        let now = chrono::Utc::now();
        Server {
            id: ServerId(uuid::Uuid::new_v4()),
            name: "web-1".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "root".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::Balanced,
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            start_directory: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn provider_config() -> AiProviderConfig {
        let now = chrono::Utc::now();
        AiProviderConfig {
            id: ProviderId::new(),
            display_name: "test".to_string(),
            provider_type: ProviderType::Anthropic,
            base_url: None,
            model: "claude-test".to_string(),
            credential_ref: CredentialRef::new(PROVIDER_SECRET_REF),
            is_active: true,
            supports_native_tool_calling: true,
            extra_headers: Vec::new(),
            max_tokens_override: None,
            web_research_enabled: false,
            attestation_url: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Server und Provider stehen in **beiden** Datenbanken: `chat_sessions`
    /// referenziert `servers` und `ai_providers` per Fremdschlüssel.
    struct Fixture {
        state: AppState,
        /// Hält die Datenbankdateien am Leben.
        _dir: tempfile::TempDir,
        /// Die Datenbank mit Chat und Ledger — ein Test schließt sie, um
        /// einen Ladefehler der Historie zu erzeugen.
        chat_db: SqliteProfileStore,
        server_id: ServerId,
    }

    async fn fixture(connector: Arc<dyn Connector>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let main_db = SqliteProfileStore::connect_plaintext(&dir.path().join("main.db"))
            .await
            .unwrap();
        let chat_db = SqliteProfileStore::connect_plaintext(&dir.path().join("chat.db"))
            .await
            .unwrap();
        let server = server();
        let provider = provider_config();
        for db in [&main_db, &chat_db] {
            db.create_server(&server).await.unwrap();
            db.ai_provider_store().create(&provider).await.unwrap();
        }
        main_db
            .ai_provider_store()
            .set_active(&provider.id, chrono::Utc::now())
            .await
            .unwrap();
        let credential_store = InMemoryCredentialStore::new()
            .with_secret(&CredentialRef::new(PROVIDER_SECRET_REF), "sk-test");
        let state = AppState {
            sessions: app_logic::session::SessionManager::new(),
            session_extension: None,
            root_key_fingerprint: [0; 32],
            ai_provider_store: Arc::new(main_db.ai_provider_store()),
            policy_store: main_db.policy_store(),
            prompt_history_store: main_db.prompt_history_store(),
            chat_session_store: chat_db.chat_session_store(),
            ledger_store: chat_db.ledger_store(),
            profile_store: Arc::new(main_db),
            credential_store: Arc::new(credential_store),
            key_file_reader: Arc::new(ssh_manager_core::ssh::mock::MockKeyFileReader::new()),
            keychain: credentials_keyring::KeychainAvailability::Available,
            host_key_store: Arc::new(NoHostKeys),
            connector,
            entitlements: Arc::new(ssh_manager_core::entitlements::FixedEntitlements(
                ssh_manager_core::entitlements::Entitlements::free(),
            )),
            pending_host_key_confirmations: ConfirmationRegistry::new(),
            connect_attempts: Default::default(),
            pending_action_confirmations: ConfirmationRegistry::new(),
            running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
            mcp: app_logic::state::McpState::default(),
            rate_limit_registry: ai_providers::RateLimitRegistry::new(),
            pending_ssh_config_import: std::sync::Mutex::new(None),
            outgoing_transform: None,
            chat_content: Default::default(),
        };
        Fixture {
            state,
            _dir: dir,
            chat_db,
            server_id: server.id,
        }
    }

    fn acknowledged_app() -> tauri::App<tauri::test::MockRuntime> {
        let app = test_app();
        let handle = app.handle().clone();
        reset(&handle);
        handle
            .store("settings.json")
            .unwrap()
            .set("first_run_notice_acknowledged", serde_json::json!(true));
        app
    }

    fn connected(disconnected: &Arc<AtomicBool>) -> Result<ConnectOutcome, SshError> {
        Ok(ConnectOutcome::Connected(Box::new(RecordingTransport {
            disconnected: disconnected.clone(),
        })))
    }

    fn user_text(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn status_events(app: &tauri::App<tauri::test::MockRuntime>) -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        app.handle()
            .listen("connection-status-changed", move |event| {
                sink.lock().unwrap().push(event.payload().to_string());
            });
        seen
    }

    #[tokio::test]
    async fn test_connect_registers_session_emits_connected_and_creates_chat_session() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let events = status_events(&app);
        let disconnected = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&disconnected));
        let fx = fixture(connector.clone()).await;
        let session_id = uuid::Uuid::new_v4();

        let result = connect_session(
            app.handle(),
            &fx.state,
            fx.server_id,
            session_id,
            None,
            true,
        )
        .await
        .expect("connect should succeed");

        assert_eq!(result, session_id);
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert!(fx.state.sessions.get(session_id).is_some());
        assert!(!disconnected.load(Ordering::SeqCst));
        let sessions = fx
            .state
            .chat_session_store
            .list_sessions_for_server(&fx.server_id)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1, "one chat session row is created");
        let session = fx.state.sessions.get(session_id).unwrap();
        assert_eq!(*session.chat_session_id.lock().await, Some(sessions[0].id));
        let seen = events.lock().unwrap().clone();
        assert!(
            seen.iter()
                .any(|p| p.contains(&session_id.to_string())
                    && p.to_lowercase().contains("connected")),
            "connected status event expected, got {seen:?}"
        );
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_connect_without_persisting_creates_no_chat_session_row() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let disconnected = Arc::new(AtomicBool::new(false));
        let fx = fixture(ScriptedConnector::new(connected(&disconnected))).await;

        connect_session(
            app.handle(),
            &fx.state,
            fx.server_id,
            uuid::Uuid::new_v4(),
            None,
            false,
        )
        .await
        .unwrap();

        let sessions = fx
            .state
            .chat_session_store
            .list_sessions_for_server(&fx.server_id)
            .await
            .unwrap();
        assert!(sessions.is_empty());
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_resume_loads_history_and_clears_ended_at() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let disconnected = Arc::new(AtomicBool::new(false));
        let fx = fixture(ScriptedConnector::new(connected(&disconnected))).await;
        let store = &fx.state.chat_session_store;
        let existing = store.create_session(&fx.server_id, None).await.unwrap();
        store
            .append_message(existing, &user_text("earlier question"))
            .await
            .unwrap();
        store.mark_ended(existing).await.unwrap();
        let before = store.list_sessions_for_server(&fx.server_id).await.unwrap();
        assert!(before[0].ended_at.is_some(), "precondition: session ended");

        let tab = uuid::Uuid::new_v4();
        connect_session(
            app.handle(),
            &fx.state,
            fx.server_id,
            tab,
            Some(existing),
            true,
        )
        .await
        .expect("resume should succeed");

        let session = fx.state.sessions.get(tab).expect("session registered");
        assert_eq!(*session.chat_session_id.lock().await, Some(existing));
        let history = session.context.lock().await.history.clone();
        assert!(
            history.iter().any(|m| matches!(
                &m.content,
                MessageContent::Text(t) if t == "earlier question"
            )),
            "stored history is loaded into the session"
        );
        let after = store.list_sessions_for_server(&fx.server_id).await.unwrap();
        assert_eq!(after.len(), 1, "no new chat session row");
        assert!(after[0].ended_at.is_none(), "ended_at is cleared");
        assert!(!disconnected.load(Ordering::SeqCst));
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_resume_with_failing_history_load_disconnects_and_registers_nothing() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let disconnected = Arc::new(AtomicBool::new(false));
        let fx = fixture(ScriptedConnector::new(connected(&disconnected))).await;
        let existing = fx
            .state
            .chat_session_store
            .create_session(&fx.server_id, None)
            .await
            .unwrap();
        // Das Laden der Historie schlägt fehl, wenn die Chat-Datenbank weg
        // ist; Server und Provider liegen in der anderen Datenbank.
        fx.chat_db.close().await;

        let tab = uuid::Uuid::new_v4();
        let err = connect_session(
            app.handle(),
            &fx.state,
            fx.server_id,
            tab,
            Some(existing),
            true,
        )
        .await
        .expect_err("resume must fail when the history cannot be loaded");

        assert!(!err.message.is_empty());
        assert!(
            disconnected.load(Ordering::SeqCst),
            "the established connection must be closed explicitly"
        );
        assert!(fx.state.sessions.get(tab).is_none());
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_connector_error_is_mapped_with_its_stable_code() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let connector = ScriptedConnector::new(Err(SshError::AuthenticationFailed));
        let fx = fixture(connector.clone()).await;
        let tab = uuid::Uuid::new_v4();

        let err = connect_session(app.handle(), &fx.state, fx.server_id, tab, None, true)
            .await
            .expect_err("connector error must fail the connect");

        assert_eq!(err.code, Some(SshError::AuthenticationFailed.code()));
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert!(fx.state.sessions.get(tab).is_none());
        assert!(fx
            .state
            .chat_session_store
            .list_sessions_for_server(&fx.server_id)
            .await
            .unwrap()
            .is_empty());
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_unacknowledged_first_run_notice_never_reaches_the_connector() {
        let _guard = lock_async().await;
        let app = test_app();
        reset(app.handle());
        let connector = ScriptedConnector::new(Err(SshError::Timeout));
        let fx = fixture(connector.clone()).await;

        let err = connect_session(
            app.handle(),
            &fx.state,
            fx.server_id,
            uuid::Uuid::new_v4(),
            None,
            true,
        )
        .await
        .expect_err("gate must block");

        assert_eq!(err.code, Some("FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED"));
        assert_eq!(connector.calls.load(Ordering::SeqCst), 0);
    }

    // --- Issue #355 / Spec 0105: reconnect ---------------------------------

    fn registry_pair() -> (
        crate::elevated_sftp::ElevatedSftpRegistry,
        app_logic::local_path_grants::LocalPathGrants,
    ) {
        (
            crate::elevated_sftp::ElevatedSftpRegistry::default(),
            app_logic::local_path_grants::LocalPathGrants::default(),
        )
    }

    /// Connects and simulates the drop the terminal actor reports.
    async fn connected_then_lost(
        app: &tauri::App<tauri::test::MockRuntime>,
        fx: &Fixture,
        connector: &ScriptedConnector,
        disconnected: &Arc<AtomicBool>,
    ) -> (SessionId, Arc<Session>) {
        connector.push(connected(disconnected));
        let session_id = uuid::Uuid::new_v4();
        connect_session(
            app.handle(),
            &fx.state,
            fx.server_id,
            session_id,
            None,
            true,
        )
        .await
        .unwrap();
        let session = fx.state.sessions.get(session_id).unwrap();
        *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
        (session_id, session)
    }

    #[tokio::test]
    async fn test_reconnect_keeps_session_id_and_ai_context_and_replaces_transport() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let events = status_events(&app);
        let old_closed = Arc::new(AtomicBool::new(false));
        let new_closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&old_closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &old_closed).await;
        session
            .context
            .lock()
            .await
            .history
            .push(user_text("before the drop"));
        let history_len = session.context.lock().await.history.len();
        connector.push(connected(&new_closed));
        let (elevated, grants) = registry_pair();

        reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .expect("reconnect should succeed");

        let after = fx.state.sessions.get(session_id).unwrap();
        assert!(Arc::ptr_eq(&session, &after), "same session object and id");
        assert_eq!(*after.status.lock().unwrap(), ConnectionStatus::Connected);
        assert_eq!(after.context.lock().await.history.len(), history_len);
        assert!(old_closed.load(Ordering::SeqCst), "old transport is closed");
        assert!(
            !new_closed.load(Ordering::SeqCst),
            "new transport stays open"
        );
        assert!(!after.transport.is_closed());
        let seen = events.lock().unwrap().clone();
        assert!(seen.iter().any(|p| p.contains("reconnecting")), "{seen:?}");
        assert!(
            seen.last()
                .is_some_and(|p| p.contains("connected") && !p.contains("dis")),
            "{seen:?}"
        );
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_reconnect_rereads_sudo_password_for_sudo_and_redactor() {
        use app_logic::server_credentials::sudo_password_credential_ref;
        use secrecy::ExposeSecret;
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let old_closed = Arc::new(AtomicBool::new(false));
        let new_closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&old_closed));
        let fx = fixture(connector.clone()).await;
        let cred_ref = sudo_password_credential_ref(fx.server_id);
        fx.state
            .credential_store
            .set(&cred_ref, "old-sudo-pw".to_string().into())
            .unwrap();
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &old_closed).await;
        assert_eq!(
            session.sudo_password.get().unwrap().expose_secret(),
            "old-sudo-pw"
        );
        fx.state
            .credential_store
            .set(&cred_ref, "new-sudo-pw".to_string().into())
            .unwrap();
        connector.push(connected(&new_closed));
        let (elevated, grants) = registry_pair();

        reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .expect("reconnect should succeed");

        let after = fx.state.sessions.get(session_id).unwrap();
        // The password used for `sudo -S` and the redactor come from one read.
        assert_eq!(
            after.sudo_password.get().unwrap().expose_secret(),
            "new-sudo-pw"
        );
        let redacted = after.redactor.redact_text("echo new-sudo-pw");
        assert!(!redacted.contains("new-sudo-pw"), "{redacted}");
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_reconnect_of_a_live_session_does_nothing() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        *session.status.lock().unwrap() = ConnectionStatus::Connected;
        let calls = connector.calls.load(Ordering::SeqCst);
        let (elevated, grants) = registry_pair();

        let err = reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, Some("SESSION_NOT_RECONNECTABLE"));
        assert_eq!(connector.calls.load(Ordering::SeqCst), calls);
        assert!(!closed.load(Ordering::SeqCst), "live connection untouched");
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_second_reconnect_trigger_while_one_runs_starts_no_connect() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        let calls = connector.calls.load(Ordering::SeqCst);
        let (elevated, grants) = registry_pair();

        // The first trigger holds the claim (its connect is "running").
        let running = session.begin_reconnect().expect("first trigger claims");
        let err = reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, Some("SESSION_NOT_RECONNECTABLE"));
        assert_eq!(connector.calls.load(Ordering::SeqCst), calls);
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Reconnecting
        );
        drop(running);
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_failed_reconnect_stays_disconnected_and_can_be_retried() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        let (elevated, grants) = registry_pair();
        connector.push(Err(SshError::ConnectionFailed("down".to_string())));

        let err = reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .unwrap_err();
        assert!(err.code.is_some(), "the translated error code is reported");
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );

        let fresh = Arc::new(AtomicBool::new(false));
        connector.push(connected(&fresh));
        reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .expect("the button stays usable");
        assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_pending_confirmation_at_drop_is_rejected_not_executed_after_reconnect() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        let action_id = uuid::Uuid::new_v4();
        let waiting = fx.state.pending_action_confirmations.register(action_id);
        *session.pending_action.lock().unwrap() = Some(action_id);
        connector.push(connected(&Arc::new(AtomicBool::new(false))));
        let (elevated, grants) = registry_pair();

        reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .unwrap();

        assert!(
            matches!(waiting.await, Ok(app_logic::dto::ActionUserDecision::Deny)),
            "the waiting action is denied, never approved"
        );
        reset(app.handle());
    }

    /// Issue #356: an automatic attempt never opens the host key dialog and
    /// never trusts a key; the tab stays disconnected.
    #[tokio::test]
    async fn test_automatic_attempt_stops_at_a_changed_host_key_without_a_dialog() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        connector.push(Ok(ConnectOutcome::PendingHostKeyConfirmation {
            host: "example.invalid".to_string(),
            port: 22,
            raw_key: vec![1, 2, 3],
            decision: HostKeyDecision::Mismatch {
                expected_fingerprint: "SHA256:old".to_string(),
                actual_fingerprint: "SHA256:new".to_string(),
            },
        }));
        let (elevated, grants) = registry_pair();

        let err = reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            false,
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
        assert!(!app_logic::auto_reconnect::is_retryable_code(err.code));
        assert!(
            fx.state
                .pending_host_key_confirmations
                .resolve(&session_id, HostKeyUserDecision::Trust)
                .is_err(),
            "no host key question was registered"
        );
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_rejected_host_key_during_reconnect_leaves_the_tab_disconnected() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let (session_id, session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        connector.push(Ok(ConnectOutcome::PendingHostKeyConfirmation {
            host: "example.invalid".to_string(),
            port: 22,
            raw_key: vec![1, 2, 3],
            decision: HostKeyDecision::Mismatch {
                expected_fingerprint: "SHA256:old".to_string(),
                actual_fingerprint: "SHA256:new".to_string(),
            },
        }));
        let (elevated, grants) = registry_pair();

        let reject = async {
            loop {
                if fx
                    .state
                    .pending_host_key_confirmations
                    .resolve(&session_id, HostKeyUserDecision::Reject)
                    .is_ok()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (result, ()) = tokio::join!(
            reconnect_session_inner(
                app.handle(),
                &fx.state,
                &elevated,
                &grants,
                session_id,
                true
            ),
            reject
        );

        assert_eq!(result.unwrap_err().code, Some("SSH_HOST_KEY_NOT_TRUSTED"));
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_reconnect_goes_through_the_jump_host_chain_again() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let closed = Arc::new(AtomicBool::new(false));
        let connector = ScriptedConnector::new(connected(&closed));
        let fx = fixture(connector.clone()).await;
        let mut jump = server();
        jump.name = "bastion".to_string();
        fx.state.profile_store.create_server(&jump).await.unwrap();
        let mut target = fx
            .state
            .profile_store
            .get_server(&fx.server_id)
            .await
            .unwrap();
        target.jump_host = Some(jump.id);
        fx.state.profile_store.update_server(&target).await.unwrap();
        let (session_id, _session) = connected_then_lost(&app, &fx, &connector, &closed).await;
        assert_eq!(connector.last_hops.load(Ordering::SeqCst), 2);
        connector.last_hops.store(0, Ordering::SeqCst);
        connector.push(connected(&Arc::new(AtomicBool::new(false))));
        let (elevated, grants) = registry_pair();

        reconnect_session_inner(
            app.handle(),
            &fx.state,
            &elevated,
            &grants,
            session_id,
            true,
        )
        .await
        .unwrap();

        assert_eq!(
            connector.last_hops.load(Ordering::SeqCst),
            2,
            "bastion + target"
        );
        reset(app.handle());
    }

    // --- Issue #395: reconnect through a real two-hop SSH chain -------------

    use super::two_hop_fixture::{RunningTestServer, TEST_PASSWORD, TEST_USERNAME};

    const HOP_PASSWORD_REF: &str = "test:hop-password";

    /// Trusts every host key: the host key dialog is not under test here.
    struct TrustAll;
    impl HostKeyStore for TrustAll {
        fn check(&self, _host: &str, _port: u16, _key: &[u8]) -> HostKeyDecision {
            HostKeyDecision::Trusted
        }
        fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
            Ok(())
        }
    }

    /// TCP forwarder in front of the bastion. It counts the connections it
    /// accepted (what only a connection *through* the bastion can raise),
    /// can cut all live connections like a network drop, and can be stopped
    /// so that the bastion is unreachable.
    struct BastionProxy {
        port: u16,
        accepted: Arc<AtomicUsize>,
        connections: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
        accept_task: tokio::task::JoinHandle<()>,
    }

    impl BastionProxy {
        async fn start(target: std::net::SocketAddr) -> Self {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let port = listener.local_addr().unwrap().port();
            let accepted = Arc::new(AtomicUsize::new(0));
            let connections = Arc::new(Mutex::new(Vec::new()));
            let (count, conns) = (accepted.clone(), connections.clone());
            let accept_task = tokio::spawn(async move {
                while let Ok((mut inbound, _)) = listener.accept().await {
                    count.fetch_add(1, Ordering::SeqCst);
                    let handle = tokio::spawn(async move {
                        if let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await {
                            let _ =
                                tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                        }
                    });
                    conns.lock().unwrap().push(handle);
                }
            });
            Self {
                port,
                accepted,
                connections,
                accept_task,
            }
        }

        fn accepted(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }

        /// Cuts every live connection; new ones are still accepted.
        async fn sever_connections(&self) {
            let handles: Vec<_> = self.connections.lock().unwrap().drain(..).collect();
            for handle in handles {
                handle.abort();
                let _ = handle.await;
            }
        }

        /// Closes the listener: the bastion is unreachable from now on.
        async fn stop(&self) {
            self.accept_task.abort();
            // Wait until the aborted task has dropped the listener.
            for _ in 0..100 {
                if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            panic!("the bastion proxy still accepts connections");
        }
    }

    fn password_server(name: &str, port: u16, jump_host: Option<ServerId>) -> Server {
        let mut server = server();
        server.name = name.to_string();
        server.host = "127.0.0.1".to_string();
        server.port = port;
        server.username = TEST_USERNAME.to_string();
        server.auth = AuthMethod::Password {
            credential_ref: CredentialRef::new(HOP_PASSWORD_REF),
        };
        server.jump_host = jump_host;
        server
    }

    struct TwoHop {
        fx: Fixture,
        target_id: ServerId,
        proxy: BastionProxy,
        // Keep the in-process servers alive.
        _bastion: RunningTestServer,
        _target: RunningTestServer,
    }

    /// Real connector, bastion behind a [`BastionProxy`], target reachable
    /// only by the bastion's jump chain from the profile's point of view.
    async fn two_hop_fixture() -> TwoHop {
        let bastion = RunningTestServer::start().await;
        let target = RunningTestServer::start().await;
        let proxy = BastionProxy::start(bastion.addr).await;
        let mut fx = fixture(Arc::new(app_logic::test_connection::RealConnector)).await;
        fx.state.host_key_store = Arc::new(TrustAll);
        fx.state.credential_store = Arc::new(
            InMemoryCredentialStore::new()
                .with_secret(&CredentialRef::new(PROVIDER_SECRET_REF), "sk-test")
                .with_secret(&CredentialRef::new(HOP_PASSWORD_REF), TEST_PASSWORD),
        );
        let jump = password_server("bastion", proxy.port, None);
        let target_server = password_server("target", target.addr.port(), Some(jump.id));
        fx.state.profile_store.create_server(&jump).await.unwrap();
        fx.state
            .profile_store
            .create_server(&target_server)
            .await
            .unwrap();
        TwoHop {
            fx,
            target_id: target_server.id,
            proxy,
            _bastion: bastion,
            _target: target,
        }
    }

    /// Connects through both hops, then cuts the connection to the bastion
    /// and waits until the session has noticed (as the terminal actor
    /// would report it).
    async fn connect_and_sever(
        app: &tauri::App<tauri::test::MockRuntime>,
        hop: &TwoHop,
    ) -> (SessionId, Arc<Session>) {
        let session_id = uuid::Uuid::new_v4();
        connect_session(
            app.handle(),
            &hop.fx.state,
            hop.target_id,
            session_id,
            None,
            false,
        )
        .await
        .expect("two-hop connect should succeed");
        assert_eq!(hop.proxy.accepted(), 1, "first connect went via bastion");
        let session = hop.fx.state.sessions.get(session_id).unwrap();
        let out = session
            .transport
            .lock()
            .await
            .execute("before")
            .await
            .unwrap();
        assert_eq!(out.stdout, b"echo:before\n");

        hop.proxy.sever_connections().await;
        let mut gone = false;
        for _ in 0..100 {
            if session.transport.lock().await.execute("x").await.is_err() {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(gone, "the session should end after the cut");
        *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
        (session_id, session)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_reconnect_over_a_real_two_hop_chain_runs_a_command_via_the_bastion() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let hop = two_hop_fixture().await;
        let (session_id, session) = connect_and_sever(&app, &hop).await;
        let (elevated, grants) = registry_pair();

        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            reconnect_session_inner(
                app.handle(),
                &hop.fx.state,
                &elevated,
                &grants,
                session_id,
                true,
            ),
        )
        .await
        .expect("reconnect must not hang")
        .expect("reconnect through the bastion should succeed");

        assert_eq!(
            hop.proxy.accepted(),
            2,
            "the reconnect must open a new connection to the BASTION; a direct \
             connection to the target would leave this at 1"
        );
        assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);
        assert!(!session.transport.is_closed());
        let out = session
            .transport
            .lock()
            .await
            .execute("after-reconnect")
            .await
            .expect("a command runs on the new transport");
        assert_eq!(out.stdout, b"echo:after-reconnect\n");
        assert_eq!(out.exit_code, Some(0));
        reset(app.handle());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_reconnect_with_unreachable_bastion_fails_and_session_stays_dead() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let hop = two_hop_fixture().await;
        let (session_id, session) = connect_and_sever(&app, &hop).await;
        hop.proxy.stop().await;
        let (elevated, grants) = registry_pair();

        let err = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            reconnect_session_inner(
                app.handle(),
                &hop.fx.state,
                &elevated,
                &grants,
                session_id,
                true,
            ),
        )
        .await
        .expect("reconnect must not hang")
        .expect_err("reconnect must fail while the bastion is unreachable");

        assert!(err.code.is_some(), "stable error code expected: {err:?}");
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected,
            "the session stays dead and can be retried"
        );
        assert!(
            session.transport.is_closed(),
            "no half-swapped transport: the dead one stays in place, closed"
        );
        assert!(
            session
                .transport
                .lock()
                .await
                .execute("nope")
                .await
                .is_err(),
            "no command may run on a failed reconnect"
        );
        assert_eq!(hop.proxy.accepted(), 1, "no connection was accepted");
        reset(app.handle());
    }

    // --- Issue #546: live connect progress and cancel -----------------------

    type StepScript = Box<dyn FnOnce(&ConnectLog) + Send>;
    type ScriptItem = (StepScript, Option<Result<ConnectOutcome, SshError>>);

    /// Writes the scripted steps into the attempt log, then returns the result
    /// (`None`: never returns, like a server that does not answer).
    struct StepConnector {
        queue: Mutex<std::collections::VecDeque<ScriptItem>>,
    }

    impl StepConnector {
        fn new(items: Vec<(StepScript, Option<Result<ConnectOutcome, SshError>>)>) -> Arc<Self> {
            Arc::new(Self {
                queue: Mutex::new(items.into()),
            })
        }
    }

    #[async_trait]
    impl Connector for StepConnector {
        async fn connect(
            &self,
            _target: &ConnectionTarget,
            _credentials: &(dyn CredentialStore + Send + Sync),
            _key_files: &(dyn KeyFileReader + Send + Sync),
            _host_keys: Arc<dyn HostKeyStore>,
            log: &ConnectLog,
            _limits: ssh_transport::ConnectLimits,
        ) -> Result<ConnectOutcome, SshError> {
            let (script, result) = self
                .queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("connector called too often");
            script(log);
            match result {
                Some(r) => r,
                None => std::future::pending().await,
            }
        }
    }

    #[derive(Default)]
    struct RecordingEmitter(Mutex<Vec<serde_json::Value>>);
    impl EventEmitter for RecordingEmitter {
        fn emit_event(&self, event: &str, payload: serde_json::Value) {
            if event == "connect-progress" {
                self.0.lock().unwrap().push(payload);
            }
        }
    }

    fn step(log: &ConnectLog, hop: &str, step: ssh_manager_core::ssh::ConnectStep, ok: bool) {
        let id = log.start(0, hop, step);
        if ok {
            log.succeed(id);
        }
    }

    fn tcp(log: &ConnectLog) {
        step(
            log,
            "root@example.invalid:22",
            ssh_manager_core::ssh::ConnectStep::TcpConnect {
                address: None,
                port: 22,
            },
            true,
        );
    }

    fn handshake_step() -> ssh_manager_core::ssh::ConnectStep {
        ssh_manager_core::ssh::ConnectStep::Handshake {
            server_version: None,
            kex: None,
            host_key_algorithm: None,
            cipher: None,
            mac: None,
        }
    }

    fn pending_unknown() -> ConnectOutcome {
        ConnectOutcome::PendingHostKeyConfirmation {
            host: "example.invalid".to_string(),
            port: 22,
            raw_key: vec![1, 2, 3],
            decision: HostKeyDecision::Unknown {
                fingerprint: "SHA256:x".to_string(),
            },
        }
    }

    fn host_key_unknown_steps(log: &ConnectLog) {
        tcp(log);
        let id = log.start(
            0,
            "root@example.invalid:22",
            ssh_manager_core::ssh::ConnectStep::HostKeyCheck {
                key_type: None,
                fingerprint: None,
                result: Some(ssh_manager_core::ssh::HostKeyCheckResult::Unknown),
            },
        );
        log.fail(id, ssh_manager_core::ssh::HOST_KEY_UNKNOWN);
    }

    async fn wait_until(mut cond: impl FnMut() -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !cond() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("condition not reached in time");
    }

    #[tokio::test]
    async fn test_progress_events_carry_attempt_id_and_end_with_all_steps_ok() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let disconnected = Arc::new(AtomicBool::new(false));
        let script: StepScript = Box::new(|log| {
            tcp(log);
            step(log, "root@example.invalid:22", handshake_step(), true);
            step(
                log,
                "root@example.invalid:22",
                ssh_manager_core::ssh::ConnectStep::SessionReady,
                true,
            );
        });
        let connector = StepConnector::new(vec![(script, Some(connected(&disconnected)))]);
        let fx = fixture(connector).await;
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "attempt-1".into(), emitter.clone());

        connect_session_with_progress(
            app.handle(),
            &fx.state,
            fx.server_id,
            uuid::Uuid::new_v4(),
            None,
            true,
            Some(&progress),
        )
        .await
        .unwrap();

        let events = emitter.0.lock().unwrap();
        assert!(!events.is_empty());
        assert!(events.iter().all(|e| e["attemptId"] == "attempt-1"
            && e["attempt"] == 1
            && e["serverId"] == serde_json::json!(fx.server_id)));
        let last = events.last().unwrap()["steps"].as_array().unwrap().clone();
        assert_eq!(last.last().unwrap()["kind"], "sessionReady");
        assert!(last.iter().all(|s| s["status"] == "ok"));
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_auth_failure_emits_failed_auth_step_and_keeps_the_error_log() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let script: StepScript = Box::new(|log| {
            tcp(log);
            step(
                log,
                "root@example.invalid:22",
                ssh_manager_core::ssh::ConnectStep::Authentication {
                    method: ssh_manager_core::ssh::AuthMethodKind::IdentityFile,
                    remaining_methods: vec![],
                    partial_success: false,
                },
                false,
            );
            log.fail_running("SSH_AUTH_FAILED");
        });
        let connector =
            StepConnector::new(vec![(script, Some(Err(SshError::AuthenticationFailed)))]);
        let fx = fixture(connector).await;
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "a".into(), emitter.clone());

        let err = connect_session_with_progress(
            app.handle(),
            &fx.state,
            fx.server_id,
            uuid::Uuid::new_v4(),
            None,
            true,
            Some(&progress),
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, Some("SSH_AUTH_FAILED"));
        assert!(err.connect_log.is_some());
        let events = emitter.0.lock().unwrap();
        let last = events.last().unwrap()["steps"].as_array().unwrap().clone();
        let auth = last.last().unwrap();
        assert_eq!(auth["kind"], "authentication");
        assert_eq!(auth["status"], "failed");
        assert_eq!(auth["errorCode"], "SSH_AUTH_FAILED");
        assert_eq!(auth["detail"]["method"], "publickey (key file)");
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_handshake_timeout_emits_failed_step_with_timeout_code() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let script: StepScript = Box::new(|log| {
            step(log, "root@example.invalid:22", handshake_step(), false);
            log.fail_running("SSH_TIMEOUT");
        });
        let connector = StepConnector::new(vec![(script, Some(Err(SshError::Timeout)))]);
        let fx = fixture(connector).await;
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "a".into(), emitter.clone());

        let err = connect_session_with_progress(
            app.handle(),
            &fx.state,
            fx.server_id,
            uuid::Uuid::new_v4(),
            None,
            true,
            Some(&progress),
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, Some("SSH_TIMEOUT"));
        let events = emitter.0.lock().unwrap();
        let first = events.first().unwrap()["steps"][0].clone();
        assert_eq!(first["status"], "running");
        assert_eq!(first["detail"]["code"], "waitingForResponse");
        let last = events.last().unwrap()["steps"][0].clone();
        assert_eq!(last["status"], "failed");
        assert_eq!(last["errorCode"], "SSH_TIMEOUT");
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_host_key_prompt_waits_for_decision_and_retry_is_attempt_two() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let disconnected = Arc::new(AtomicBool::new(false));
        let second: StepScript = Box::new(|log| {
            tcp(log);
        });
        let connector = StepConnector::new(vec![
            (
                Box::new(host_key_unknown_steps),
                Some(Ok(pending_unknown())),
            ),
            (second, Some(connected(&disconnected))),
        ]);
        let fx = fixture(connector).await;
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "a".into(), emitter.clone());
        let tab = uuid::Uuid::new_v4();

        let decide = async {
            wait_until(|| {
                emitter.0.lock().unwrap().iter().any(|e| {
                    e["steps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|s| s["detail"]["code"] == "waitingForDecision")
                })
            })
            .await;
            let waiting = emitter
                .0
                .lock()
                .unwrap()
                .iter()
                .find_map(|e| {
                    e["steps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|s| s["detail"]["code"] == "waitingForDecision")
                        .cloned()
                })
                .unwrap();
            assert_eq!(waiting["status"], "running");
            assert_eq!(waiting["kind"], "hostKeyCheck");
            wait_until(|| {
                fx.state
                    .pending_host_key_confirmations
                    .resolve(&tab, HostKeyUserDecision::Trust)
                    .is_ok()
            })
            .await;
        };
        let (result, ()) = tokio::join!(
            connect_session_with_progress(
                app.handle(),
                &fx.state,
                fx.server_id,
                tab,
                None,
                true,
                Some(&progress)
            ),
            decide
        );
        result.unwrap();

        let events = emitter.0.lock().unwrap();
        assert_eq!(events.last().unwrap()["attempt"], 2);
        assert_eq!(events.last().unwrap()["steps"].as_array().unwrap().len(), 1);
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_cancel_during_handshake_ends_connect_without_session() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let script: StepScript = Box::new(|log| {
            step(log, "root@example.invalid:22", handshake_step(), false);
        });
        let connector = StepConnector::new(vec![(script, None)]);
        let fx = fixture(connector).await;
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "cancel-me".into(), emitter.clone());
        let tab = uuid::Uuid::new_v4();

        let cancel = async {
            wait_until(|| !emitter.0.lock().unwrap().is_empty()).await;
            fx.state.connect_attempts.cancel("cancel-me");
        };
        let (result, ()) = tokio::join!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                connect_session_with_progress(
                    app.handle(),
                    &fx.state,
                    fx.server_id,
                    tab,
                    None,
                    true,
                    Some(&progress)
                )
            ),
            cancel
        );

        let err = result
            .expect("cancel must end connect within 1 s")
            .unwrap_err();
        assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
        assert!(fx.state.sessions.get(tab).is_none());
        assert!(fx
            .state
            .chat_session_store
            .list_sessions_for_server(&fx.server_id)
            .await
            .unwrap()
            .is_empty());
        let events = emitter.0.lock().unwrap();
        let last = events.last().unwrap()["steps"][0].clone();
        assert_eq!(last["status"], "failed");
        assert_eq!(last["errorCode"], "SSH_CONNECT_CANCELLED");
        reset(app.handle());
    }

    /// Issue #568: a resume carries an attempt id like a plain connect —
    /// progress events use it, `cancel_connect` aborts it, and the saved chat
    /// session stays untouched.
    #[tokio::test]
    async fn test_resume_emits_progress_and_cancel_leaves_saved_session_unchanged() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let script: StepScript = Box::new(|log| {
            step(log, "root@example.invalid:22", handshake_step(), false);
        });
        let connector = StepConnector::new(vec![(script, None)]);
        let fx = fixture(connector).await;
        let store = &fx.state.chat_session_store;
        let existing = store.create_session(&fx.server_id, None).await.unwrap();
        store.mark_ended(existing).await.unwrap();
        let before = store.list_sessions_for_server(&fx.server_id).await.unwrap();
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "resume-1".into(), emitter.clone());
        let tab = uuid::Uuid::new_v4();

        let cancel = async {
            wait_until(|| !emitter.0.lock().unwrap().is_empty()).await;
            fx.state.connect_attempts.cancel("resume-1");
        };
        let (result, ()) = tokio::join!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                connect_session_with_progress(
                    app.handle(),
                    &fx.state,
                    fx.server_id,
                    tab,
                    Some(existing),
                    true,
                    Some(&progress)
                )
            ),
            cancel
        );

        let err = result
            .expect("cancel must end the resume within 1 s")
            .unwrap_err();
        assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
        assert!(fx.state.sessions.get(tab).is_none());
        assert!(emitter
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|e| e["attemptId"] == "resume-1"));
        let after = store.list_sessions_for_server(&fx.server_id).await.unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(after[0].ended_at, before[0].ended_at);
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_cancel_during_host_key_wait_releases_registration_and_ends_prompt() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let connector = StepConnector::new(vec![(
            Box::new(host_key_unknown_steps),
            Some(Ok(pending_unknown())),
        )]);
        let fx = fixture(connector).await;
        let ended = Arc::new(Mutex::new(Vec::new()));
        let sink = ended.clone();
        app.handle()
            .listen("host-key-verification-ended", move |event| {
                sink.lock().unwrap().push(event.payload().to_string());
            });
        let emitter = Arc::new(RecordingEmitter::default());
        let progress = ConnectProgress::new(&fx.state, "hk".into(), emitter.clone());
        let tab = uuid::Uuid::new_v4();

        let cancel = async {
            wait_until(|| {
                emitter.0.lock().unwrap().iter().any(|e| {
                    e["steps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|s| s["detail"]["code"] == "waitingForDecision")
                })
            })
            .await;
            fx.state.connect_attempts.cancel("hk");
        };
        let (result, ()) = tokio::join!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                connect_session_with_progress(
                    app.handle(),
                    &fx.state,
                    fx.server_id,
                    tab,
                    None,
                    true,
                    Some(&progress)
                )
            ),
            cancel
        );

        let err = result
            .expect("cancel must end connect within 1 s")
            .unwrap_err();
        assert_eq!(err.code, Some("SSH_CONNECT_CANCELLED"));
        assert!(fx.state.sessions.get(tab).is_none());
        assert!(
            fx.state
                .pending_host_key_confirmations
                .resolve(&tab, HostKeyUserDecision::Trust)
                .is_err(),
            "the host key registration was released"
        );
        assert_eq!(ended.lock().unwrap().len(), 1, "prompt end event emitted");
        assert!(fx
            .state
            .chat_session_store
            .list_sessions_for_server(&fx.server_id)
            .await
            .unwrap()
            .is_empty());
        reset(app.handle());
    }

    #[tokio::test]
    async fn test_cancel_of_unknown_attempt_is_a_noop() {
        let _guard = lock_async().await;
        let app = acknowledged_app();
        let connector = StepConnector::new(vec![]);
        let fx = fixture(connector).await;
        assert!(!fx.state.connect_attempts.cancel("does-not-exist"));
        reset(app.handle());
    }
}
