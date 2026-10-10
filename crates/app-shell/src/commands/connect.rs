//! Spec 0007/0017/0031/0034/0068/0069: Verbindungsaufbau (inkl. Host-Key-
//! Bestätigung), persistente Chat-Sitzungen — Teil der Spec-0083-Aufteilung
//! von `commands.rs`.

use std::sync::Arc;

use tauri::{AppHandle, State};

use ssh_manager_core::ai::{default_action_schemas, ChatMessage, OutputRedactor, SessionContext};
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
    emit_connection_status_changed, emit_host_key_verification_ended,
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
) -> CommandResult<SessionId> {
    let session_id: SessionId = uuid::Uuid::new_v4();
    connect_session(&app, &state, server_id, session_id, None, true).await
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
/// im `if`), damit die eigentliche Entscheidung — die Spec-0040-Abschnitt-
/// 4-Anforderung "MCP-ausgelöste Aktionen erzeugen keine `chat_sessions`-
/// Zeile" — ohne einen echten SSH-Verbindungsaufbau testbar ist:
/// `connect_session` selbst lässt sich für einen Nicht-lokalen Server
/// nicht sinnvoll unit-testen (`ssh_transport::connect` ist dort fest
/// verdrahtet, nicht injizierbar — dieselbe Grenze wie beim eigentlichen
/// Verbindungsaufbau überall sonst in diesem Modul).
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
/// `connect_session` als Ganzes, s. Doc-Kommentar oben) isoliert testbar
/// ist, ohne einen echten SSH-Verbindungsaufbau zu brauchen.
/// Spec 0101, A9.1: Der Code kommt vollständig aus [`SshError::code`] —
/// der Zustand des Schlüsselbunds spielt für ihn keine Rolle mehr (seit A9
/// liegen die Secrets in der Datenbank, s.
/// `app_logic::error::ssh_command_error`).
fn map_connect_result(
    result: Result<ssh_transport::ConnectOutcome, SshError>,
) -> CommandResult<ssh_transport::ConnectOutcome> {
    result.map_err(|err| ssh_command_error(&err))
}

pub(crate) async fn connect_session(
    app: &AppHandle,
    state: &AppState,
    server_id: ServerId,
    session_id: SessionId,
    resume: Option<uuid::Uuid>,
    persist_chat_session: bool,
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
    let mut transport: Box<dyn ssh_manager_core::ssh::SshTransport> = if is_local {
        Box::new(ssh_transport::LocalTransport::new())
    } else {
        let target = match resolve_connection_target(&server, state.profile_store.as_ref()).await {
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
        loop {
            // Issue #51: je Versuch ein frisches Schritt-Protokoll. Es geht
            // nur ans Frontend (angehängt an den `CommandError`), nie ins
            // Log — die `tracing`-Aufrufe unten nennen weiter nur Code und
            // Meldung.
            let attempt_log = ConnectLog::new();
            // Spec 0069, Teil A3 (Issue #97): jeder Verbindungsversuch
            // (auch nach `Trust` erneut, s. Schleife) läuft unter den
            // Phasengrenzen `SSH_CONNECT_LIMITS` — je Hop eine für
            // Verbindung/Handshake, eine für die Anmeldung. Außen herum
            // liegt nur ein Sicherheitsnetz, das mit der Hop-Zahl wächst
            // und im normalen Ablauf nicht erreichbar ist. Beides
            // umschließt bewusst NUR diesen Aufruf, nicht das Warten auf
            // eine Host-Key-Entscheidung weiter unten.
            let attempt = ssh_transport::connect_with_timeout(
                ssh_transport::connect_with_limits(
                    &target,
                    state.credential_store.as_ref(),
                    // Spec 0076, §4.2: der echte Produktionspfad —
                    // dieser Aufruf geht direkt an `ssh_transport`,
                    // nicht über den `Connector`-Trait (das ist die
                    // Testabstraktion daneben).
                    state.key_file_reader.as_ref(),
                    state.host_key_store.clone(),
                    &attempt_log,
                    SSH_CONNECT_LIMITS,
                ),
                SSH_CONNECT_LIMITS.overall(target.hops.len()),
            )
            .await;
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

                    let wait = wait_for_host_key_decision(
                        &state.pending_host_key_confirmations,
                        session_id,
                        generation,
                        rx,
                        app_logic::orchestration::PENDING_ACTION_CONFIRM_TIMEOUT,
                    )
                    .await;
                    state.sessions.clear_pending_connection(session_id);
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
                    .map_err(|err| err.with_connect_log(attempt_log.snapshot()))?;
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
    };

    let sanitized_os = if let Ok(uname_output) = transport.execute("uname -a").await {
        let uname_text = String::from_utf8_lossy(&uname_output.stdout);
        app_logic::orchestration::sanitize_uname_output(&uname_text)
    } else {
        None
    };

    let (system_context_parts, notes_present) = build_session_system_context(
        app,
        &server.name,
        &server_id,
        &server.tags,
        state.profile_store.as_ref(),
        &state.policy_store,
    )
    .await;
    let system_context = system_context_parts.assemble();

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
    let redactor: Box<dyn OutputRedactor> =
        app_logic::server_redaction::redactor_with_sudo_password(sudo_password.as_ref());

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

    let session = Arc::new(
        Session::new(SessionParts {
            transport: app_logic::session::SessionTransport::new(transport),
            ai_provider,
            ai_provider_budget,
            context: tokio::sync::Mutex::new(SessionContext {
                system_context,
                history: initial_history,
                available_actions: default_action_schemas(),
                max_tokens_hint: None,
            }),
            filter_engine: Box::new(FilterEngine::new(state.policy_store.clone())),
            server_id,
            tags: server.tags,
            terminal: std::sync::Mutex::new(None),
            redactor,
            ai_provider_label: active_config.display_name,
            ai_model: active_config.model,
            system_context_parts: tokio::sync::Mutex::new(system_context_parts),
            model_context_window_tokens,
            summary: tokio::sync::Mutex::new(initial_summary),
            mcp_origin_flags: std::sync::Mutex::new(initial_mcp_origin_flags),
            sudo_password,
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
        })
        .with_start_directory(start_directory),
    );
    state.sessions.insert(session_id, session);

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
) -> CommandResult<SessionId> {
    let tab_session_id: SessionId = uuid::Uuid::new_v4();
    connect_session(
        &app,
        &state,
        server_id,
        tab_session_id,
        Some(session_id),
        true,
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

    /// Spec 0031, Abschnitt 6: "`connect()` schlägt fehl/wird blockiert,
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

    /// Spec 0031, Abschnitt 6: "Bestätigung setzt die Einstellung korrekt
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
