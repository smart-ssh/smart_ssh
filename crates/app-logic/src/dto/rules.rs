//! DTOs: rules.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use persistence_sqlite::StoredRule;
use ssh_manager_core::filter::{
    Decision, EvalContext, EvaluationTrace, Pattern, RuleAction, RuleId, Scope,
};
use ssh_manager_core::shared::ServerId;

/// Getrennt von `pattern_value` statt eines verschachtelten `Pattern` (Spec
/// 0009, Abschnitt 3 nennt `pattern_type`/`pattern_value` als eigene Felder
/// in `RuleInput`) — passt zum Formular aus Abschnitt 6: eine
/// Typ-Auswahl bestimmt, welches einzelne Eingabefeld angezeigt wird, zwei
/// flache Felder bilden das direkter ab als ein getaggtes Enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    Glob,
    Regex,
    Exact,
}

impl From<&Pattern> for PatternType {
    fn from(pattern: &Pattern) -> Self {
        match pattern {
            Pattern::Glob(_) => PatternType::Glob,
            Pattern::Regex(_) => PatternType::Regex,
            Pattern::Exact(_) => PatternType::Exact,
        }
    }
}

/// `pub(crate)`: Spec 0077, 3.1.5 — `rule_suggestions` baut damit dasselbe
/// [`Pattern`] wie das Anlegen, um einen Vorschlag zu prüfen, statt die
/// Zuordnung Typ → Variante ein zweites Mal nachzubilden.
pub(crate) fn pattern_from_parts(pattern_type: PatternType, pattern_value: String) -> Pattern {
    match pattern_type {
        PatternType::Glob => Pattern::Glob(pattern_value),
        PatternType::Regex => Pattern::Regex(pattern_value),
        PatternType::Exact => Pattern::Exact(pattern_value),
    }
}

/// Formular-Eingabe für `create_rule`/`update_rule` (Spec 0009, Abschnitt
/// 3). `action`/`scope` sind bewusst direkt `core::filter::{RuleAction,
/// Scope}` statt separater `ScopeInput`-artiger Typen, wie die Spec-Skizze
/// nahelegt: beide sind bereits `Serialize + Deserialize` und werden
/// bereits unverändert über dieselbe IPC-Grenze gereicht (z. B. `Decision`
/// in `chat-action-proposed`) — ein strukturell identischer Zweit-Typ hätte
/// hier keinen Mehrwert. Ebenso wird die in der Spec-Skizze separat
/// benannte `ScopeFilter` (mit zusätzlichem `All`-Wert) nicht eingeführt:
/// `list_rules(scope_filter: Option<Scope>)` deckt "alle Regeln" bereits
/// über `None` ab, ein zusätzlicher `All`-Wert wäre redundant. Siehe
/// ADR-Vorschlag am Ende der Aufgabe.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub pattern_type: PatternType,
    pub pattern_value: String,
    pub action: RuleAction,
    pub scope: Scope,
    pub priority: i32,
}

impl RuleInput {
    pub fn into_stored_rule(
        self,
        id: RuleId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> StoredRule {
        StoredRule {
            id,
            pattern: pattern_from_parts(self.pattern_type, self.pattern_value),
            action: self.action,
            scope: self.scope,
            priority: self.priority,
            created_at,
            updated_at,
        }
    }
}

/// Sicht auf eine gespeicherte Regel für Liste und Bearbeiten-Formular
/// (Spec 0009, Abschnitt 3/6) — spiegelt `RuleInput`s Feldaufteilung
/// (`pattern_type`/`pattern_value` statt `pattern`), damit sich eine
/// `RuleDto` ohne Umformung direkt als Formular-Vorbefüllung eignet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDto {
    pub id: RuleId,
    pub pattern_type: PatternType,
    pub pattern_value: String,
    pub action: RuleAction,
    pub scope: Scope,
    pub priority: i32,
    /// Spec 0077, 3.2.3: Fehlertext, wenn sich das Muster dieser Regel
    /// nicht übersetzen lässt — sonst `None`. Eine solche Regel kann bei
    /// der Auswertung nicht greifen, soweit ihr Muster nicht übersetzt;
    /// die Liste macht das sichtbar, statt sie wie eine wirksame Regel
    /// anzuzeigen.
    ///
    /// Bewusst **kein** `skip_serializing_if`: Das Frontend soll das Feld
    /// immer sehen (`null` statt fehlend), damit ein vergessenes Befüllen
    /// nicht wie „Muster in Ordnung" aussieht.
    pub pattern_error: Option<String>,
}

