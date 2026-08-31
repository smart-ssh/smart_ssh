//! Tauri-Commands (Spec 0007, Abschnitt 4).

use std::sync::Arc;

use secrecy::SecretString;
use tauri::{AppHandle, State};
use tokio::sync::mpsc;

use persistence_sqlite::AiProviderConfig;
use ssh_manager_core::ai::{
    default_action_schemas, ChatMessage, DefaultOutputRedactor, MessageContent, ProviderId, Role,
    SessionContext,
};
use ssh_manager_core::filter::FilterEngine;
use ssh_manager_core::profiles::effective_notes;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{resolve_connection_target, HostKeyDecision, PtySize};

use crate::ai_provider_factory::build_ai_provider;
use crate::dto::{
    credential_ref_for, ActionUserDecision, AiProviderConfigDto, AiProviderConfigInput,
    HostKeyUserDecision, ServerDto,
};
use crate::error::CommandResult;
use crate::events::{
    emit_connection_status_changed, emit_host_key_verification_needed, ConnectionStatus,
    EventEmitter, HostKeyKind,
};
use crate::orchestration::run_chat_turn;
use crate::policy::NoRulesPolicyStore;
use crate::session::{spawn_terminal_actor, Session, TerminalCommand};
use crate::state::{ActionId, AppState, SessionId};

#[tauri::command]
pub async fn list_servers(state: State<'_, AppState>) -> CommandResult<Vec<ServerDto>> {
    let servers = state.profile_store.list_servers().await?;
    Ok(servers.iter().map(ServerDto::from).collect())
}

