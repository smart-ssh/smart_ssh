//! Spec 0008/0057/0058: Notizen (Server/Gruppe, Kürzungs-Vorschlag) — Teil
//! der Spec-0083-Aufteilung von `commands.rs`.

use secrecy::ExposeSecret;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use ssh_manager_core::ai::{DefaultOutputRedactor, OutputRedactor};
use ssh_manager_core::profiles::{
    effective_notes, record_revision, GroupId, NoteEditor, NoteTarget,
};
use ssh_manager_core::shared::ServerId;

use crate::event_emitter::TauriEventEmitter;
use app_logic::ai_provider_factory::build_ai_provider;
use app_logic::dto::NoteRevisionDto;
use app_logic::error::{keychain_aware_credential_error, CommandResult};
use app_logic::orchestration::execute_note_shrink_request;
use app_logic::server_credentials::sudo_password_credential_ref;
use app_logic::state::{AppState, SessionId};

use super::ai_providers::active_ai_provider_config;

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

/// Spec 0058, Teil 1 (Etappe 5): derselbe Schwellwert wie der
/// Sitzungsende-Kürzungs-Dialog (`orchestration::LARGE_NOTE_DIALOG_
/// THRESHOLD_CHARS`, Etappe 4) — eine Quelle der Wahrheit statt einer
/// zweiten, im Frontend hartkodierten Zahl. Reiner, niemals fehlschlagender
/// Konstanten-Getter (kein `State`/`AppHandle` nötig), trotzdem als
/// `async fn` mit `CommandResult`, konsistent mit jedem anderen Befehl in
/// dieser Datei (das Frontend ruft ohnehin immer über ein `Promise`-
/// basiertes `invoke()` auf).
#[tauri::command]
pub async fn large_note_dialog_threshold_chars() -> CommandResult<u32> {
    Ok(app_logic::orchestration::LARGE_NOTE_DIALOG_THRESHOLD_CHARS as u32)
}

/// Spec 0057, §4.2 (Etappe 4): "Ja, zusammenfassen" — ausgelöst vom
/// Kürzungs-Vorschlags-Dialog (`note-shrink-suggested`), potenziell lange
/// nach dem `disconnect()`, das ihn ursprünglich zeigte. Baut deshalb einen
/// FRISCHEN `AiProvider` aus der aktuell aktiven Provider-Konfiguration —
/// derselbe Aufbau-Pfad wie `connect()`/`test_ai_provider_credentials` —
/// statt sich auf eine (zu diesem Zeitpunkt typischerweise längst
/// beendete) `Session` zu verlassen (s. `orchestration::summarize_note_
/// for_shrink`-Doc-Kommentar). Der API-Key wird hier, synchron in diesem
/// Befehl, EINMALIG aus dem `CredentialStore` gelesen (Spec 0022, Abschnitt
/// 3 — dieselbe Garantie wie bei jedem anderen `build_ai_provider`-Aufruf)
/// und danach nur noch als Teil der fertigen `AiProvider`-Instanz in den
/// Hintergrund-Task verschoben.
///
/// Läuft selbst als eigener `tokio::spawn`-Task (überlebt Navigation weg
/// vom auslösenden Dialog — derselbe Grund wie bei `disconnect()`s
/// Notiz-Vorschlag-Task, Spec 0010 Abschnitt 2 Punkt 6) und mündet bei
/// Erfolg in EXAKT denselben `note-update-suggested`/`NoteSuggestionToast`/
/// `ConfirmationRegistry`-Ablauf wie ein regulärer KI-Notiz-Vorschlag
/// (Spec 0003/0023) — keine zweite, parallele Diff-UI für dieselbe Sache.
/// Der Befehl selbst kehrt sofort zurück, sobald der Task gestartet ist;
/// Erfolg/Fehlschlag des eigentlichen KI-Aufrufs kommen ausschließlich über
/// `note-update-suggested`/`note-shrink-failed` beim Frontend an.
#[tauri::command]
pub async fn request_note_shrink(
    app: AppHandle,
    state: State<'_, AppState>,
    server_id: ServerId,
) -> CommandResult<()> {
    let active_config = active_ai_provider_config(&state).await?;
    let api_key = state
        .credential_store
        .get(&active_config.credential_ref)
        .map_err(|err| keychain_aware_credential_error(err, state.keychain))?;
    let (ai_provider, ai_provider_budget) = build_ai_provider(
        &state.rate_limit_registry,
        active_config.provider_type,
        active_config.base_url.as_deref(),
        &active_config.model,
        api_key,
        active_config.supports_native_tool_calling,
        active_config.extra_headers.clone(),
        active_config.max_tokens_override,
    );
    let provider_label = active_config.display_name.clone();
    let model = active_config.model.clone();
    // spec-reviewer-Fund (Review dieses Schritts): derselbe zusätzliche
    // Redactor-Musterbau wie in `connect()` (s. dortiger Kommentar) — ein
    // per "In Notiz übernehmen"/einem angenommenen KI-Notiz-Vorschlag in
    // die Notiz gelangtes Sudo-Passwort dieses Servers (z. B. aus einem
    // NOPASSWD-/gültiger-Sudo-Timestamp-Fall, der es unredigiert in eine
    // Kommandoausgabe hätte durchreichen lassen) wäre sonst hier nicht
    // erfasst. `sudo_password_credential_ref`/`get(...).ok()` wie dort:
    // kein hinterlegtes Passwort ist kein harter Fehler.
    let sudo_password = state
        .credential_store
        .get(&sudo_password_credential_ref(server_id))
        .ok();
    let redactor: Box<dyn OutputRedactor> = match &sudo_password {
        Some(password) => match regex::Regex::new(&regex::escape(password.expose_secret())) {
            Ok(pattern) => Box::new(DefaultOutputRedactor::with_extra_patterns(vec![pattern])),
            Err(_) => Box::new(DefaultOutputRedactor::new()),
        },
        None => Box::new(DefaultOutputRedactor::new()),
    };

    tokio::spawn(async move {
        let state = app.state::<AppState>();
        let session_id: SessionId = Uuid::new_v4();
        // Spec 0058, Teil 2: derselbe `is_local`-Verzweigungs-Idiom wie
        // `commands::disconnect`s Kürzungs-Vorschlag oben und
        // `build_session_system_context` — der lokale Pseudo-Server braucht
        // eine grundsätzlich andere Persistenz (`local_server::save_notes`
        // statt `ProfileStore::record_note_revision`, s. `orchestration::
        // NoteShrinkTarget`-Doc-Kommentar).
        if app_logic::dto::is_local(server_id) {
            let target = crate::local_server::LocalNoteShrinkTarget { app: app.clone() };
            execute_note_shrink_request(
                session_id,
                server_id,
                ai_provider.as_ref(),
                &ai_provider_budget,
                redactor.as_ref(),
                &TauriEventEmitter(app.clone()),
                &target,
                &state.pending_action_confirmations,
            )
            .await;
        } else {
            let target = app_logic::orchestration::ProfileStoreNoteShrinkTarget {
                profile_store: state.profile_store.as_ref(),
                server_id,
                provider_label,
                model,
            };
            execute_note_shrink_request(
                session_id,
                server_id,
                ai_provider.as_ref(),
                &ai_provider_budget,
                redactor.as_ref(),
                &TauriEventEmitter(app.clone()),
                &target,
                &state.pending_action_confirmations,
            )
            .await;
        }
    });

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
