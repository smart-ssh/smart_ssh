//! Tauri-Commands (Spec 0007, Abschnitt 4).

use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;

use chrono::Utc;
use uuid::Uuid;

use persistence_sqlite::AiProviderConfig;
use ssh_manager_core::ai::{
    default_action_schemas, ChatMessage, DefaultOutputRedactor, MessageContent, OutputRedactor,
    ProviderId, Role, SessionContext,
};
use ssh_manager_core::filter::{
    hard_blacklist_patterns, EffectiveScope, EvalContext, FilterEngine, PolicyStore, RuleAction,
    RuleId, Scope,
};
use ssh_manager_core::profiles::{
    effective_notes, record_revision, Group, GroupId, NoteEditor, NoteTarget, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{resolve_connection_target, HostKeyDecision, PtySize};

use crate::ai_provider_factory::build_ai_provider;
use crate::dto::{
    credential_ref_for, sort_remote_entries, ActionUserDecision, AiProviderConfigDto,
    AiProviderConfigInput, DeleteGroupResult, DocumentFormat, EvalContextInput, EvaluationTraceDto,
    GroupDto, HostKeyUserDecision, NoteRevisionDto, PatternDto, PatternSuggestionDto, PatternType,
    RemoteEntryDto, RuleDto, RuleInput, ServerDto, ServerInput, SessionSummaryDto,
    TestConnectionResult,
};
use crate::error::{CommandError, CommandResult};
use crate::events::{
    emit_connection_status_changed, emit_host_key_verification_needed, emit_sftp_transfer_finished,
    emit_sftp_transfer_started, ConnectionStatus, EventEmitter, HostKeyKind, SftpTransferKind,
};
use crate::groups::{compute_delete_group_result, validate_no_cycle};
use crate::orchestration::run_chat_turn;
use crate::server_credentials::{
    clear_sudo_password, delete_auth_method_secrets, resolve_auth_method, resolve_sudo_password,
    sudo_password_credential_ref,
};
use crate::session::{spawn_terminal_actor, Session, TerminalCommand};
use crate::state::{ActionId, AppState, SessionId};

/// `group_id` erweitert die Spec-0007-Signatur um den in Spec 0008
/// Abschnitt 4 vorgesehenen Filter (`None` = alle Server, wie bisher für
/// die einfache Liste aus Spec 0007 Teil 1 gebraucht).
#[tauri::command]
pub async fn list_servers(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: Option<GroupId>,
) -> CommandResult<Vec<ServerDto>> {
    list_servers_impl(
        &app,
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        group_id,
    )
    .await
}

/// Kern von [`list_servers`], herausgelöst aus dem `#[tauri::command]`-
/// Wrapper (analog zu `connect`/`connect_session`), damit dieser Test ohne
/// vollständigen `AppState` (echte SQLite-Stores, Keyring) auskommt — nur
/// `ProfileStore`/`CredentialStore` als Trait-Objekt plus ein gemocktes
/// `AppHandle` für `crate::local_server::synthetic_server`.
async fn list_servers_impl<R: tauri::Runtime>(
    app: &AppHandle<R>,
    profile_store: &dyn ProfileStore,
    credential_store: &(dyn ssh_manager_core::profiles::CredentialStore + Send + Sync),
    group_id: Option<GroupId>,
) -> CommandResult<Vec<ServerDto>> {
    let servers = profile_store.list_servers().await?;
    // Spec 0032, Abschnitt 3: der lokale Pseudo-Server ist immer das erste
    // Element, unabhängig von `group_id` — er gehört nie einer Gruppe an,
    // ein Gruppenfilter kann ihn also nie sinnvoll ausschließen.
    let local = ServerDto::from_server(
        &crate::local_server::synthetic_server(app),
        credential_store,
    );
    let rest = servers
        .iter()
        .filter(|s| group_id.is_none() || s.group_id == group_id)
        .map(|s| ServerDto::from_server(s, credential_store));
    Ok(std::iter::once(local).chain(rest).collect())
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

/// Spec 0025, Abschnitt 2: `GET {base_url}/models` — läuft mit den gerade
/// im Formular eingegebenen, noch nicht gespeicherten Werten (analog zu
/// `test_connection`, Spec 0008 Abschnitt 7), nicht mit einer bereits
/// persistierten Config. `existing_provider_id` deckt denselben Fall wie
/// dort ab: ist das `api_key`-Feld leer (Bearbeiten eines gespeicherten
/// Providers, "leer = unverändert"), wird stattdessen dessen bereits
/// hinterlegtes Credential herangezogen.
///
/// Nur für die OpenAI-kompatible Familie unterstützt (Spec 0025, Abschnitt
/// 2) — `anthropic` hat kein äquivalentes `/models`-Endpoint-Verhalten in
/// dieser Spec und wird mit einem klaren Fehler abgelehnt, statt einen
/// wahrscheinlich falsch geformten Request zu versuchen.
#[tauri::command]
pub async fn discover_models(
    state: State<'_, AppState>,
    config: AiProviderConfigInput,
    existing_provider_id: Option<ProviderId>,
) -> CommandResult<Vec<String>> {
    if !matches!(
        config.provider_type,
        ssh_manager_core::ai::ProviderType::OpenAi
            | ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible
            | ssh_manager_core::ai::ProviderType::Ollama
    ) {
        return Err("Modell-Discovery wird für diesen Provider-Typ nicht unterstützt".into());
    }

    let api_key = if !config.api_key.is_empty() {
        config.api_key.clone()
    } else if let Some(id) = existing_provider_id {
        let existing = state.ai_provider_store.get(&id).await?;
        state
            .credential_store
            .get(&existing.credential_ref)?
            .expose_secret()
            .to_string()
    } else {
        return Err("API-Key erforderlich".into());
    };

    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or(crate::ai_provider_factory::DEFAULT_OPENAI_BASE_URL);

    let models = ai_providers::discover_models(base_url, &api_key, &config.extra_headers).await?;
    Ok(models)
}

/// Spec 0025, Abschnitt 4: ruft den beim Provider hinterlegten
/// Attestierungs-Endpunkt ab und liefert die **rohe** Antwort unverändert
/// — anders als `discover_models` erst nach dem Speichern nutzbar
/// (`provider_id` statt Formulardaten), da der Endpunkt laut Spec "beim
/// Speichern und auf Wunsch erneut" abgerufen wird, nicht während der
/// Eingabe.
#[tauri::command]
pub async fn fetch_attestation_info(
    state: State<'_, AppState>,
    provider_id: ProviderId,
) -> CommandResult<String> {
    let existing = state.ai_provider_store.get(&provider_id).await?;
    let url = existing
        .attestation_url
        .ok_or("Kein Attestierungs-Endpunkt für diesen Provider konfiguriert")?;
    let info = ai_providers::fetch_attestation_info(&url).await?;
    Ok(info)
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
    connect_session(&app, &state, server_id, session_id).await
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
pub(crate) async fn connect_session(
    app: &AppHandle,
    state: &AppState,
    server_id: ServerId,
    session_id: SessionId,
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

    let is_local = crate::local_server::is_local(server_id);
    let server = if is_local {
        crate::local_server::synthetic_server(app)
    } else {
        state.profile_store.get_server(&server_id).await?
    };
    let active_config = active_ai_provider_config(state).await?;
    let api_key = state.credential_store.get(&active_config.credential_ref)?;
    let ai_provider = build_ai_provider(
        active_config.provider_type,
        active_config.base_url.as_deref(),
        &active_config.model,
        api_key,
        active_config.supports_native_tool_calling,
        active_config.extra_headers.clone(),
    );

    // Spec 0032, Abschnitt 2/3: der lokale Pseudo-Server hat keinen
    // Verbindungszustand, keinen Host-Key und keine Credentials — "Verbinden"
    // ist hier nur die Konstruktion eines `LocalTransport`, ohne die
    // `russh`/Host-Key-Schleife unten zu durchlaufen.
    let mut transport: Box<dyn ssh_manager_core::ssh::SshTransport> = if is_local {
        Box::new(ssh_transport::LocalTransport::new())
    } else {
        let target = resolve_connection_target(&server, state.profile_store.as_ref()).await?;
        loop {
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

                    tracing::info!(
                        session_id = %session_id,
                        host = %host,
                        port,
                        kind = ?kind,
                        "host key verification needed",
                    );

                    let rx = state.pending_host_key_confirmations.register(session_id);
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
                        app,
                        session_id,
                        host.clone(),
                        port,
                        kind,
                        fingerprint,
                        expected_fingerprint,
                    );

                    let user_decision_result = rx.await;
                    state.sessions.clear_pending_connection(session_id);
                    let Ok(user_decision) = user_decision_result else {
                        return Err("Verbindungsaufbau abgebrochen".into());
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
                            return Err(format!(
                                "Verbindung zu {host}:{port} abgelehnt (Host-Key nicht vertraut)"
                            )
                            .into());
                        }
                    }
                }
            }
        }
    };

    let sanitized_os = if let Ok(uname_output) = transport.execute("uname -a").await {
        let uname_text = String::from_utf8_lossy(&uname_output.stdout);
        sanitize_uname_output(&uname_text)
    } else {
        None
    };

    let system_context = build_session_system_context(
        app,
        &server.name,
        &server_id,
        &server.tags,
        sanitized_os.as_deref(),
        state.profile_store.as_ref(),
        &state.policy_store,
    )
    .await;

    // Spec 0018, Abschnitt 6: einmalig bei `connect()` gelesen, wie
    // `ai_provider_label`/`ai_model` — ein fehlender Eintrag (kein Sudo-
    // Passwort hinterlegt) wird zu `None`, kein harter Verbindungsfehler.
    let sudo_password = state
        .credential_store
        .get(&sudo_password_credential_ref(server_id))
        .ok();

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
    let redactor: Box<dyn OutputRedactor> = match &sudo_password {
        Some(password) => match regex::Regex::new(&regex::escape(password.expose_secret())) {
            Ok(pattern) => Box::new(DefaultOutputRedactor::with_extra_patterns(vec![pattern])),
            Err(_) => Box::new(DefaultOutputRedactor::new()),
        },
        None => Box::new(DefaultOutputRedactor::new()),
    };

    // Spec 0026, Abschnitt 3: einmalig bei `connect()` aufgelöst, s.
    // `Session::risk_second_opinion_provider`-Doc-Kommentar.
    let risk_second_opinion_provider =
        crate::risk_second_opinion::resolve_second_opinion_provider(app, state).await;

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
        redactor,
        ai_provider_label: active_config.display_name,
        ai_model: active_config.model,
        sudo_password,
        status: std::sync::Mutex::new(crate::events::ConnectionStatus::Connected),
        pending_action: std::sync::Mutex::new(None),
        sftp: tokio::sync::Mutex::new(None),
        auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
        risk_second_opinion_provider,
        running_command_cancellations: state.running_command_cancellations.clone(),
    });
    state.sessions.insert(session_id, session);

    tracing::info!(session_id = %session_id, server_id = %server_id.0, "session connected");
    emit_connection_status_changed(app, session_id, ConnectionStatus::Connected, None);
    Ok(session_id)
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