#[tauri::command]
pub async fn list_ai_providers(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AiProviderConfigDto>> {
    let configs = state.ai_provider_store.list().await?;
    Ok(configs.iter().map(AiProviderConfigDto::from).collect())
}

/// Spec 0007, Abschnitt 8.2: Backend generiert eine neue `ProviderId`,
/// speichert `api_key` zuerst über den `CredentialStore`, danach erst die
/// restlichen Felder in `ai_provider_configs`.
#[tauri::command]
pub async fn add_ai_provider(
    state: State<'_, AppState>,
    config: AiProviderConfigInput,
) -> CommandResult<ProviderId> {
    let id = ProviderId::new();
    let credential_ref = credential_ref_for(id);
    state
        .credential_store
        .set(&credential_ref, SecretString::from(config.api_key.clone()))?;

    let new_config = config.into_new_config(id);
    if let Err(err) = state.ai_provider_store.create(&new_config).await {
        // Best-effort-Aufräumen: ohne diesen Rückbau bliebe bei einem
        // DB-Fehler ein verwaister Credential-Eintrag im Keychain zurück,
        // auf den keine `ai_provider_configs`-Zeile mehr verweist. Ein
        // Fehler beim Aufräumen selbst wird bewusst verschluckt (nicht per
        // `?` weitergereicht) — der eigentliche Fehler (`err`) ist die
        // relevante Information für den Aufrufer, ein sekundärer
        // Keychain-Fehler beim Aufräumversuch soll ihn nicht überdecken.
        let _ = state.credential_store.delete(&credential_ref);
        return Err(err.into());
    }
    Ok(id)
}

/// Spec 0007, Abschnitt 8.2: leeres `api_key`-Feld heißt "Credential
/// unverändert lassen", nicht "löschen". Reihenfolge bewusst umgekehrt zu
/// `add_ai_provider`/`delete_ai_provider`: erst die DB-Metadaten
/// aktualisieren (schlägt sauber mit `NotFound` fehl, falls `id` nicht
/// existiert), erst danach — nur bei nicht-leerem `api_key` — den
/// Credential überschreiben. So wird nie ein Secret für eine `id`
/// geschrieben, die sich als ungültig herausstellt.
#[tauri::command]
pub async fn update_ai_provider(
    state: State<'_, AppState>,
    id: ProviderId,
    config: AiProviderConfigInput,
) -> CommandResult<()> {
    let api_key = config.api_key.clone();
    state
        .ai_provider_store
        .update_fields(&config.into_update(id))
        .await?;

    if !api_key.is_empty() {
        state
            .credential_store
            .set(&credential_ref_for(id), SecretString::from(api_key))?;
    }
    Ok(())
}

/// Spec 0007, Abschnitt 8.2/9: erst `CredentialStore::delete()`, dann die
/// DB-Zeile — aber erst, nachdem geprüft wurde, dass der Provider nicht
/// aktiv ist (Abschnitt 9: Löschen eines aktiven Providers ist verboten).
/// Würde man `is_active` nicht **vor** dem Credential-Löschen prüfen, könnte
/// ein verbotener Löschversuch trotzdem den Credential eines weiterhin
/// aktiven, in der DB unverändert bleibenden Providers entfernen.
#[tauri::command]
pub async fn delete_ai_provider(state: State<'_, AppState>, id: ProviderId) -> CommandResult<()> {
    let existing = state.ai_provider_store.get(&id).await?;
    if existing.is_active {
        return Err(
            persistence_sqlite::AiProviderStoreError::ActiveProviderDeletionForbidden(id).into(),
        );
    }

    state.credential_store.delete(&existing.credential_ref)?;
    state.ai_provider_store.delete(&id).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_active_ai_provider(
    state: State<'_, AppState>,
    id: ProviderId,
) -> CommandResult<()> {
    state
        .ai_provider_store
        .set_active(&id, chrono::Utc::now())
        .await?;
    Ok(())
}

/// Kein eigener `get_active`-Query in `SqliteAiProviderStore` (s. dortige
/// API) — bei der zu erwartenden geringen Providerzahl reicht ein Filter
/// über `list()`, ein zusätzlicher SQL-Pfad nur für diesen einen Aufrufer
/// wäre unnötige API-Fläche.
async fn active_ai_provider_config(state: &AppState) -> CommandResult<AiProviderConfig> {
    state
        .ai_provider_store
        .list()
        .await?
        .into_iter()
        .find(|c| c.is_active)
        .ok_or_else(|| {
            "kein aktiver AI-Provider konfiguriert — bitte zuerst in den Einstellungen einrichten"
                .into()
        })
}

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

    let server = state.profile_store.get_server(&server_id).await?;
    let target = resolve_connection_target(&server, state.profile_store.as_ref()).await?;
    let active_config = active_ai_provider_config(&state).await?;
    let api_key = state.credential_store.get(&active_config.credential_ref)?;
    let ai_provider = build_ai_provider(
        active_config.provider_type,
        active_config.base_url.as_deref(),
        &active_config.model,
        api_key,
        active_config.supports_native_tool_calling,
    );

    let mut transport = loop {
        let outcome = ssh_transport::connect(
            &target,
            state.credential_store.as_ref(),
            state.host_key_store.clone(),
        )
        .await?;

        match outcome {
            ssh_transport::ConnectOutcome::Connected(transport) => break transport,
            ssh_transport::ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                decision,
            } => {
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

                let rx = state.pending_host_key_confirmations.register(session_id);
                emit_host_key_verification_needed(
                    &app,
                    session_id,
                    host.clone(),
                    port,
                    kind,
                    fingerprint,
                    expected_fingerprint,
                );

                let Ok(user_decision) = rx.await else {
                    return Err("Verbindungsaufbau abgebrochen".into());
                };
                match user_decision {
                    HostKeyUserDecision::Trust => {
                        state.host_key_store.trust(&host, port, &raw_key)?;
                        // Erneuter Versuch mit demselben `target` — s.
                        // Doc-Kommentar oben.
                    }
                    HostKeyUserDecision::Reject => {
                        return Err(format!(
                            "Verbindung zu {host}:{port} abgelehnt (Host-Key nicht vertraut)"
                        )
                        .into());
                    }
                }
            }
        }
    };

    // Bestmöglicher, nicht-fataler Versuch, den `system_context` um
    // Remote-OS-Info zu ergänzen (Spec 0006, `SessionContext.system_context`-
    // Doc: "effective_notes() + OS/Distro-Info"). Schlägt `uname` fehl (z. B.
    // ein Windows-Zielsystem ohne POSIX-Tools), bleibt der Kontext einfach
    // ohne diesen Abschnitt — kein Grund, `connect()` deswegen abzubrechen.
    let mut system_context = effective_notes(&server, state.profile_store.as_ref()).await?;
    if let Ok(uname_output) = transport.execute("uname -a").await {
        let uname_text = String::from_utf8_lossy(&uname_output.stdout);
        if !uname_text.trim().is_empty() {
            system_context.push_str(&format!("\n\n## Remote-System\n{}", uname_text.trim()));
        }
    }

    let session = Arc::new(Session {
        transport: tokio::sync::Mutex::new(transport),
        ai_provider,
        context: tokio::sync::Mutex::new(SessionContext {
            system_context,
            history: Vec::new(),
            available_actions: default_action_schemas(),
        }),
        filter_engine: Box::new(FilterEngine::new(NoRulesPolicyStore)),
        server_id,
        tags: server.tags,
        terminal: std::sync::Mutex::new(None),
        redactor: Box::new(DefaultOutputRedactor::new()),
        ai_provider_label: active_config.display_name,
        ai_model: active_config.model,
    });
    state.sessions.insert(session_id, session);

    emit_connection_status_changed(&app, session_id, ConnectionStatus::Connected, None);
    Ok(session_id)
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

#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;

    let shell = {
        let mut transport = session.transport.lock().await;
        // Standardgröße, bis das Frontend die tatsächliche Terminal-Größe
        // per `terminal_resize` meldet (Spec 0007 Abschnitt 4 sieht für
        // `open_terminal` selbst keinen Größen-Parameter vor).
        transport.open_shell(PtySize { cols: 80, rows: 24 }).await?
    };

    let (tx, rx) = mpsc::unbounded_channel();
    *session.terminal.lock().unwrap() = Some(tx);
    spawn_terminal_actor(
        session_id,
        shell,
        rx,
        Arc::new(app) as Arc<dyn EventEmitter>,
    );
    Ok(())
}

