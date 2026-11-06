//! Opening the transport of a session (host key flow, jump hosts, credentials).

use super::*;

/// Opens the SSH connection of a session, including jump hosts, credential
/// resolution and the host key dialog (ADR 0007). Shared by the first connect
/// and by `reconnect_session` (Issue #355), so a reconnect is verified exactly
/// like a first connect: no cached "accepted" decision, a changed key shows
/// the dialog again.
#[expect(clippy::cognitive_complexity, reason = "split planned")]
pub(super) async fn establish_transport<R: tauri::Runtime>(
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
                ssh_transport::timeout_unless_paused(
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
                    // Issue #1187: stands still while a login prompt is open.
                    &attempt_log.prompt_pause(),
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
