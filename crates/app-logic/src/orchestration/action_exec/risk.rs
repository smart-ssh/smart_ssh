//! Split out of `action_exec.rs` (issue #859, pure move).

use super::*;

use ssh_manager_core::filter::{
    exceeds_command_length_limit, Decision, DEFAULT_MAX_COMMAND_LENGTH,
};
use ssh_manager_core::profiles::{AiAction, PostIngestPolicy};
use ssh_manager_core::risk::{RiskAssessment, RiskClassifier, RiskLevel, RuleBasedRiskClassifier};

use crate::session::Session;
use crate::state::SessionId;

/// Issue #688 / Spec 0094: name of a decision's kind, for log lines.
pub(super) fn decision_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::AutoExec => "AutoExec",
        Decision::Confirm { .. } => "Confirm",
        Decision::Deny { .. } => "Deny",
    }
}

/// Issue #688: logs `filter decision escalated` when the post-engine chain
/// changed the engine decision. Only kinds, the escalation code, the action
/// kind, the session id and the byte length — no command, path or reason.
pub(super) fn log_filter_escalation(
    session_id: SessionId,
    action: &AiAction,
    engine_decision: &Decision,
    final_decision: &Decision,
) {
    if matches!(engine_decision, Decision::AutoExec) && matches!(final_decision, Decision::AutoExec)
    {
        return;
    }
    // The chain only ever turns `AutoExec` into `Confirm`.
    if !matches!(engine_decision, Decision::AutoExec) {
        return;
    }
    let code = match final_decision {
        Decision::Confirm { code, .. } | Decision::Deny { code, .. } => code.as_str(),
        Decision::AutoExec => return,
    };
    let action_kind = match action {
        AiAction::SuggestCommand { .. } => "command",
        AiAction::ReadRemoteFile { .. } => "file_read",
        AiAction::WriteRemoteFile { .. } => "file_write",
        AiAction::ProposeNoteUpdate { .. } => "note_update",
        AiAction::GenerateDocument { .. } => "document",
        AiAction::AskUser { .. } => "ask_user",
    };
    tracing::info!(
        session_id = %session_id,
        engine_decision = decision_kind(engine_decision),
        final_decision = decision_kind(final_decision),
        code,
        action_kind,
        command_len = pseudo_command_for_risk_classification(action).map_or(0, |c| c.len()),
        "filter decision escalated",
    );
}

/// Test helper: the decision the secret-path / `sftp-server` step produces for
/// `finding`, via the shared `escalate` (the former stand-alone function).
#[cfg(test)]
pub(super) fn check_finding_confirm(
    finding: ssh_manager_core::risk::CommandCheckFinding,
    match_code: &str,
    injection_suspected: bool,
    injection_reason: Option<&str>,
) -> Decision {
    let mut facts = super::super::escalation::EscalationFacts {
        injection_suspected,
        injection_reason: injection_reason.map(str::to_string),
        ..Default::default()
    };
    if match_code == "FILTER_SFTP_SERVER_REQUIRES_CONFIRM" {
        facts.sftp_server = Some(finding);
    } else {
        facts.secret_path = Some(finding);
    }
    super::super::escalation::escalate(Decision::AutoExec, &facts).decision
}

/// Reads the session state the escalation chain depends on (read-only: the
/// one-shot flags are loaded, never consumed). `mcp_origin` and
/// `earlier_rejection` are transient and left `false`; the caller sets them.
pub(super) fn escalation_facts(
    session: &Session,
    action: &AiAction,
    risk_assessment: Option<&RiskAssessment>,
    transform_scope: Option<&ssh_manager_core::outgoing::ServerScope>,
) -> super::super::escalation::EscalationFacts {
    use std::sync::atomic::Ordering::SeqCst;
    let pseudo = pseudo_command_for_risk_classification(action);
    let post_ingest_escalates = session.untrusted_content_ingested.load(SeqCst) && {
        let is_modifying_action = risk_assessment.is_some_and(|r| r.server_risk != RiskLevel::None);
        match session.post_ingest_policy {
            PostIngestPolicy::Strict => true,
            PostIngestPolicy::Balanced => is_modifying_action,
            PostIngestPolicy::Standard => false,
        }
    };
    super::super::escalation::EscalationFacts {
        secret_path: pseudo
            .as_deref()
            .and_then(ssh_manager_core::risk::secret_path_read_reason),
        sftp_server: pseudo
            .as_deref()
            .and_then(ssh_manager_core::risk::sftp_server_invocation_reason),
        red_risk_reason: if session.red_risk_always_confirm {
            red_risk_confirm_reason(action, risk_assessment)
        } else {
            None
        },
        post_ingest_escalates,
        injection_suspected: session.injection_suspected.load(SeqCst),
        injection_reason: peek_injection_reason(session),
        injection_check_unavailable: session.injection_check_unavailable.load(SeqCst),
        mcp_origin: false,
        uses_stored_sudo_password: uses_stored_sudo_password(session, action),
        transform_marks_text: transform_requires_confirmation(session, transform_scope, action),
        earlier_rejection: false,
        server_tags_unavailable: false,
    }
}

/// Issue #596: the escalation code that does not depend on the user's choice,
/// for the verdict of a rule suggestion: the code of the step that would turn
/// an `AutoExec` into a `Confirm` for `command` (chat origin, no earlier
/// rejection), or `None`. Runs the same [`escalate`](super::super::escalation::escalate)
/// as the decision path, so the codes are identical (an injection suspicion
/// shows the injection code for a secret-path hit, like the main chain). It
/// never consumes a one-shot flag: the returned `Escalation` is only read.
pub(crate) fn state_independent_escalation_code(
    session: &Session,
    command: &str,
    transform_scope: Option<&ssh_manager_core::outgoing::ServerScope>,
) -> Option<&'static str> {
    let action = AiAction::SuggestCommand {
        command: command.to_string(),
    };
    let risk_assessment = risk_assessment_for_action(&action);
    let facts = escalation_facts(session, &action, risk_assessment.as_ref(), transform_scope);
    super::super::escalation::escalate(Decision::AutoExec, &facts).code
}