fn terminal_sender(session: &Session) -> CommandResult<mpsc::UnboundedSender<TerminalCommand>> {
    session
        .terminal
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Terminal wurde noch nicht geöffnet (open_terminal aufrufen)".into())
}

#[tauri::command]
pub async fn terminal_input(
    state: State<'_, AppState>,
    session_id: SessionId,
    data: Vec<u8>,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    terminal_sender(&session)?
        .send(TerminalCommand::Write(data))
        .map_err(|_| "Terminal-Kanal bereits geschlossen")?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    session_id: SessionId,
    cols: u16,
    rows: u16,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    terminal_sender(&session)?
        .send(TerminalCommand::Resize(PtySize { cols, rows }))
        .map_err(|_| "Terminal-Kanal bereits geschlossen")?;
    Ok(())
}

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

    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text(text),
    });

    run_chat_turn(
        &session,
        session_id,
        &app,
        state.profile_store.as_ref(),
        &state.pending_action_confirmations,
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn respond_to_action(
    state: State<'_, AppState>,
    session_id: SessionId,
    action_id: ActionId,
    decision: ActionUserDecision,
) -> CommandResult<()> {
    if state.sessions.get(session_id).is_none() {
        return Err("Session nicht gefunden".into());
    }
    state
        .pending_action_confirmations
        .resolve(&action_id, decision)?;
    Ok(())
}

#[tauri::command]
pub async fn disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    let session = state
        .sessions
        .remove(session_id)
        .ok_or("Session nicht gefunden")?;

    // Best-effort: ein Fehler beim Trennen selbst (z. B. Verbindung bereits
    // tot) soll `disconnect()` nicht scheitern lassen — die Session wird in
    // jedem Fall aus `state.sessions` entfernt.
    let _ = session.transport.lock().await.disconnect().await;
    // Droppt den Sender -> der Terminal-Aktor (falls einer läuft) beendet
    // sich selbst beim nächsten `commands.recv()` (s. dortiger Kommentar),
    // ohne hier ein zweites `connection-status-changed`-Event auszulösen.
    *session.terminal.lock().unwrap() = None;

    emit_connection_status_changed(&app, session_id, ConnectionStatus::Disconnected, None);
    Ok(())
}