async fn build_session_system_context<R: tauri::Runtime>(
    app: &AppHandle<R>,
    server_name: &str,
    server_id: &ServerId,
    tags: &[String],
    remote_os_info: Option<&str>,
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
) -> String {
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
    let notes = if crate::local_server::is_local(*server_id) {
        crate::local_server::synthetic_server(app).notes
    } else {
        match profile_store.get_server(server_id).await {
            Ok(s) => match effective_notes(&s, profile_store).await {
                Ok(notes) => notes,
                Err(err) => {
                    tracing::warn!(
                        server_id = %server_id.0,
                        error = %err,
                        "effective_notes fehlgeschlagen — Session-Kontext enthält keine Notizen",
                    );
                    String::new()
                }
            },
            Err(err) => {
                tracing::warn!(
                    server_id = %server_id.0,
                    error = %err,
                    "get_server fehlgeschlagen — Session-Kontext enthält keine Notizen",
                );
                String::new()
            }
        }
    };

    let mut context = format!(
        "Du bist ein intelligenter SSH- und System-Administrations-Assistent für den Server '{server_name}'.\n\
         Du unterstützt den Administrator bei der Analyse, Wartung und Verwaltung des Systems.\n\n\
         Wichtige Handlungsanweisungen für Werkzeuge:\n\
         - Wenn du Befehle auf dem Remote-Server ausführen möchtest, schlage sie mit dem Werkzeug `suggest_command` vor.\n\
         - Wenn der Nutzer nach einem Dokument, Bericht, einer Zusammenfassung als Datei, einer Analyse oder einem Word-/Markdown-Export fragt, erstelle den vollständigen Inhalt und rufe IMMER das Werkzeug `generate_document` auf. Antworte in diesem Fall nicht nur mit einfachem Chat-Text und behaupte nicht, das Dokument erstellt zu haben, ohne die Funktion aufzurufen.\n\
         - Halte während der gesamten Sitzung aktiv Ausschau nach für künftige Sitzungen nützlichen Erkenntnissen (installierte Software/Versionen, Konfigurationspfade, getroffene Entscheidungen, behobene Probleme, Systembesonderheiten) und schlage dafür proaktiv — bei Bedarf auch mehrfach pro Sitzung, sobald sich jeweils etwas Neues ergibt, nicht erst am Ende abwartend — eine Notiz-Aktualisierung mit `propose_note_update` vor. Wiederhole dabei keine bereits in den Notizen stehenden Informationen."
    );

    let eval_ctx = EvalContext {
        server_id: *server_id,
        tags: tags.to_vec(),
    };
    let scope = EffectiveScope::from(&eval_ctx);
    let rules = policy_store.rules_for(&scope).await;
    let allow_rules: Vec<String> = rules
        .iter()
        .filter(|r| r.action == RuleAction::Allow)
        .map(|r| {
            format!(
                "- `{}` ({})",
                r.pattern.display_text(),
                r.pattern.kind_str()
            )
        })
        .collect();

    if !allow_rules.is_empty() {
        context.push_str("\n\n## Freigegebene Befehle (Whitelist / AutoExec)\nDie folgenden Befehle sind für diesen Server freigegeben und können ohne Rückfrage direkt ausgeführt werden:\n");
        context.push_str(&allow_rules.join("\n"));
    }

    if !notes.is_empty() {
        context.push_str(&format!("\n\n## Notizen / Kontext\n{notes}"));
    }

    if let Some(os) = remote_os_info {
        context.push_str(&format!("\n\n## Remote-System\n{os}"));
    }

    context
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
        Arc::clone(&session),
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

    // Spec 0015, Abschnitt 3: Prompt-Historie ist eine Zusatzfunktion für
    // die Pfeiltasten-Navigation im Eingabefeld — ein Fehlschlag beim
    // Persistieren (z. B. kurzzeitig gesperrte DB) soll den eigentlichen
    // Chat-Versand nicht verhindern, deshalb best-effort statt `?`.
    if let Err(err) = state
        .prompt_history_store
        .record(&session.server_id, &text)
        .await
    {
        eprintln!("Prompt konnte nicht in der Historie gespeichert werden: {err}");
    }

    // Spec 0032: `profile_store.get_server` findet den lokalen
    // Pseudo-Server nie (keine `servers`-Zeile) — ohne diesen Zweig würde
    // der Servername in JEDER Chat-Nachricht auf das generische "Server"
    // degradieren (unabhängiger Review-Pass, s. docs/adr/0026).
    let (server_name, current_tags) = if crate::local_server::is_local(session.server_id) {
        let local = crate::local_server::synthetic_server(&app);
        (local.name, local.tags)
    } else {
        match state.profile_store.get_server(&session.server_id).await {
            Ok(s) => (s.name, s.tags),
            Err(_) => ("Server".to_string(), session.tags.clone()),
        }
    };

    let remote_os = {
        let ctx = session.context.lock().await;
        ctx.system_context
            .find("## Remote-System\n")
            .map(|pos| ctx.system_context[pos + "## Remote-System\n".len()..].to_string())
    };

    let updated_system_context = build_session_system_context(
        &app,
        &server_name,
        &session.server_id,
        &current_tags,
        remote_os.as_deref(),
        state.profile_store.as_ref(),
        &state.policy_store,
    )
    .await;

    {
        let mut ctx = session.context.lock().await;
        ctx.system_context = updated_system_context;
        ctx.history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text(text),
        });
    }

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

