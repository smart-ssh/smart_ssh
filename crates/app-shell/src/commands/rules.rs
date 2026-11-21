//! Spec 0009/0011: Filter-Regel-Verwaltung und Regel-Schnellvorschlag — Teil
//! der Spec-0083-Aufteilung von `commands.rs`.

use tauri::State;

use ssh_manager_core::filter::{hard_blacklist_patterns, RuleId, Scope};

use app_logic::dto::{EvalContextInput, EvaluationTraceDto, PatternDto, RuleDto, RuleInput};
use app_logic::error::CommandResult;
use app_logic::state::{AppState, SessionId};

// --- Spec 0009: Filter-Regel-Verwaltung ------------------------------------

/// `scope_filter: None` liefert alle Regeln (s. `app_logic::filter_rules::list_rules`-
/// Doc-Kommentar zur `ScopeFilter::All`-Vereinfachung).
#[tauri::command]
pub async fn list_rules(
    state: State<'_, AppState>,
    scope_filter: Option<Scope>,
) -> CommandResult<Vec<RuleDto>> {
    app_logic::filter_rules::list_rules(&state.policy_store, scope_filter)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn create_rule(state: State<'_, AppState>, input: RuleInput) -> CommandResult<RuleId> {
    // Spec 0077, 3.1.3: `From<RuleWriteError> for CommandError` setzt den Code.
    app_logic::filter_rules::create_rule(&state.policy_store, input)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn update_rule(
    state: State<'_, AppState>,
    id: RuleId,
    input: RuleInput,
) -> CommandResult<()> {
    app_logic::filter_rules::update_rule(&state.policy_store, id, input)
        .await
        .map_err(Into::into)
}

/// Spec 0077, 3.2.3: tauscht die Prioritäten zweier Regeln in einer
/// Transaktion; prüft vorher beide Muster.
#[tauri::command]
pub async fn swap_rule_priorities(
    state: State<'_, AppState>,
    first: RuleId,
    second: RuleId,
) -> CommandResult<()> {
    app_logic::filter_rules::swap_rule_priorities(&state.policy_store, first, second)
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
    Ok(app_logic::filter_rules::evaluate_explained(state.policy_store.clone(), command, ctx).await)
}

// --- Spec 0011: Regel-Schnellvorschlag im Bestätigungsdialog ---------------

/// Issue #596, Spec 0011: coverage per segment, suggestions for the
/// uncovered segments and a verdict per suggestion for the pending command
/// of `session_id`, with the suggestions' patterns as candidate rules in
/// `scope`. Read-only: nothing is stored. Server and tags come from the
/// session, not from the frontend.
#[tauri::command]
pub async fn explain_confirmation(
    state: State<'_, AppState>,
    session_id: SessionId,
    command: String,
    scope: Scope,
) -> CommandResult<app_logic::dto::ConfirmationExplanationDto> {
    let session = app_logic::confirmation_explain::session_for_explain(
        &state.sessions,
        session_id,
        &command,
    )?;
    let stored = app_logic::filter_rules::list_rules(&state.policy_store, None)
        .await
        .map_err(|e| {
            let err = app_logic::error::CommandError::from(e);
            app_logic::confirmation_explain::log_explain_failure(&err, session_id, &command);
            err
        })?;
    Ok(app_logic::confirmation_explain::explain_confirmation(
        &session,
        state.profile_store.as_ref(),
        &stored,
        &command,
        scope,
        None,
    )
    .await)
}

/// Issue #1356, Spec 0011: rule suggestions for ONE segment of the pending
/// `command` (index as in `SegmentCoverageDto.index`) in `scope`, with a
/// segment verdict and a command verdict per suggestion, given the rules the
/// user has staged. Read-only: nothing is stored. Server and tags come from
/// the session.
#[tauri::command]
pub async fn explain_segment_rules(
    state: State<'_, AppState>,
    session_id: SessionId,
    command: String,
    segment_index: usize,
    scope: Scope,
    staged: Vec<app_logic::dto::StagedRuleDto>,
) -> CommandResult<app_logic::dto::SegmentRuleSuggestionsDto> {
    let session = app_logic::confirmation_explain::session_for_explain(
        &state.sessions,
        session_id,
        &command,
    )?;
    app_logic::confirmation_explain::explain_segment_rules_for_session(
        &session,
        state.profile_store.as_ref(),
        &command,
        segment_index,
        scope,
        &staged,
    )
    .await
    .inspect_err(|e| {
        app_logic::confirmation_explain::log_explain_failure(e, session_id, &command);
    })
}

/// Issue #1356, Spec 0011: saves the staged rules (each with its own
/// patterns and scope) in one transaction, all or nothing. Does not touch
/// the pending confirmation.
#[tauri::command]
pub async fn create_quick_rules_batch(
    state: State<'_, AppState>,
    rules: Vec<app_logic::dto::StagedRuleDto>,
) -> CommandResult<Vec<RuleId>> {
    app_logic::quick_rules_batch::create_quick_rules_batch(&state.policy_store, rules)
        .await
        .map_err(Into::into)
}