impl From<&StoredRule> for RuleDto {
    fn from(rule: &StoredRule) -> Self {
        Self {
            id: rule.id.clone(),
            pattern_type: PatternType::from(&rule.pattern),
            pattern_value: rule.pattern.display_text().to_string(),
            action: rule.action.clone(),
            scope: rule.scope.clone(),
            priority: rule.priority,
            // Spec 0077, 3.2.3: aus derselben Prüfung wie Schicht 1 —
            // eine Regel, die heute nicht mehr gespeichert werden könnte,
            // wird in der Liste als solche erkennbar.
            pattern_error: rule.pattern.validate().err().map(|e| e.to_string()),
        }
    }
}

/// Read-only-Anzeige eines Hard-Blacklist-Musters (Spec 0009, Abschnitt 3:
/// `list_hard_blacklist`). Ein flaches `{kind, value}`-Struct statt eines
/// getaggten `Pattern`-Enums — vermeidet die `rename_all_fields`-Klasse von
/// Fehlern (s. `crate::dto::tests`, `AuthMethodInput` u. a.) von vornherein,
/// ganz ohne serde-Sonderfall nötig zu machen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternDto {
    pub kind: &'static str,
    pub value: String,
}

impl From<&Pattern> for PatternDto {
    fn from(pattern: &Pattern) -> Self {
        Self {
            kind: pattern.kind_str(),
            value: pattern.display_text().to_string(),
        }
    }
}

/// Eingabe für `evaluate_explained` (Spec 0009, Abschnitt 3). Eigener Typ
/// statt direkter Wiederverwendung von `core::filter::EvalContext`: Abschnitt
/// 6 verlangt eine **optionale** Server-Simulation im Testen-Panel
/// ("optionale Scope-Simulation"), `EvalContext.server_id` ist aber nicht
/// optional (folgerichtig — in der echten Kernschleife gibt es immer einen
/// verbundenen Server). `None` wird beim Umwandeln auf eine frische
/// `ServerId` abgebildet (kann garantiert keine `Scope::Server`-Regel
/// matchen), statt `EvalContext` selbst mit dieser reinen UI-Rücksicht zu
/// belasten.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalContextInput {
    pub server_id: Option<ServerId>,
    pub tags: Vec<String>,
}

impl From<EvalContextInput> for EvalContext {
    fn from(input: EvalContextInput) -> Self {
        Self {
            server_id: input.server_id.unwrap_or_default(),
            tags: input.tags,
        }
    }
}

/// Sicht auf eine [`EvaluationTrace`] für das Testen-Panel (Spec 0009,
/// Abschnitt 4/6).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationTraceDto {
    pub decision: Decision,
    pub matched_rule: Option<RuleId>,
    pub matched_hard_blacklist_entry: Option<String>,
    pub sub_command_traces: Vec<EvaluationTraceDto>,
    pub kind: ssh_manager_core::filter::TraceKind,
    pub segment_index: Option<usize>,
    pub checks: Vec<String>,
}

impl From<EvaluationTrace> for EvaluationTraceDto {
    fn from(trace: EvaluationTrace) -> Self {
        Self {
            decision: trace.decision,
            matched_rule: trace.matched_rule,
            matched_hard_blacklist_entry: trace.matched_hard_blacklist_entry,
            kind: trace.kind,
            segment_index: trace.segment_index,
            checks: trace.checks,
            sub_command_traces: trace
                .sub_command_traces
                .into_iter()
                .map(EvaluationTraceDto::from)
                .collect(),
        }
    }
}

// --- Spec 0011: Regel-Schnellvorschlag ----------------------------------