/// Spec 0021, Abschnitt 5: "Automatik stoppen" — bricht die automatische
/// Fortsetzungskette für die aktuelle Nutzer-Nachricht sofort ab (keine
/// weiteren automatischen `AiProvider::send()`-Aufrufe mehr), unabhängig
/// vom Runden-Zähler. Ein bereits offener Bestätigungsdialog ist davon
/// nicht betroffen — `run_chat_turn` prüft dieses Flag nur *zwischen*
/// Runden (s. dortiger Kommentar), nie während eine Runde noch läuft.
#[tauri::command]
pub async fn stop_auto_continuation(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    session
        .auto_continue_stop
        .store(true, std::sync::atomic::Ordering::SeqCst);
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

    tracing::info!(session_id = %session_id, "session disconnected");
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
    let result = compute_delete_group_result(
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        id,
        confirm_cascade,
    )
    .await?;
    if confirm_cascade {
        state.profile_store.delete_group(&id).await?;
    }
    Ok(result)
}

// --- Spec 0008: Server -----------------------------------------------------

#[tauri::command]
pub async fn get_server(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ServerId,
) -> CommandResult<ServerDto> {
    if crate::local_server::is_local(id) {
        return Ok(ServerDto::from_server(
            &crate::local_server::synthetic_server(&app),
            state.credential_store.as_ref(),
        ));
    }
    let server = state.profile_store.get_server(&id).await?;
    Ok(ServerDto::from_server(
        &server,
        state.credential_store.as_ref(),
    ))
}