/// Issue #815: asks the registered outgoing transform (pure query) whether it
/// will change the text of `action`. `false` without a transform or scope.
pub(super) fn transform_requires_confirmation(
    session: &Session,
    scope: Option<&ssh_manager_core::outgoing::ServerScope>,
    action: &AiAction,
) -> bool {
    use ssh_manager_core::outgoing::OutgoingKind;
    let (Some(transform), Some(scope)) = (session.outgoing_transform.as_ref(), scope) else {
        return false;
    };
    match action {
        AiAction::SuggestCommand { command } => {
            transform.requires_confirmation(scope, OutgoingKind::Command, command)
        }
        AiAction::WriteRemoteFile { content, .. } => {
            transform.requires_confirmation(scope, OutgoingKind::FileContent, content)
        }
        _ => false,
    }
}

pub(super) fn risk_assessment_for_action(action: &AiAction) -> Option<RiskAssessment> {
    let pseudo_command = pseudo_command_for_risk_classification(action)?;
    Some(RuleBasedRiskClassifier.classify(&pseudo_command))
}

/// Spec 0092, A2: der Grund, mit dem das neue Glied eskaliert — `None`, wenn
/// es nicht greifen soll.
///
/// **Fail-safe bei Überlänge** (spec-reviewer-Fund, Runde 1, adversarialer
/// Fall 7): Der Klassifizierer bricht ab, sobald das Pseudokommando länger
/// als [`DEFAULT_MAX_COMMAND_LENGTH`] Bytes ist, und liefert dann
/// `None`/`None` — „kein Risiko erkannt" heißt dort „nicht geprüft". Früher
/// zählte die Filter-Engine Zeichen statt Bytes, und ein Kommando mit
/// Mehrbyte-Zeichen konnte unter ihrer Schranke liegen (mit Allow-Regel also
/// `AutoExec` werden), während der Klassifizierer schon aufgegeben hatte.
/// Seit Issue #110 messen beide über [`exceeds_command_length_limit`]
/// dasselbe, die Filter-Engine verlangt in diesem Fall also selbst schon
/// `Confirm`. Der Zweig bleibt trotzdem als Tiefenverteidigung: Das
/// Pseudokommando ist nicht immer das Kommando, das die Filter-Engine sieht,
/// und „nicht geprüft" darf hier nie als „unauffällig" durchgehen. Derselbe
/// Helfer und dieselbe Schranke wie im Klassifizierer, damit genau das
/// Fenster abgedeckt ist, in dem er aussteigt.
pub(super) fn red_risk_confirm_reason(
    action: &AiAction,
    assessment: Option<&RiskAssessment>,
) -> Option<String> {
    if let Some(pseudo_command) = pseudo_command_for_risk_classification(action) {
        if exceeds_command_length_limit(&pseudo_command, DEFAULT_MAX_COMMAND_LENGTH) {
            return Some(
                "Kommando zu lang für eine Risiko-Einschätzung – wird wie rot behandelt"
                    .to_string(),
            );
        }
    }
    red_risk_reason(assessment)
}

/// Spec 0092, A2.1: `Some`, wenn **eine** der beiden Achsen `Red` ist — mit
/// einem Grund, der die rote Achse und deren Begründung nennt (beide, falls
/// beide rot sind). `None` für „keine Einschätzung vorhanden" (`ProposeNote
/// Update`/`GenerateDocument`, s. `pseudo_command_for_risk_classification`)
/// und für alles unter `Red`.
///
/// Der Text landet in Ledger und KI-Kontext; im Dialog sieht der Nutzer den
/// festen übersetzten Text zum Code und die rote Achse am Badge (Spec 0092,
/// §5, „Anzeige des Grunds").
pub(super) fn red_risk_reason(assessment: Option<&RiskAssessment>) -> Option<String> {
    let assessment = assessment?;
    let axis = |level: RiskLevel, label: &str, reason: Option<&String>| {
        (level == RiskLevel::Red).then(|| match reason {
            Some(reason) => format!("{label}: {reason}"),
            // Der Klassifizierer liefert zu einem `Red` immer eine
            // Begründung; fehlt sie doch, wird trotzdem eskaliert statt die
            // Eskalation an einem fehlenden Text scheitern zu lassen.
            None => format!("{label}: rot eingestuft"),
        })
    };
    let parts: Vec<String> = [
        axis(
            assessment.server_risk,
            "Server-Risiko rot",
            assessment.server_risk_reason.as_ref(),
        ),
        axis(
            assessment.data_risk,
            "Daten-Risiko rot",
            assessment.data_risk_reason.as_ref(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Spec 0026, Abschnitt 3: "Nur Eskalation, nie Abschwächung" — als reine
/// Funktion getrennt von der Event-/Async-Maschinerie um
/// `fetch_second_opinion`, damit genau dieser Fall (ein regelbasiertes
/// `Red` bleibt `Red`, egal was die KI zurückgibt) direkt und ohne
/// Event-Mitschnitt testbar ist.
pub(super) fn escalate_data_risk(
    rule_based_level: RiskLevel,
    rule_based_reason: Option<String>,
    second_opinion: Option<(RiskLevel, String)>,
) -> (RiskLevel, Option<String>) {
    match second_opinion {
        Some((ai_level, ai_reason)) if ai_level > rule_based_level => (ai_level, Some(ai_reason)),
        _ => (rule_based_level, rule_based_reason),
    }
}