/// Art eines Muster-Vorschlags (Spec 0011, Abschnitt 2). Das Backend liefert
/// nur die Art und Parameter, keinen Text: Die Beschriftung baut das
/// Frontend übersetzt daraus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionKind {
    /// Das Kommando selbst, unverändert.
    Exact,
    /// Je Teilkommando einer Verkettung/Pipeline das Kommando selbst.
    ExactPerSegment,
    /// Wie das Kommando, aber der Pfad durch sein Elternverzeichnis plus `*`
    /// ersetzt (Zahlen nach Optionen werden `*`).
    PathScoped,
    /// Wie das Kommando, aber nur das letzte Argument ist frei (`*`).
    LastArgOpen,
    /// Unterkommando mit beliebigen Argumenten (`names`: `tool sub`).
    Subcommand,
    /// Alle Aufrufe eines Programms (`names`: das Programm).
    Tool,
}

/// Ein einzelnes Muster eines Vorschlags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionPatternDto {
    pub pattern_type: PatternType,
    pub pattern_value: String,
}

/// Ein Muster-Vorschlag für den Schnellvorschlag-Dropdown im
/// Bestätigungsdialog (Spec 0011, Abschnitt 2). Ein Vorschlag besteht aus
/// einer **Menge** von Mustern, die zusammen (atomar) angelegt werden.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternSuggestionDto {
    pub kind: SuggestionKind,
    /// Parameter für die Beschriftung (Programme bzw. `tool sub`), ohne
    /// Doppelte, in Reihenfolge des Auftretens. Leer bei `exact*`.
    pub names: Vec<String>,
    pub patterns: Vec<SuggestionPatternDto>,
}

// --- Issue #596: Erklärung des Bestätigungsdialogs -----------------------

/// Zustand eines Teilkommandos im Bestätigungsdialog (Spec 0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SegmentCoverageState {
    /// Eine Allow-Regel lässt das Teilkommando automatisch laufen.
    Covered,
    /// Keine Regel greift; eine Allow-Regel könnte es abdecken.
    NeedsRule,
    /// Verlangt eine Bestätigung, egal welche Allow-Regel existiert
    /// (`reasons`), oder eine Regel verlangt Bestätigung/verbietet es.
    Blocked,
}

/// Die Regel, die ein Teilkommando abdeckt (nur Anzeige).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoveringRuleDto {
    pub pattern_type: PatternType,
    pub pattern_value: String,
}

/// Issue #696: a rule that let (a segment of) an auto-executed command run
/// without confirmation. Display only. `segment_indices` lists the top-level
/// segments the rule covered (one entry for an unsplit command).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoExecRuleDto {
    pub rule_id: RuleId,
    pub pattern_type: PatternType,
    pub pattern_value: String,
    pub scope: Scope,
    pub segment_indices: Vec<usize>,
}

/// Ein Teilkommando mit seinem Zustand. `reasons` sind feste Codes
/// (`FILTER_*`-Codes), keine Texte.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentCoverageDto {
    pub index: usize,
    pub text: String,
    pub state: SegmentCoverageState,
    pub rule: Option<CoveringRuleDto>,
    pub reasons: Vec<String>,
}

/// Grund, warum ein Vorschlag das Kommando nicht automatisch laufen lässt.
/// `code`: `FILTER_NO_RULE_MATCHED` (Teilkommando ohne Regel), `FILTER_RULE_CONFIRM`, `FILTER_RULE_DENY`,
/// ein `FILTER_*`-Code der Engine oder ein Code einer nachgelagerten
/// Eskalation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerdictReasonDto {
    pub code: String,
    pub segment_index: Option<usize>,
}

/// Ergebnis der Auswertung des Kommandos mit den Mustern des Vorschlags als
/// Kandidatenregeln.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionVerdictDto {
    pub auto_exec: bool,
    pub reasons: Vec<VerdictReasonDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainedSuggestionDto {
    #[serde(flatten)]
    pub suggestion: PatternSuggestionDto,
    pub verdict: SuggestionVerdictDto,
}

/// Antwort von `explain_confirmation`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationExplanationDto {
    /// Leer bei einem Teilkommando ohne regelunabhängigen Grund.
    pub segments: Vec<SegmentCoverageDto>,
    pub suggestions: Vec<ExplainedSuggestionDto>,
}

// --- Spec 0012: KI-generierte Dokumente ---------------------------------