/// Spec 0032, Abschnitt 6: der lokale Pseudo-Server ist explizit als
/// Jump-Host ausgeschlossen — vor dieser Prüfung fiel das erst implizit,
/// tief in `resolve_connection_target`, mit einer generischen "nicht
/// auflösbar"-Meldung auf (unabhängiger Review-Pass, s. docs/adr/0026).
fn reject_local_jump_host(jump_host: Option<ServerId>) -> CommandResult<()> {
    if jump_host.is_some_and(crate::local_server::is_local) {
        return Err(CommandError::with_code(
            "Der lokale Pseudo-Server kann nicht als Jump-Host verwendet werden",
            "SERVER_JUMP_HOST_LOCAL",
        ));
    }
    Ok(())
}

/// Spec 0008, Abschnitt 4: `CredentialStore` zuerst, dann die DB-Zeile —
/// dieselbe Reihenfolge/Begründung wie `add_ai_provider` (Spec 0007,
/// Abschnitt 8.2).
#[tauri::command]
pub async fn create_server(
    state: State<'_, AppState>,
    input: ServerInput,
) -> CommandResult<ServerId> {
    reject_local_jump_host(input.jump_host)?;
    let id = ServerId::new();
    let auth = resolve_auth_method(state.credential_store.as_ref(), id, input.auth, None)?;
    resolve_sudo_password(state.credential_store.as_ref(), id, input.sudo_password)?;

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
    if crate::local_server::is_local(id) {
        // Spec 0032, Abschnitt 3: existiert nicht als `servers`-Zeile — nur
        // Notizen/Tags sind editierbar, über die dedizierten
        // `update_local_server_notes`/`update_local_server_tags`-Befehle.
        return Err("Der lokale Pseudo-Server kann nicht auf diesem Weg bearbeitet werden".into());
    }
    reject_local_jump_host(input.jump_host)?;
    let existing = state.profile_store.get_server(&id).await?;
    let auth = resolve_auth_method(
        state.credential_store.as_ref(),
        id,
        input.auth,
        Some(&existing.auth),
    )?;
    resolve_sudo_password(state.credential_store.as_ref(), id, input.sudo_password)?;

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
    if crate::local_server::is_local(id) {
        // Spec 0032, Abschnitt 3: existiert nicht als löschbare Zeile.
        return Err("Der lokale Pseudo-Server kann nicht gelöscht werden".into());
    }
    let server = state.profile_store.get_server(&id).await?;
    delete_auth_method_secrets(state.credential_store.as_ref(), &server.auth);
    clear_sudo_password(state.credential_store.as_ref(), id);
    state.profile_store.delete_server(&id).await?;
    Ok(())
}

