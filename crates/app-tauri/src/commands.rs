//! Tauri-Commands (Spec 0007, Abschnitt 4).

use std::sync::Arc;

use secrecy::SecretString;
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;

use chrono::Utc;
use uuid::Uuid;

use persistence_sqlite::AiProviderConfig;
use ssh_manager_core::ai::{
    default_action_schemas, ChatMessage, DefaultOutputRedactor, MessageContent, ProviderId, Role,
    SessionContext,
};
use ssh_manager_core::filter::{hard_blacklist_patterns, FilterEngine, RuleId, Scope};
use ssh_manager_core::profiles::{
    effective_notes, record_revision, Group, GroupId, NoteEditor, NoteTarget, Server,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{resolve_connection_target, HostKeyDecision, PtySize};

use crate::ai_provider_factory::build_ai_provider;
use crate::dto::{
    credential_ref_for, ActionUserDecision, AiProviderConfigDto, AiProviderConfigInput,
    DeleteGroupResult, DocumentFormat, EvalContextInput, EvaluationTraceDto, GroupDto,
    HostKeyUserDecision, NoteRevisionDto, PatternDto, PatternSuggestionDto, PatternType, RuleDto,
    RuleInput, ServerDto, ServerInput, TestConnectionResult,
};
use crate::error::CommandResult;
use crate::events::{
    emit_connection_status_changed, emit_host_key_verification_needed, ConnectionStatus,
    EventEmitter, HostKeyKind,
};
use crate::groups::{compute_delete_group_result, validate_no_cycle};
use crate::orchestration::run_chat_turn;
use crate::server_credentials::{delete_auth_method_secrets, resolve_auth_method};
use crate::session::{spawn_terminal_actor, Session, TerminalCommand};
use crate::state::{ActionId, AppState, SessionId};

/// `group_id` erweitert die Spec-0007-Signatur um den in Spec 0008
/// Abschnitt 4 vorgesehenen Filter (`None` = alle Server, wie bisher für
/// die einfache Liste aus Spec 0007 Teil 1 gebraucht).
#[tauri::command]
pub async fn list_servers(
    state: State<'_, AppState>,
    group_id: Option<GroupId>,
) -> CommandResult<Vec<ServerDto>> {
    let servers = state.profile_store.list_servers().await?;
    Ok(servers
        .iter()
        .filter(|s| group_id.is_none() || s.group_id == group_id)
        .map(ServerDto::from)
        .collect())
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
    // ein Windows-Zielsystem ohne POSIX-Tools) oder enthält die Ausgabe
    // ungültige/verdächtige Zeichen (Spec 0013, SEC-02), bleibt der Kontext
    // einfach ohne diesen Abschnitt.
    let mut system_context = effective_notes(&server, state.profile_store.as_ref()).await?;
    if let Ok(uname_output) = transport.execute("uname -a").await {
        let uname_text = String::from_utf8_lossy(&uname_output.stdout);
        if let Some(sanitized) = sanitize_uname_output(&uname_text) {
            system_context.push_str(&format!("\n\n## Remote-System\n{}", sanitized));
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
        filter_engine: Box::new(FilterEngine::new(state.policy_store.clone())),
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
        crate::orchestration::suggest_note_update_on_disconnect(
            &session,
            session_id,
            &app_for_suggestion,
            state.profile_store.as_ref(),
            &state.pending_action_confirmations,
        )
        .await;
    });

    Ok(())
}

// --- Spec 0008: Gruppen --------------------------------------------------

#[tauri::command]
pub async fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<GroupDto>> {
    let groups = state.profile_store.list_groups().await?;
    Ok(groups.iter().map(GroupDto::from).collect())
}

#[tauri::command]
pub async fn create_group(
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<GroupId>,
) -> CommandResult<GroupId> {
    validate_no_cycle(state.profile_store.as_ref(), None, parent_id).await?;

    let now = Utc::now();
    let group = Group {
        id: GroupId::new(),
        name,
        parent_id,
        notes: String::new(),
        created_at: now,
        updated_at: now,
    };
    state.profile_store.create_group(&group).await?;
    Ok(group.id)
}

#[tauri::command]
pub async fn update_group(
    state: State<'_, AppState>,
    id: GroupId,
    name: String,
    parent_id: Option<GroupId>,
) -> CommandResult<()> {
    validate_no_cycle(state.profile_store.as_ref(), Some(id), parent_id).await?;

    let mut group = state.profile_store.get_group(&id).await?;
    group.name = name;
    group.parent_id = parent_id;
    group.updated_at = Utc::now();
    state.profile_store.update_group(&group).await?;
    Ok(())
}

/// Spec 0008, Abschnitt 3: `confirm_cascade: false` liefert nur die
/// Vorschau (nichts wird gelöscht), `confirm_cascade: true` löscht
/// tatsächlich — ein zweiter, expliziter Aufruf, kein Query-Parameter, der
/// versehentlich beim ersten Aufruf schon `true` sein könnte.
#[tauri::command]
pub async fn delete_group(
    state: State<'_, AppState>,
    id: GroupId,
    confirm_cascade: bool,
) -> CommandResult<DeleteGroupResult> {
    let result =
        compute_delete_group_result(state.profile_store.as_ref(), id, confirm_cascade).await?;
    if confirm_cascade {
        state.profile_store.delete_group(&id).await?;
    }
    Ok(result)
}

// --- Spec 0008: Server -----------------------------------------------------

#[tauri::command]
pub async fn get_server(state: State<'_, AppState>, id: ServerId) -> CommandResult<ServerDto> {
    let server = state.profile_store.get_server(&id).await?;
    Ok(ServerDto::from(&server))
}

/// Spec 0008, Abschnitt 4: `CredentialStore` zuerst, dann die DB-Zeile —
/// dieselbe Reihenfolge/Begründung wie `add_ai_provider` (Spec 0007,
/// Abschnitt 8.2).
#[tauri::command]
pub async fn create_server(
    state: State<'_, AppState>,
    input: ServerInput,
) -> CommandResult<ServerId> {
    let id = ServerId::new();
    let auth = resolve_auth_method(state.credential_store.as_ref(), id, input.auth, None)?;

    let now = Utc::now();
    let server = Server {
        id,
        name: input.name,
        host: input.host,
        port: input.port,
        username: input.username,
        group_id: input.group_id,
        tags: input.tags,
        auth,
        notes: String::new(),
        jump_host: input.jump_host,
        created_at: now,
        updated_at: now,
    };

    if let Err(err) = state.profile_store.create_server(&server).await {
        // Best-effort-Aufräumen, analog zu `add_ai_provider`: ohne diesen
        // Rückbau blieben bei einem DB-Fehler verwaiste Credential-
        // Einträge im Keychain zurück.
        delete_auth_method_secrets(state.credential_store.as_ref(), &server.auth);
        return Err(err.into());
    }
    Ok(id)
}

#[tauri::command]
pub async fn update_server(
    state: State<'_, AppState>,
    id: ServerId,
    input: ServerInput,
) -> CommandResult<()> {
    let existing = state.profile_store.get_server(&id).await?;
    let auth = resolve_auth_method(
        state.credential_store.as_ref(),
        id,
        input.auth,
        Some(&existing.auth),
    )?;

    let server = Server {
        id,
        name: input.name,
        host: input.host,
        port: input.port,
        username: input.username,
        group_id: input.group_id,
        tags: input.tags,
        auth,
        notes: existing.notes,
        jump_host: input.jump_host,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    state.profile_store.update_server(&server).await?;
    Ok(())
}

#[tauri::command]
pub async fn delete_server(state: State<'_, AppState>, id: ServerId) -> CommandResult<()> {
    let server = state.profile_store.get_server(&id).await?;
    delete_auth_method_secrets(state.credential_store.as_ref(), &server.auth);
    state.profile_store.delete_server(&id).await?;
    Ok(())
}

/// Spec 0008, Abschnitt 7. `existing_server_id` ist eine gegenüber der
/// Spec-Skizze notwendige Ergänzung — s. Doc-Kommentar an
/// `crate::test_connection::test_connection`.
#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
) -> CommandResult<TestConnectionResult> {
    crate::test_connection::test_connection(
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        state.host_key_store.clone(),
        &crate::test_connection::RealConnector,
        input,
        existing_server_id,
    )
    .await
}

/// Kein Teil der Spec-0008-Signaturliste — aber zwingend nötig, damit das
/// Frontend nach einer in `test_connection` bestätigten
/// `HostKeyUnknown`/`HostKeyMismatch`-Warnung tatsächlich `trust()`
/// aufrufen kann (Spec Abschnitt 7: "kann ... bei Zustimmung `trust()`
/// aufrufen"). Anders als der reguläre `connect()`-Host-Key-Fluss (Spec
/// 0007) braucht `test_connection` dafür keine wartende Session/
/// `oneshot`-Bestätigung — es ist ein einzelner, synchroner
/// Vertrauens-Eintrag, danach ist der Aufruf fertig.
#[tauri::command]
pub async fn trust_host_key(
    state: State<'_, AppState>,
    host: String,
    port: u16,
    raw_key: Vec<u8>,
) -> CommandResult<()> {
    state.host_key_store.trust(&host, port, &raw_key)?;
    Ok(())
}

// --- Spec 0008: Notizen ----------------------------------------------------

#[tauri::command]
pub async fn update_group_notes(
    state: State<'_, AppState>,
    id: GroupId,
    content: String,
) -> CommandResult<()> {
    let revision = record_revision(NoteTarget::Group(id), content, NoteEditor::User);
    state.profile_store.record_note_revision(&revision).await?;
    Ok(())
}

#[tauri::command]
pub async fn update_server_notes(
    state: State<'_, AppState>,
    id: ServerId,
    content: String,
) -> CommandResult<()> {
    let revision = record_revision(NoteTarget::Server(id), content, NoteEditor::User);
    state.profile_store.record_note_revision(&revision).await?;
    Ok(())
}

#[tauri::command]
pub async fn list_note_revisions(
    state: State<'_, AppState>,
    target: NoteTarget,
) -> CommandResult<Vec<NoteRevisionDto>> {
    let revisions = state.profile_store.list_note_revisions(target).await?;
    Ok(revisions.iter().map(NoteRevisionDto::from).collect())
}

/// Spec 0008, Abschnitt 5: überschreibt die vorherige Revision **nicht**
/// still, sondern erzeugt selbst eine neue Revision mit dem alten Inhalt
/// (append-only) — so bleibt nachvollziehbar, dass ein Rollback
/// stattgefunden hat.
#[tauri::command]
pub async fn rollback_note(
    state: State<'_, AppState>,
    target: NoteTarget,
    revision_id: Uuid,
) -> CommandResult<()> {
    let revisions = state.profile_store.list_note_revisions(target).await?;
    let old = revisions
        .iter()
        .find(|r| r.id == revision_id)
        .ok_or("Revision nicht gefunden")?;
    let revision = record_revision(target, old.content.clone(), NoteEditor::User);
    state.profile_store.record_note_revision(&revision).await?;
    Ok(())
}

#[tauri::command]
pub async fn preview_effective_notes(
    state: State<'_, AppState>,
    server_id: ServerId,
) -> CommandResult<String> {
    let server = state.profile_store.get_server(&server_id).await?;
    effective_notes(&server, state.profile_store.as_ref())
        .await
        .map_err(Into::into)
}

// --- Spec 0009: Filter-Regel-Verwaltung ------------------------------------

/// `scope_filter: None` liefert alle Regeln (s. `crate::filter_rules::list_rules`-
/// Doc-Kommentar zur `ScopeFilter::All`-Vereinfachung).
#[tauri::command]
pub async fn list_rules(
    state: State<'_, AppState>,
    scope_filter: Option<Scope>,
) -> CommandResult<Vec<RuleDto>> {
    crate::filter_rules::list_rules(&state.policy_store, scope_filter)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn create_rule(state: State<'_, AppState>, input: RuleInput) -> CommandResult<RuleId> {
    crate::filter_rules::create_rule(&state.policy_store, input)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn update_rule(
    state: State<'_, AppState>,
    id: RuleId,
    input: RuleInput,
) -> CommandResult<()> {
    crate::filter_rules::update_rule(&state.policy_store, id, input)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn delete_rule(state: State<'_, AppState>, id: RuleId) -> CommandResult<()> {
    state.policy_store.delete(&id).await.map_err(Into::into)
}

/// Rein lesend, kein `AppState` nötig — die Hard-Blacklist ist fest im Core
/// codiert (Spec 0002, Abschnitt 3.1), nicht in der Datenbank.
#[tauri::command]
pub async fn list_hard_blacklist() -> CommandResult<Vec<PatternDto>> {
    Ok(hard_blacklist_patterns()
        .iter()
        .map(PatternDto::from)
        .collect())
}

#[tauri::command]
pub async fn list_known_tags(state: State<'_, AppState>) -> CommandResult<Vec<String>> {
    state
        .profile_store
        .list_known_tags()
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn evaluate_explained(
    state: State<'_, AppState>,
    command: String,
    ctx: EvalContextInput,
) -> CommandResult<EvaluationTraceDto> {
    Ok(crate::filter_rules::evaluate_explained(state.policy_store.clone(), command, ctx).await)
}

// --- Spec 0011: Regel-Schnellvorschlag im Bestätigungsdialog ---------------

/// Rein lesend, kein `AppState` nötig — reine Textheuristik ohne
/// Datenbankzugriff (Spec 0011, Abschnitt 2).
#[tauri::command]
pub async fn suggest_rule_patterns(command: String) -> CommandResult<Vec<PatternSuggestionDto>> {
    Ok(crate::rule_suggestions::suggest_rule_patterns(&command))
}

/// Spec 0011, Abschnitt 3: legt zuerst die Regel an (Schritt 1, delegiert
/// an [`crate::filter_rules::create_rule`] über
/// [`crate::rule_suggestions::create_quick_rule`]), löst **danach** die
/// wartende `Confirm`-Entscheidung für `action_id` auf (Schritt 2) — exakt
/// wie ein `respond_to_action`-Aufruf mit `Approve`. Schlägt Schritt 1 fehl,
/// wird Schritt 2 nicht erreicht (kein `?` vor dem `resolve`-Aufruf nötig,
/// `?` auf `create_quick_rule` selbst genügt) — kein halb abgeschlossener
/// Zustand (Regel angelegt, aber Dialog bleibt hängen, oder umgekehrt).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn accept_and_create_rule(
    state: State<'_, AppState>,
    // Wie bei `respond_to_action` (Spec 0010) Teil der Signatur, aber nicht
    // die Grundlage für eine Gültigkeitsprüfung — `pending_action_confirmations
    // .resolve()` prüft `action_id` bereits selbst ausreichend (s. dortiger
    // Kommentar).
    session_id: SessionId,
    action_id: ActionId,
    pattern_type: PatternType,
    pattern_value: String,
    scope: Scope,
    priority: Option<i32>,
) -> CommandResult<RuleId> {
    let _ = session_id;
    let rule_id = crate::rule_suggestions::create_quick_rule(
        &state.policy_store,
        pattern_type,
        pattern_value,
        scope,
        priority,
    )
    .await?;
    state
        .pending_action_confirmations
        .resolve(&action_id, ActionUserDecision::Approve)?;
    Ok(rule_id)
}

// --- Spec 0012: KI-generierte Dokumente -------------------------------

/// Spec 0012, Abschnitt 3: öffnet einen nativen Speichern-unter-Dialog
/// (vorbelegt mit einem aus `title` abgeleiteten Dateinamen, s.
/// [`crate::document_export::default_export_file_name`]) und schreibt
/// **erst nach dessen Bestätigung** — bricht der Nutzer den Dialog ab,
/// liefert der Callback `None`, der Command kehrt dann ohne jeden
/// Seiteneffekt zurück (kein Fehler: Abbrechen ist kein Fehlerfall).
///
/// Der Dialog-Callback selbst ist nicht `async` (Tauri-Dialog-Plugin-API,
/// Abschnitt 3 der Spec nennt nur "nativer Speichern-unter-Dialog", nicht
/// welche der beiden Varianten) — er wird deshalb über einen `oneshot`-Kanal
/// an diesen `async fn`-Command zurücküberführt, statt die blockierende
/// `blocking_save_file()`-Variante zu nutzen, die den Async-Runtime-Thread
/// blockieren würde.
#[tauri::command]
pub async fn export_document(
    app: AppHandle,
    content_markdown: String,
    title: String,
    format: DocumentFormat,
) -> CommandResult<()> {
    use tauri_plugin_dialog::DialogExt;

    let file_name = crate::document_export::default_export_file_name(&title, format);
    let (filter_name, extension): (&str, &str) = match format {
        DocumentFormat::Markdown => ("Markdown", "md"),
        DocumentFormat::Word => ("Word-Dokument", "docx"),
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&file_name)
        .add_filter(filter_name, &[extension])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(path) = rx.await.ok().flatten() else {
        return Ok(());
    };
    let path = path.into_path()?;

    // `std::fs::write` statt `tokio::fs`: Letzteres bräuchte das
    // ungenutzte `fs`-Feature nur für diesen einen, durch eine explizite
    // Nutzeraktion ausgelösten Einzelschreibvorgang — für eine
    // Analyse-Dokumentgröße unkritisch blockierend.
    match format {
        DocumentFormat::Markdown => std::fs::write(path, content_markdown)?,
        DocumentFormat::Word => std::fs::write(
            path,
            crate::document_export::markdown_to_docx_bytes(&content_markdown),
        )?,
    }

    Ok(())
}

/// Validiert und bereinigt `uname -a` Output (Spec 0013, SEC-02) vor der
/// Aufnahme in den privilegierten System-Prompt: max 256 Zeichen, nur
/// erlaubte Zeichen (alphanumerisch, . _ - # : space tab), keine Steuerzeichen
/// oder Zeilenumbrüche.
pub(crate) fn sanitize_uname_output(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 256 {
        return None;
    }
    if trimmed.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || c == '.'
            || c == '_'
            || c == '-'
            || c == ' '
            || c == '\t'
            || c == '#'
            || c == ':'
    }) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Liest den Textinhalt einer vom Nutzer im nativen Dateidialog ausgewählten
/// Schlüssel-/Zertifikatsdatei (Spec 0013, SEC-06). Ersetzt globale Dateilese-
/// Berechtigungen im Frontend.
#[tauri::command]
pub async fn read_credential_file(path: String) -> CommandResult<String> {
    let content = std::fs::read_to_string(&path)?;
    Ok(content)
}
