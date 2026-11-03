//! Spec 0007/0017/0031/0034/0068/0069: Verbindungsaufbau (inkl. Host-Key-
//! Bestätigung), persistente Chat-Sitzungen — Teil der Spec-0083-Aufteilung
//! von `commands.rs`.

mod progress;
mod system_context;

pub(super) use system_context::build_session_system_context;

pub(crate) use progress::ConnectProgress;
use progress::{cancelled_error, or_cancelled};

use super::scope::extension_scope_lookup;
use std::sync::Arc;

use tauri::{AppHandle, State};

use ssh_manager_core::ai::{ChatMessage, OutputRedactor, SessionContext};
use ssh_manager_core::filter::FilterEngine;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{resolve_connection_target, ConnectLog, SshError};

use app_logic::ai_provider_factory::build_ai_provider_from_config;
use app_logic::dto::HostKeyUserDecision;
use app_logic::error::{secret_store_error, ssh_command_error, CommandError, CommandResult};
use app_logic::events::{emit_connection_status_changed, ConnectionStatus};
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
                    // Issue #835: the whole host key decision (prompt, wait,
                    // trust) lives in `app_logic::connect::host_key_flow`.
                    let emitter = crate::event_emitter::TauriEventEmitter(app.clone());
                    let report = |steps| {
                        if let Some(p) = progress {
                            p.emit(server_id, attempt_no, steps);
                        }
                    };
                    let cancel = async {
                        match progress {
                            Some(p) => p.cancelled().await,
                            None => std::future::pending().await,
                        }
                    };
                    app_logic::connect::host_key_flow::resolve_pending_host_key(
                        app_logic::connect::host_key_flow::HostKeyFlowContext {
                            session_id,
                            server_id,
                            host_key_store: state.host_key_store.as_ref(),
                            confirmations: &state.pending_host_key_confirmations,
                            sessions: &state.sessions,
                            emitter: &emitter,
                            attempt_log: &attempt_log,
                            progress: &report,
                            interactive,
                            timeout: app_logic::orchestration::PENDING_ACTION_CONFIRM_TIMEOUT,
                        },
                        host,
                        port,
                        raw_key,
                        decision,
                        cancel,
                    )
                    .await?;
                    // Retry with the same `target` after Trust (ADR 0007).
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
    let sudo = app_logic::server_redaction::read_sudo_password_checked(
        state.credential_store.as_ref(),
        server_id,
    );
    let sudo_password = sudo.0.clone();

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
    let scope_lookup = extension_scope_lookup(app, state.profile_store.as_ref(), server_id).await;
    let extension_scope = scope_lookup.scope.clone();
    let extension_secrets = app_logic::session_extension::known_secrets(
        state.session_extension.as_deref(),
        extension_scope.as_ref(),
    );
    let (redactor, redaction_incomplete) = app_logic::session_extension::redactor_for_connect(
        &sudo,
        &scope_lookup,
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
    let (risk_second_opinion_provider, risk_second_opinion_budget, second_opinion_source) =
        match second_opinion_resolved {
            Some((provider, budget, source)) => (Some(provider), Some(budget), Some(source)),
            None => (None, None, None),
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
    let (
        injection_check_provider,
        injection_check_budget,
        injection_check_inactive,
        injection_check_source,
    ) = if server.ai_injection_check_enabled {
        match crate::risk_second_opinion::resolve_second_opinion_provider(app, state)
            .await
            .into_parts()
            .0
        {
            Some((provider, budget, source)) => (Some(provider), Some(budget), false, Some(source)),
            None => (None, None, true, None),
        }
    } else {
        (None, None, false, None)
    };
    // Issue #610: the side calls share one configured provider.
    let side_call_source = second_opinion_source.or(injection_check_source);
    let ai_usage_reporting = app_logic::ai_usage::AiUsageReporting {
        observers: state.ai_usage_observers.clone(),
        runtime_session_id: session_id.to_string(),
        main: app_logic::ai_usage::AiUsageSource {
            provider_config_id: active_config.id.0.to_string(),
            provider_type: active_config.provider_type,
            configured_model: active_config.model.clone(),
        },
        side_call: side_call_source,
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
            // eine unbedingte Rundenkürzung
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
            context_usage: Default::default(),
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
            injection_suspected_reason: std::sync::Mutex::new(None),
            injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
            second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(
                second_opinion_setup_failed,
            ),
            injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(
                injection_check_inactive,
            ),
            redaction_incomplete_notice_pending: std::sync::atomic::AtomicBool::new(
                redaction_incomplete,
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
        .with_is_root(is_root)
        .with_usage_reporting(Some(ai_usage_reporting)),
    );
    // Byte-wise copy: stays masked if a later lookup fails (issue #843).
    session.redactor.add_secret_values(&extension_secrets);
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
        let sudo = app_logic::server_redaction::read_sudo_password_checked(
            state.credential_store.as_ref(),
            server_id,
        );
        let lookup = extension_scope_lookup(app, state.profile_store.as_ref(), server_id).await;
        let extension_secrets = app_logic::session_extension::known_secrets(
            state.session_extension.as_deref(),
            lookup.scope.as_ref(),
        );
        let redactor = app_logic::session_extension::redactor_for_reconnect(
            &session,
            &sudo,
            &lookup,
            &extension_secrets,
        );
        let sudo_password = sudo.0;
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
                );
            }
            tracing::info!(session_id = %session_id, "session reconnected");
            emit_connection_status_changed(&emitter, session_id, ConnectionStatus::Connected, None);
            let current = *session.chat_session_id.lock().await;
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