/// Spec 0018, Abschnitt 4: expliziter "Entfernen"-Weg — ein leeres
/// `sudo_password`-Feld in `update_server` bedeutet bereits "unverändert",
/// s. `crate::server_credentials::resolve_sudo_password`.
#[tauri::command]
pub async fn clear_server_sudo_password(
    state: State<'_, AppState>,
    id: ServerId,
) -> CommandResult<()> {
    clear_sudo_password(state.credential_store.as_ref(), id);
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
    if existing_server_id.is_some_and(crate::local_server::is_local) {
        // Spec 0032, Abschnitt 5: kein Verbindungstest-Button für den
        // lokalen Pseudo-Server (er hat gar keine Verbindung, die getestet
        // werden könnte).
        return Err("Für den lokalen Pseudo-Server gibt es keinen Verbindungstest".into());
    }
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

/// Spec 0032, Abschnitt 3: Notizen des lokalen Pseudo-Servers laufen nicht
/// über `record_note_revision` (keine `servers`-Zeile, s.
/// `crate::local_server`-Doc-Kommentar) — dediziertes Befehlspaar statt
/// `update_server_notes`/`NoteTarget::Server`, bewusst **ohne**
/// Revisions-Historie.
#[tauri::command]
pub async fn update_local_server_notes(app: AppHandle, content: String) -> CommandResult<()> {
    crate::local_server::save_notes(&app, &content).map_err(Into::into)
}

#[tauri::command]
pub async fn update_local_server_tags(app: AppHandle, tags: Vec<String>) -> CommandResult<()> {
    crate::local_server::save_tags(&app, &tags).map_err(Into::into)
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
/// wartende `Confirm`-Entscheidung für `action_id` auf (Schritt 2). Schlägt
/// Schritt 1 fehl, wird Schritt 2 nicht erreicht (kein `?` vor dem
/// `resolve`-Aufruf nötig, `?` auf `create_quick_rule` selbst genügt) —
/// kein halb abgeschlossener Zustand (Regel angelegt, aber Dialog bleibt
/// hängen, oder umgekehrt).
///
/// `edited_command`: unabhängiger Review-Pass (Spec 0007/0008) — das
/// Frontend zeigt/verwendet zur Muster-Ableitung den vom Nutzer im
/// Bearbeiten-Feld editierten Text (`ConfirmActionForm`s `edited`-State),
/// aber diese Funktion löste die Bestätigung bislang immer mit
/// `ActionUserDecision::Approve` auf — das führt die **ursprüngliche,
/// unbearbeitete** `AiAction` aus. Ein Nutzer, der z. B. `rm -rf
/// /var/log/*` zu `ls /var/log` bearbeitet und dann "Regel anlegen &
/// ausführen" klickt, bekäme eine Regel für `ls /var/log`, während
/// tatsächlich `rm -rf /var/log/*` ausgeführt würde — exakt der
/// Bestätigungsdialog-Bypass, gegen den `EditThenApprove` (Aufgabenstellung
/// Teil 1, Punkt 4) eigentlich schützt. `Some(cmd)` (Text unterscheidet
/// sich vom ursprünglich vorgeschlagenen Kommando) löst deshalb jetzt mit
/// `EditThenApprove { command: cmd }` auf — dieselbe erneute
/// Filter-Engine-Prüfung wie beim regulären "Ausführen"-Button
/// (`crate::orchestration::handle_user_decision`). `None` (Text
/// unverändert) verhält sich wie zuvor.
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
    edited_command: Option<String>,
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
    let decision = match edited_command {
        Some(command) => ActionUserDecision::EditThenApprove { command },
        None => ActionUserDecision::Approve,
    };
    state
        .pending_action_confirmations
        .resolve(&action_id, decision)?;
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

// --- Spec 0015: Chat-Prompt-Historie ---------------------------------------

/// Spec 0015, Abschnitt 4: liefert die gespeicherten Prompts eines Servers
/// chronologisch aufsteigend (älteste zuerst) — das Frontend kehrt für die
/// Pfeiltasten-Navigation selbst um bzw. greift von hinten zu.
#[tauri::command]
pub async fn list_prompt_history(
    state: State<'_, AppState>,
    server_id: ServerId,
) -> CommandResult<Vec<String>> {
    Ok(state.prompt_history_store.list(&server_id).await?)
}

// --- Spec 0016: Strukturiertes Logging & Diagnose --------------------------

/// Spec 0016, Abschnitt 5: öffnet den Log-Ordner im System-Dateimanager
/// (Finder/Explorer) — ein Klick statt manuell zum plattformspezifischen
/// Pfad navigieren zu müssen.
#[tauri::command]
pub async fn open_log_directory(app: AppHandle) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let dir = crate::logging::default_log_dir();
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)?;
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
///
/// Unabhängiger Review-Pass (Spec 0013): nahm bislang einen beliebigen,
/// vom Frontend übergebenen `path: String` entgegen und las ihn ohne jede
/// Prüfung — funktional gleichbedeutend mit der pauschalen
/// Dateilese-Berechtigung, die SEC-06 gerade abschaffen sollte, da JEDER
/// Code im Webview (nicht nur der eigentliche "Datei wählen"-Button)
/// `invoke("read_credential_file", { path: "~/.ssh/id_rsa" })` aufrufen
/// konnte. Der Dialog läuft jetzt — wie bei `export_document`/
/// `sftp_download` bereits etabliert — im Backend selbst
/// (`app.dialog().file().pick_file(...)` + `oneshot`-Rückkanal, da der
/// Callback selbst nicht `async` ist): das Frontend übergibt nur noch
/// einen Anzeige-`title`, nie einen Pfad, und kann dadurch keinen
/// beliebigen Pfad mehr erzwingen — nur eine tatsächliche
/// Nutzerinteraktion mit dem nativen Dialog liefert einen Pfad.
#[tauri::command]
pub async fn read_credential_file(app: AppHandle, title: String) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(path) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = path.into_path()?;
    let content = std::fs::read_to_string(path)?;
    Ok(Some(content))
}

