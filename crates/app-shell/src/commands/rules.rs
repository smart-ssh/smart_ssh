//! Spec 0009/0011: Filter-Regel-Verwaltung und Regel-Schnellvorschlag — Teil
//! der Spec-0083-Aufteilung von `commands.rs`.

use tauri::State;

use ssh_manager_core::filter::{hard_blacklist_patterns, RuleId, Scope};

use crate::dto::{
    ActionUserDecision, EvalContextInput, EvaluationTraceDto, PatternDto, PatternSuggestionDto,
    PatternType, RuleDto, RuleInput,
};
use crate::error::CommandResult;
use crate::state::{ActionId, AppState, SessionId};

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
    // Spec 0077, 3.1.3: ausdrücklich umwandeln, nie `.map_err(Into::into)` —
    // der blanket `From<E: Display>` würde den Code still verschlucken.
    crate::filter_rules::create_rule(&state.policy_store, input)
        .await
        .map_err(crate::error::rule_write_error)
}

#[tauri::command]
pub async fn update_rule(
    state: State<'_, AppState>,
    id: RuleId,
    input: RuleInput,
) -> CommandResult<()> {
    // Spec 0077, 3.1.3: s. `create_rule`.
    crate::filter_rules::update_rule(&state.policy_store, id, input)
        .await
        .map_err(crate::error::rule_write_error)
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

/// Spec 0011, Abschnitt 3: versucht zuerst, die Regel anzulegen (Schritt 1,
/// delegiert an [`crate::filter_rules::create_rule`] über
/// [`crate::rule_suggestions::create_quick_rule`]), löst **danach** die
/// wartende `Confirm`-Entscheidung für `action_id` auf (Schritt 2).
///
/// Schlägt Schritt 1 fehl, wird Schritt 2 trotzdem erreicht: Das Ergebnis
/// von Schritt 1 wird zwischengespeichert und erst am Ende zurückgegeben,
/// damit ein Fehlschlag beim Anlegen der Regel die Bestätigung nicht
/// mitreißt (Spec 0021, Abschnitt 7 — Begründung im Kommentar an der
/// `resolve`-Stelle weiter unten). Der Nutzer bekommt dann den Fehler der
/// Regel-Erstellung, und die bestätigte Aktion läuft trotzdem.
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
    let rule_result = crate::rule_suggestions::create_quick_rule(
        &state.policy_store,
        pattern_type,
        pattern_value,
        scope,
        priority,
    )
    .await;

    // Unabhängiger Review-Pass (Spec 0021, Abschnitt 7): die Bestätigung
    // muss UNABHÄNGIG vom Erfolg der Regel-Erstellung aufgelöst werden.
    // Vorher lief `create_quick_rule(...).await?` zuerst — schlug das fehl
    // (DB-Fehler, gesperrte Datei), kehrte der Command mit `Err` zurück,
    // OHNE die Bestätigung je aufzulösen. Das Frontend hatte die Karte zu
    // diesem Zeitpunkt aber schon optimistisch als beantwortet markiert
    // (kein erneuter Versuch möglich) — `handle_action_proposed` wartete
    // dann für den Rest der Session ergebnislos auf `rx.await`, Eingabefeld
    // und Senden-Button blieben dauerhaft gesperrt. Genau der Fail-Safe-
    // Verstoß, den Abschnitt 7 verhindern sollte. Die Aktion selbst wird
    // jetzt immer aufgelöst (der Nutzer hat sie explizit bestätigt); ein
    // Fehlschlag der Regel-Erstellung wird separat als Fehler zurückgegeben,
    // statt die Bestätigung mitzureißen.
    let decision = match edited_command {
        Some(command) => ActionUserDecision::EditThenApprove { command },
        None => ActionUserDecision::Approve,
    };
    state
        .pending_action_confirmations
        .resolve(&action_id, decision)?;

    // Spec 0077, 3.1.3: auch hier ausdrücklich umwandeln, damit die
    // Schnellregel denselben Code liefert wie das Formular. Der Fehlerweg
    // bleibt wie bisher: Die Bestätigung ist oben schon aufgelöst, der
    // Fehler der Regel-Erstellung kommt getrennt zurück (3.1.2).
    rule_result.map_err(crate::error::rule_write_error)
}