/// Liefert das aktuelle Betriebssystem ("macos", "windows", "linux", "unknown")
/// zur plattformspezifischen Anpassung des UI-Paddings im Frontend (Spec 0014, Abschnitt 4).
#[tauri::command]
pub fn get_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown"
    }
}

/// Aktiviert die Overlay-Titelleiste und konfiguriert macOS-Ampel-Insets (Spec 0014, Abschnitt 3 & 6).
#[tauri::command]
pub async fn create_overlay_titlebar(window: tauri::WebviewWindow) -> CommandResult<()> {
    use tauri_plugin_decoration::WebviewWindowExt;
    let _ = window.activate_decoration().await;
    #[cfg(target_os = "macos")]
    {
        // Spec 0014 Abschnitt 3 & 6: Startwert für Ampel-Positionierung
        let _ = window.set_traffic_lights_inset(12.0, 16.0).await;
    }
    Ok(())
}

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser -------------------------
//
// Bewusst OHNE Filter-Engine-Prüfung — anders als `ReadRemoteFile`/
// `WriteRemoteFile` (Spec 0020, Abschnitt 4, `crate::orchestration`) laufen
// diese Befehle nie über den KI-Chat, sondern sind direkte Nutzeraktionen im
// Dateibrowser-Panel, analog zum interaktiven Terminal (Spec 0005, Abschnitt
// 1: auch dort läuft rohe Tastatureingabe ungefiltert durch).
//
// **Design-Entscheidung, Löschen/Herunterladen auf Dateien beschränkt**: Der
// `SftpSession`-Trait (Spec 0020, Abschnitt 3, bereits exakt so in Teil 1
// committet) bietet nur `remove()` (SFTP `REMOVE`, wirkt ausschließlich auf
// Dateien) und kein rekursives Verzeichnis-Löschen oder einen
// Mehrdatei-Download. Ein Versuch, `remove()` auf ein Verzeichnis
// anzuwenden, schlägt serverseitig mit einem Protokollfehler fehl. Statt
// das im Frontend erst nach einem verwirrenden Fehler sichtbar zu machen,
// bietet das Kontextmenü "Herunterladen"/"Löschen" dort von vornherein nur
// für Dateien an — Verzeichnisse lassen sich weiterhin öffnen (Navigation)
// und umbenennen (SFTP `RENAME` funktioniert für beide Eintragstypen).
// Siehe ADR-Vorschlag am Ende der Aufgabe.

/// Liefert die Session und öffnet ihre SFTP-Verbindung bei Bedarf (Spec
/// 0020, Abschnitt 3, `crate::orchestration::ensure_sftp_open`) — gemeinsame
/// Vorbedingung aller `sftp_*`-Befehle unten.
async fn session_sftp(state: &AppState, session_id: SessionId) -> CommandResult<Arc<Session>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    crate::orchestration::ensure_sftp_open(&session).await?;
    Ok(session)
}

fn file_name_of(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    session_id: SessionId,
    path: String,
) -> CommandResult<Vec<RemoteEntryDto>> {
    let session = session_sftp(&state, session_id).await?;
    let entries = {
        let mut guard = session.sftp.lock().await;
        let sftp = guard
            .as_mut()
            .expect("ensure_sftp_open lief erfolgreich durch");
        sftp.list_dir(&path).await?
    };
    let mut dtos: Vec<RemoteEntryDto> = entries.iter().map(RemoteEntryDto::from).collect();
    sort_remote_entries(&mut dtos);
    Ok(dtos)
}

/// Spec 0020, Abschnitt 5: "nativer Speichern-Dialog" — derselbe
/// oneshot-Kanal-Umweg wie `export_document` (dortiger Doc-Kommentar erklärt
/// das Warum), gefolgt von einem `sftp-transfer-started`/`-finished`-
/// Ereignispaar (s. `crate::events`-Moduldoc zur Fortschritts-Design-
/// Entscheidung) und dem eigentlichen Lesen+lokalem Schreiben.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    remote_path: String,
) -> CommandResult<()> {
    use tauri_plugin_dialog::DialogExt;

    let session = session_sftp(&state, session_id).await?;
    let file_name = file_name_of(&remote_path);

    // Größe vorab für die Fortschrittsanzeige — ein fehlgeschlagenes
    // `stat()` (z. B. eingeschränkte Leserechte aufs Elternverzeichnis)
    // blockiert den eigentlichen Download nicht, die Anzeige zeigt dann
    // schlicht keine Gesamtgröße.
    let total_bytes = {
        let mut guard = session.sftp.lock().await;
        let sftp = guard
            .as_mut()
            .expect("ensure_sftp_open lief erfolgreich durch");
        sftp.stat(&remote_path).await.ok().map(|entry| entry.size)
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&file_name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(local_path) = rx.await.ok().flatten() else {
        return Ok(()); // Abbrechen ist kein Fehler, s. `export_document`.
    };
    let local_path = local_path.into_path()?;

    let transfer_id = Uuid::new_v4();
    emit_sftp_transfer_started(
        &app,
        session_id,
        transfer_id,
        SftpTransferKind::Download,
        file_name,
        total_bytes,
    );

    let result: CommandResult<()> = async {
        let bytes = {
            let mut guard = session.sftp.lock().await;
            let sftp = guard
                .as_mut()
                .expect("ensure_sftp_open lief erfolgreich durch");
            sftp.read_file(&remote_path).await?
        };
        // `spawn_blocking` statt eines direkten `std::fs::write` (anders als
        // z. B. `read_credential_file`s kleine Zertifikatsdateien): Downloads
        // hier können beliebig groß sein, Spec 0020 Abschnitt 5 verlangt
        // ausdrücklich, dass Transfers die Session nicht blockieren.
        tokio::task::spawn_blocking(move || std::fs::write(&local_path, bytes))
            .await
            .map_err(|e| format!("Hintergrund-Task für Download fehlgeschlagen: {e}"))??;
        Ok(())
    }
    .await;

    emit_sftp_transfer_finished(
        &app,
        session_id,
        transfer_id,
        result.as_ref().err().map(|e| e.message.clone()),
    );
    result
}

/// `local_path` ist bereits vom Frontend aufgelöst — entweder über den
/// nativen Öffnen-Dialog (Upload-Button, `@tauri-apps/plugin-dialog`, s.
/// `frontend/src/fileDialog.ts` für das bereits etablierte Muster) oder über
/// einen Drag-and-Drop-Vorgang aus dem Betriebssystem (der Pfad kommt dort
/// direkt vom OS-Drop-Ereignis) — beides sind explizite Nutzeraktionen im
/// Sinne von Spec 0020, Abschnitt 5 ("nie ohne expliziten Dialog"), auch
/// wenn der Dialog beim Drag-and-Drop kein Fenster ist, sondern die
/// Drag-Geste selbst.
#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    local_path: String,
    remote_path: String,
) -> CommandResult<()> {
    let session = session_sftp(&state, session_id).await?;
    let file_name = file_name_of(&remote_path);

    let local_path_for_stat = local_path.clone();
    let total_bytes = tokio::task::spawn_blocking(move || {
        std::fs::metadata(&local_path_for_stat)
            .map(|m| m.len())
            .ok()
    })
    .await
    .unwrap_or(None);

    let transfer_id = Uuid::new_v4();
    emit_sftp_transfer_started(
        &app,
        session_id,
        transfer_id,
        SftpTransferKind::Upload,
        file_name,
        total_bytes,
    );

    let local_path_for_read = local_path.clone();
    let result: CommandResult<()> = async {
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(local_path_for_read))
            .await
            .map_err(|e| format!("Hintergrund-Task für Upload fehlgeschlagen: {e}"))??;
        let mut guard = session.sftp.lock().await;
        let sftp = guard
            .as_mut()
            .expect("ensure_sftp_open lief erfolgreich durch");
        sftp.write_file(&remote_path, &bytes).await?;
        Ok(())
    }
    .await;

    emit_sftp_transfer_finished(
        &app,
        session_id,
        transfer_id,
        result.as_ref().err().map(|e| e.message.clone()),
    );
    result
}

/// Löschen einer Datei — die Bestätigungsrückfrage selbst läuft im Frontend
/// (Spec 0020, Abschnitt 5: "Löschen erfordert eine Bestätigungsrückfrage im
/// UI"), dieser Befehl führt sie nur noch aus. Nur für Dateien angeboten,
/// s. Moduldoc-Kommentar oben ("Design-Entscheidung").
#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    session_id: SessionId,
    path: String,
) -> CommandResult<()> {
    let session = session_sftp(&state, session_id).await?;
    let mut guard = session.sftp.lock().await;
    let sftp = guard
        .as_mut()
        .expect("ensure_sftp_open lief erfolgreich durch");
    sftp.remove(&path).await?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    session_id: SessionId,
    from: String,
    to: String,
) -> CommandResult<()> {
    let session = session_sftp(&state, session_id).await?;
    let mut guard = session.sftp.lock().await;
    let sftp = guard
        .as_mut()
        .expect("ensure_sftp_open lief erfolgreich durch");
    sftp.rename(&from, &to).await?;
    Ok(())
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    session_id: SessionId,
    path: String,
) -> CommandResult<()> {
    let session = session_sftp(&state, session_id).await?;
    let mut guard = session.sftp.lock().await;
    let sftp = guard
        .as_mut()
        .expect("ensure_sftp_open lief erfolgreich durch");
    sftp.create_dir(&path).await?;
    Ok(())
}

#[cfg(test)]
mod local_server_tests {
    //! Spec 0032, Abschnitt 3: `list_servers()` enthält den lokalen
    //! Pseudo-Server immer als erstes Element, unabhängig vom
    //! `group_id`-Filter — s. `list_servers_impl`.

    use ssh_manager_core::profiles::{AuthMethod, GroupId, Server};
    use ssh_manager_core::shared::ServerId;

    use crate::first_run_notice::test_support::{lock_async, test_app};
    use crate::local_server::LOCAL_SERVER_ID;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};

    use super::*;

    fn dummy_server(name: &str, group_id: Option<GroupId>) -> Server {
        let now = Utc::now();
        Server {
            id: ServerId::new(),
            name: name.to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "user".to_string(),
            group_id,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
            notes: String::new(),
            jump_host: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn test_list_servers_always_has_local_pseudo_server_first_regardless_of_filter() {
        let _guard = lock_async().await;
        let app = test_app();
        let handle = app.handle().clone();

        let group_id = GroupId::new();
        let profile_store = InMemoryProfileStore::new()
            .with_server(dummy_server("alpha", None))
            .with_server(dummy_server("beta", Some(group_id)));
        let credential_store = InMemoryCredentialStore::new();

        // Ohne Filter.
        let all = list_servers_impl(&handle, &profile_store, &credential_store, None)
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, LOCAL_SERVER_ID.0.to_string());
        assert!(all[0].is_local);
        assert!(all[1..].iter().all(|s| !s.is_local));

        // Mit einem Gruppenfilter, der den lokalen Server nicht treffen
        // könnte (er hat keine Gruppe) — er muss trotzdem als erstes
        // Element vorhanden bleiben.
        let filtered =
            list_servers_impl(&handle, &profile_store, &credential_store, Some(group_id))
                .await
                .unwrap();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, LOCAL_SERVER_ID.0.to_string());
        assert!(filtered[0].is_local);
        assert_eq!(filtered[1].name, "beta");
    }

    /// Regressionstest für den unabhängigen Review-Pass (docs/adr/0026):
    /// `build_session_system_context` fiel für den lokalen Pseudo-Server
    /// bislang immer in den "Server nicht gefunden"-Fallback, weil
    /// `profile_store.get_server(LOCAL_SERVER_ID)` per Definition nie eine
    /// Zeile findet — Notizen blieben dadurch dauerhaft leer, obwohl über
    /// `local_server::save_notes` welche hinterlegt waren.
    #[tokio::test]
    async fn test_build_session_system_context_includes_local_server_notes() {
        let _guard = lock_async().await;
        let app = test_app();
        let handle = app.handle().clone();
        crate::local_server::save_notes(&handle, "Docker Compose unter ~/services").unwrap();

        let profile_store = InMemoryProfileStore::new();
        let dir = tempfile::tempdir().expect("Temp-Verzeichnis sollte anlegbar sein");
        let policy_store = persistence_sqlite::SqliteProfileStore::connect(
            &dir.path().join("test.db"),
        )
        .await
        .expect("frische SQLite-Datenbank mit angewendeten Migrationen sollte immer aufbaubar sein")
        .policy_store();

        let context = build_session_system_context(
            &handle,
            "Localhost",
            &LOCAL_SERVER_ID,
            &[],
            None,
            &profile_store,
            &policy_store,
        )
        .await;

        assert!(
            context.contains("Docker Compose unter ~/services"),
            "Notizen des lokalen Pseudo-Servers müssen im System-Kontext landen, war: {context}"
        );

        crate::local_server::save_notes(&handle, "").unwrap();
    }

    #[test]
    fn test_reject_local_jump_host_rejects_local_id_but_allows_others_and_none() {
        assert!(reject_local_jump_host(Some(LOCAL_SERVER_ID)).is_err());
        assert!(reject_local_jump_host(Some(ServerId::new())).is_ok());
        assert!(reject_local_jump_host(None).is_ok());
    }
}
