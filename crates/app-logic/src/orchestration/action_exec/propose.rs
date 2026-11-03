//! Split out of `action_exec.rs` (issue #859, pure move).

use super::*;
use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, RejectionReason, Role};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent};
use ssh_manager_core::filter::{Decision, EvalContext, EvaluationTrace};
use ssh_manager_core::profiles::{AiAction, ProfileStore};
use ssh_manager_core::risk::{RiskAssessment, RiskLevel};
use ssh_manager_core::ssh::CommandOutput;

use crate::confirmation::ConfirmationRegistry;
use crate::confirmation_settled::emit_settled_for_rejection;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{
    emit_action_decision_escalated, emit_chat_action_proposed, emit_chat_action_result,
    emit_risk_assessment_updated, ActionResultPayload, EventEmitter,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::super::approval::ApprovedAction;
use super::super::chat_turn::{
    ledger_source_for_origin, push_history_scoped, wait_for_ai_request_slot,
    wait_for_rate_limit_budget, write_ledger_entry, PENDING_ACTION_CONFIRM_TIMEOUT,
};
use super::super::notes::note_target_preview_for_action;
use super::super::pending_confirmation::{
    PendingConfirmation, ANOTHER_CONFIRMATION_PENDING_MESSAGE,
};
use super::super::remote_files::previous_file_content_for_action;

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
///
/// `earlier_rejection` (Spec 0068, Teil 4, Review-Fund): gemeinsam für alle
/// Aktionen EINER KI-Antwort. Wird hier gesetzt, sobald eine Aktion
/// blockiert (`Deny`) oder vom Nutzer abgelehnt wird; ist es gesetzt, wird
/// jede weitere Aktion derselben Antwort von `AutoExec` auf `Confirm`
/// eskaliert — sie kann inhaltlich auf der abgelehnten aufbauen.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_action_proposed(
    session: &Session,
    session_id: SessionId,
    mut action: AiAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    origin: ActionOrigin,
    earlier_rejection: &std::sync::atomic::AtomicBool,
) -> bool {
    // Spec 0040, Abschnitt 4: MCP-Herkunft schreibt nie in die persistierte,
    // wiederaufnehmbare Historie. Einmal aus `origin` abgeleitet und durch
    // die gesamte Ausführungskette gereicht.
    let persist = !matches!(origin, ActionOrigin::Mcp { .. });
    let ctx = ProposeCtx {
        session,
        session_id,
        emitter,
        profile_store,
        action_confirmations,
        origin,
        earlier_rejection,
        action_id: Uuid::new_v4(),
        persist,
    };

    prepare_action(&ctx, &mut action).await;
    let (evaluation, server_tags_unavailable) =
        evaluate_action(session, &action, profile_store).await;
    let escalated = apply_escalations(&ctx, &action, &evaluation, server_tags_unavailable).await;
    let risk_assessment = escalated.risk_assessment;
    let uses_password = escalated.uses_password;

    // Spec 0088, A1.2/A1.4: registration stays BEFORE the awaits for preview
    // and second opinion (a click in between must not be lost).
    let Some(prepared) = prepare_decision(&ctx, escalated.decision.clone()) else {
        return report_confirmation_conflict(&ctx).await;
    };

    let proposed_payload = announce_proposal(
        &ctx,
        &action,
        &evaluation,
        &escalated.decision,
        uses_password,
        risk_assessment.clone(),
    )
    .await;

    let second_opinion = run_second_opinion(&ctx, &action, risk_assessment).await;

    let Ok(prepared) =
        escalate_after_second_opinion(&ctx, prepared, &second_opinion, proposed_payload)
    else {
        return report_confirmation_conflict(&ctx).await;
    };

    let matched_rule = evaluation.matched_rule.clone();
    let matched_rule_origin = evaluation.matched_rule_origin;
    match prepared {
        PreparedDecision::AutoExec if auto_exec_stopped(&ctx) => {
            skip_auto_exec_after_stop(
                session,
                session_id,
                ctx.action_id,
                &action,
                emitter,
                persist,
            )
            .await
        }
        PreparedDecision::AutoExec => {
            run_approved(
                &ctx,
                action,
                matched_rule,
                matched_rule_origin,
                uses_password,
            )
            .await
        }
        PreparedDecision::Deny { reason, code } => {
            report_denied(
                &ctx,
                &action,
                reason,
                code,
                matched_rule,
                matched_rule_origin,
            )
            .await
        }
        PreparedDecision::Confirm {
            reason,
            code,
            pending,
        } => {
            await_confirmation(
                &ctx,
                action,
                pending,
                (reason, code),
                (matched_rule, matched_rule_origin),
                uses_password,
            )
            .await
        }
    }
}

/// Inputs shared by all steps of one `handle_action_proposed` run.
pub(super) struct ProposeCtx<'a> {
    pub(super) session: &'a Session,
    pub(super) session_id: SessionId,
    pub(super) emitter: &'a dyn EventEmitter,
    pub(super) profile_store: &'a dyn ProfileStore,
    pub(super) action_confirmations: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
    pub(super) origin: ActionOrigin,
    pub(super) earlier_rejection: &'a std::sync::atomic::AtomicBool,
    pub(super) action_id: ActionId,
    /// `false` for MCP origin: nothing goes into the persisted history.
    pub(super) persist: bool,
}

impl ProposeCtx<'_> {
    fn is_mcp(&self) -> bool {
        matches!(self.origin, ActionOrigin::Mcp { .. })
    }
}

type MatchedRule = (
    Option<ssh_manager_core::filter::RuleId>,
    Option<ssh_manager_core::filter::RuleOrigin>,
);

/// Result of [`apply_escalations`].
pub(super) struct Escalated {
    pub(super) decision: Decision,
    risk_assessment: Option<RiskAssessment>,
    uses_password: bool,
}

/// Outcome of the second-opinion step (Spec 0026, 0092 A3, issue #102).
pub(super) struct SecondOpinionOutcome {
    pub(super) data_risk: Option<RiskLevel>,
    pub(super) unavailable: bool,
}

/// How a finished confirmation wait is to be treated.
#[derive(Debug)]
pub(super) enum ConfirmationSettlement {
    Decided(ActionUserDecision, RejectionReason),
    /// Sender dropped (e.g. app shutting down): nothing runs.
    SenderDropped,
}

/// Step 1 — prepare. Input: the raw proposal. Output: `action` with its
/// remote path normalised exactly once; the "command proposed" ledger entry
/// (Spec 0057) is written. Safety property: every later consumer (filter,
/// risk, preview, execution) sees the same normalised path (Spec 0020).
pub(super) async fn prepare_action(ctx: &ProposeCtx<'_>, action: &mut AiAction) {
    announce_second_opinion_setup_failure(ctx.session, ctx.session_id, ctx.emitter);
    if let AiAction::SuggestCommand { command } = &*action {
        write_ledger_entry(
            ctx.session,
            ledger_source_for_origin(&ctx.origin),
            LedgerEntryContent::CommandProposed {
                command: command.clone(),
            },
        )
        .await;
    }
    if let AiAction::ReadRemoteFile { path } | AiAction::WriteRemoteFile { path, .. } = action {
        *path = ssh_manager_core::filter::normalize_lexical_path(path);
    }
}

/// Step 2 — escalate. Input: the engine's trace plus session/origin facts.
/// Output: the final decision, the rule-based risk assessment and whether a
/// stored sudo password is used. Safety property: the chain only combines
/// via `Decision::stricter` (`escalation::escalate`), so the engine decision
/// is never lowered; the one-shot flags the result names are consumed here.
pub(super) async fn apply_escalations(
    ctx: &ProposeCtx<'_>,
    action: &AiAction,
    evaluation: &EvaluationTrace,
    server_tags_unavailable: bool,
) -> Escalated {
    let session = ctx.session;
    let engine_decision = evaluation.decision.clone();
    let decision = evaluation.decision.clone();
    // `risk_assessment_for_action` is a pure function of `action`.
    let risk_assessment = risk_assessment_for_action(action);

    // The post-engine escalation chain lives once in `escalation::escalate`
    // (issue #826). Here: read the facts, escalate, then consume exactly the
    // one-shot flags the result names.
    let uses_password = uses_stored_sudo_password(session, action);
    let transform_scope = if session.outgoing_transform.is_some() {
        Some(super::super::outgoing::outgoing_scope(session, ctx.profile_store).await)
    } else {
        None
    };
    let mut facts = escalation_facts(
        session,
        action,
        risk_assessment.as_ref(),
        transform_scope.as_ref(),
    );
    facts.mcp_origin = ctx.is_mcp();
    facts.server_tags_unavailable = server_tags_unavailable;
    facts.earlier_rejection = ctx
        .earlier_rejection
        .load(std::sync::atomic::Ordering::SeqCst);
    let escalation = super::super::escalation::escalate(decision, &facts);
    if escalation.consume_injection_suspected
        && session
            .injection_suspected
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        let _ = take_injection_reason(session);
    }
    if escalation.consume_injection_check_unavailable {
        session
            .injection_check_unavailable
            .swap(false, std::sync::atomic::Ordering::SeqCst);
    }
    let decision = escalation.decision;

    // Issue #688: one default-level line when the chain turned the engine's
    // `AutoExec` into something stricter. Logging only.
    log_filter_escalation(ctx.session_id, action, &engine_decision, &decision);
    Escalated {
        decision,
        risk_assessment,
        uses_password,
    }
}

/// Step 3 — decide. Input: the final decision. Output: the decision with its
/// confirmation already registered (`None` if another MCP confirmation is
/// pending, Issue #108 / Spec 0104 §3). Safety property: a `Confirm` cannot
/// exist without its registered receiver (Spec 0088, A1.4).
pub(super) fn prepare_decision<'a>(
    ctx: &ProposeCtx<'a>,
    decision: Decision,
) -> Option<PreparedDecision<'a>> {
    match decision {
        Decision::AutoExec => Some(PreparedDecision::AutoExec),
        Decision::Deny { reason, code } => Some(PreparedDecision::Deny { reason, code }),
        Decision::Confirm { reason, code } => {
            let pending = PendingConfirmation::register(
                ctx.session,
                ctx.action_confirmations,
                ctx.action_id,
                ctx.is_mcp(),
            )
            .ok()?;
            Some(PreparedDecision::Confirm {
                reason,
                code,
                pending,
            })
        }
    }
}

/// Reports "another confirmation is pending" to the client; nothing runs.
async fn report_confirmation_conflict(ctx: &ProposeCtx<'_>) -> bool {
    emit_action_error(
        ctx.session,
        ctx.emitter,
        ctx.session_id,
        ANOTHER_CONFIRMATION_PENDING_MESSAGE.to_string(),
        None,
        ctx.persist,
    )
    .await
}

/// Step 4 — announce. Input: the final decision. Output: the
/// `chat-action-proposed` payload, emitted and remembered. Safety property:
/// the card shows exactly the decision that will be enforced; the rule names
/// for an automatic run are listed only for the final `AutoExec` (#696).
async fn announce_proposal(
    ctx: &ProposeCtx<'_>,
    action: &AiAction,
    evaluation: &EvaluationTrace,
    decision: &Decision,
    uses_password: bool,
    risk_assessment: Option<RiskAssessment>,
) -> Option<serde_json::Value> {
    let auto_exec_rules = if matches!(decision, Decision::AutoExec) {
        auto_exec_rules_for(ctx.session, ctx.profile_store, action, evaluation).await
    } else {
        Vec::new()
    };
    let (previous_note_content, target_name) =
        note_target_preview_for_action(action, ctx.session, ctx.profile_store).await;
    let (previous_file_content, previous_file_size) =
        previous_file_content_for_action(action, ctx.session).await;
    let payload = emit_chat_action_proposed(
        ctx.emitter,
        ctx.session_id,
        ctx.action_id,
        action.clone(),
        decision.clone(),
        previous_note_content,
        uses_password,
        previous_file_content,
        previous_file_size,
        target_name,
        risk_assessment,
        ctx.origin.clone(),
        auto_exec_rules,
    );
    ctx.session
        .remember_proposal(ctx.action_id, payload.clone());
    payload
}

/// Step 5 — second opinion (Spec 0026, Spec 0092 A3, issue #102). Input:
/// action and rule-based assessment. Output: the raised data-risk level and
/// whether the check was unavailable. Safety property: only escalates
/// (`escalate_data_risk`); an unavailable check is reported, not ignored.
/// Runs AFTER the proposal event, so the first display is not delayed.
async fn run_second_opinion(
    ctx: &ProposeCtx<'_>,
    action: &AiAction,
    risk_assessment: Option<RiskAssessment>,
) -> SecondOpinionOutcome {
    let session = ctx.session;
    let mut outcome = SecondOpinionOutcome {
        data_risk: None,
        unavailable: false,
    };
    let (Some(provider), Some(assessment)) = (
        session.risk_second_opinion_provider.as_deref(),
        risk_assessment,
    ) else {
        return outcome;
    };
    let Some(pseudo_command) = pseudo_command_for_risk_classification(action) else {
        return outcome;
    };
    wait_for_ai_request_slot(session).await;
    if let Some(budget) = session.risk_second_opinion_budget.as_deref() {
        wait_for_rate_limit_budget(
            budget,
            crate::compaction::estimate_tokens(&pseudo_command),
            ctx.emitter,
            ctx.session_id,
        )
        .await;
    }
    let mut request_usage = None;
    let second_opinion = crate::second_opinion::fetch_second_opinion_with_usage(
        provider,
        &pseudo_command,
        &mut request_usage,
    )
    .await;
    if let Some(usage) = request_usage {
        session
            .report_ai_usage(crate::ai_usage::AiUsagePurpose::RiskSecondOpinion, usage)
            .await;
    }
    // Issue #102: "could not be checked" differs from "checked, unremarkable".
    outcome.unavailable = second_opinion.is_unavailable();
    let (data_risk, reason) = escalate_data_risk(
        assessment.data_risk,
        assessment.data_risk_reason,
        match second_opinion {
            crate::second_opinion::SideCallOutcome::Verdict(level, text) => Some((level, text)),
            _ => None,
        },
    );
    session.review_proposal(ctx.action_id, data_risk, &reason);
    emit_risk_assessment_updated(
        ctx.emitter,
        ctx.session_id,
        ctx.action_id,
        data_risk,
        reason,
        outcome.unavailable,
    );
    // Spec 0092, A3: keep only the LEVEL, not the second opinion's text.
    outcome.data_risk = Some(data_risk);
    outcome
}

/// Pure part of step 6: the (reason, code) with which an `AutoExec` is turned
/// into `Confirm` after the second opinion, or `None` when it stays.
/// Safety property: only escalates; a stop before auto-exec (Spec 0066, A3.3)
/// takes precedence over the dialog (internal origin only). The texts never
/// quote the second-opinion model output (Spec 0092, §6: it ends up in the
/// ledger).
pub(super) fn second_opinion_escalation(
    red_risk_always_confirm: bool,
    outcome: &SecondOpinionOutcome,
    is_auto_exec: bool,
    stopped: bool,
) -> Option<(String, String)> {
    let escalate_for_red = red_risk_always_confirm && outcome.data_risk == Some(RiskLevel::Red);
    // Issue #102 (fail closed, per action): an enabled but failed second
    // opinion does not let an `AutoExec` through silently, independent of
    // `red_risk_always_confirm`.
    if !(escalate_for_red || outcome.unavailable) || !is_auto_exec || stopped {
        return None;
    }
    Some(if escalate_for_red {
        (
            "Daten-Risiko rot (KI-Zweitmeinung) – erfordert immer Bestätigung".to_string(),
            "FILTER_RED_RISK_REQUIRES_CONFIRM".to_string(),
        )
    } else {
        // Issue #102: fixed text, no provider error text.
        (
            "KI-Zweitmeinung nicht verfügbar – erfordert Bestätigung".to_string(),
            "FILTER_SECOND_OPINION_UNAVAILABLE_REQUIRES_CONFIRM".to_string(),
        )
    })
}

/// Whether a stop (Spec 0066) blocks a not-yet-started auto-execution. Only
/// for the own chat (internal origin), not MCP clients.
fn auto_exec_stopped(ctx: &ProposeCtx<'_>) -> bool {
    matches!(ctx.origin, ActionOrigin::Internal)
        && ctx
            .session
            .auto_continue_stop
            .load(std::sync::atomic::Ordering::SeqCst)
}

/// Step 6 — escalate after the second opinion (Spec 0092, A3). Input: the
/// prepared decision and the second-opinion outcome. Output: the same
/// decision or a `Confirm` (`Err` when another MCP confirmation is pending).
/// Safety property: only `AutoExec` can change, only to `Confirm`. Order:
/// `register` BEFORE the escalation event (A3.2).
fn escalate_after_second_opinion<'a>(
    ctx: &ProposeCtx<'a>,
    prepared: PreparedDecision<'a>,
    outcome: &SecondOpinionOutcome,
    proposed_payload: Option<serde_json::Value>,
) -> Result<PreparedDecision<'a>, ()> {
    let Some((reason, code)) = second_opinion_escalation(
        ctx.session.red_risk_always_confirm,
        outcome,
        matches!(prepared, PreparedDecision::AutoExec),
        auto_exec_stopped(ctx),
    ) else {
        return Ok(prepared);
    };
    let pending = PendingConfirmation::register(
        ctx.session,
        ctx.action_confirmations,
        ctx.action_id,
        ctx.is_mcp(),
    )
    .map_err(|_| ())?;
    emit_action_decision_escalated(
        ctx.emitter,
        ctx.session_id,
        ctx.action_id,
        reason.clone(),
        code.clone(),
    );
    ctx.session
        .escalate_proposal(ctx.action_id, proposed_payload, &reason, &code);
    Ok(PreparedDecision::Confirm {
        reason,
        code,
        pending,
    })
}

/// Step 7a — run an auto-approved action. Input: the action that survived all
/// escalations. Output: whether it ran. Safety property: the ledger entry
/// "auto approved" precedes execution; only `ApprovedAction::from_auto_exec`
/// reaches `execute_action`.
async fn run_approved(
    ctx: &ProposeCtx<'_>,
    action: AiAction,
    matched_rule: Option<ssh_manager_core::filter::RuleId>,
    matched_rule_origin: Option<ssh_manager_core::filter::RuleOrigin>,
    uses_password: bool,
) -> bool {
    if let AiAction::SuggestCommand { .. } = &action {
        write_ledger_entry(
            ctx.session,
            ledger_source_for_origin(&ctx.origin),
            LedgerEntryContent::Decision {
                outcome: LedgerDecisionOutcome::AutoApproved,
                reason: None,
                code: None,
                matched_rule,
                matched_rule_origin,
            },
        )
        .await;
    }
    execute_action(
        ctx.session,
        ctx.session_id,
        ctx.action_id,
        ApprovedAction::from_auto_exec(action),
        ctx.emitter,
        ctx.profile_store,
        ctx.persist,
        ledger_source_for_origin(&ctx.origin),
        uses_password,
    )
    .await
}

/// Step 7b — report a blocked action. Output: `true` (the AI gets a follow-up
/// round). Safety property: nothing is executed and no confirmation awaited
/// (Spec 0007 §5); later actions of the same AI reply escalate (Spec 0068).
async fn report_denied(
    ctx: &ProposeCtx<'_>,
    action: &AiAction,
    reason: String,
    code: String,
    matched_rule: Option<ssh_manager_core::filter::RuleId>,
    matched_rule_origin: Option<ssh_manager_core::filter::RuleOrigin>,
) -> bool {
    ctx.earlier_rejection
        .store(true, std::sync::atomic::Ordering::SeqCst);
    if let AiAction::SuggestCommand { .. } = action {
        write_ledger_entry(
            ctx.session,
            ledger_source_for_origin(&ctx.origin),
            LedgerEntryContent::Decision {
                outcome: LedgerDecisionOutcome::Rejected,
                reason: Some(reason.clone()),
                code: Some(code),
                matched_rule,
                matched_rule_origin,
            },
        )
        .await;
    }
    // Spec 0021, section 3, case 4: the AI gets a context entry with the
    // block reason instead of being silently ignored.
    push_history_scoped(
        ctx.session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::ActionRejected {
                command: describe_rejected_action(action),
                reason: RejectionReason::Blocked(reason),
            },
        },
        ctx.persist,
    )
    .await;
    true
}

/// Pure part of step 7c: maps the wait outcome to a settlement. Safety
/// property: a timeout or reconnect is a denial, never an approval
/// (Spec 0046, finding 4; issue #1019).
pub(super) fn settle_confirmation(
    action_id: ActionId,
    outcome: super::super::pending_confirmation::ConfirmationWaitOutcome,
) -> ConfirmationSettlement {
    match outcome {
        Ok(Ok(ActionUserDecision::DenyReconnect)) => {
            ConfirmationSettlement::Decided(ActionUserDecision::Deny, RejectionReason::Reconnect)
        }
        Ok(Ok(decision)) => ConfirmationSettlement::Decided(decision, RejectionReason::User),
        Ok(Err(_)) => ConfirmationSettlement::SenderDropped,
        Err(_elapsed) => {
            // Spec 0046, finding 4: no response ever came. Treated as a
            // denial (`Timeout`, not `User`: no human decided).
            tracing::warn!(
                ?action_id,
                timeout_secs = PENDING_ACTION_CONFIRM_TIMEOUT.as_secs(),
                "pending action confirmation timed out without a response, treating as denied"
            );
            ConfirmationSettlement::Decided(ActionUserDecision::Deny, RejectionReason::Timeout)
        }
    }
}

/// Step 7c — wait for the user. Input: the registered confirmation. Output:
/// whether the action ran (via `handle_user_decision`). Safety property: the
/// pending indicator and registry entry are cleaned up BEFORE the decision is
/// executed (Spec 0088, A1.1); every non-approval marks `earlier_rejection`.
async fn await_confirmation(
    ctx: &ProposeCtx<'_>,
    action: AiAction,
    pending: PendingConfirmation<'_>,
    (confirm_reason, confirm_code): (String, String),
    (matched_rule, matched_rule_origin): MatchedRule,
    uses_password: bool,
) -> bool {
    let (timeout_result, cleanup) = pending
        .wait_for_decision(PENDING_ACTION_CONFIRM_TIMEOUT)
        .await;
    let (user_decision, deny_reason) = match settle_confirmation(ctx.action_id, timeout_result) {
        ConfirmationSettlement::Decided(decision, reason) => (decision, reason),
        ConfirmationSettlement::SenderDropped => {
            ctx.earlier_rejection
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return false;
        }
    };
    drop(cleanup);
    emit_settled_for_rejection(ctx.emitter, ctx.session_id, ctx.action_id, &deny_reason);
    if matches!(user_decision, ActionUserDecision::Deny) {
        ctx.earlier_rejection
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    handle_user_decision(
        ctx.session,
        ctx.session_id,
        ctx.action_id,
        action,
        user_decision,
        deny_reason,
        ctx.emitter,
        ctx.profile_store,
        ctx.origin.clone(),
        matched_rule,
        matched_rule_origin,
        confirm_reason,
        confirm_code,
        uses_password,
        ctx.earlier_rejection,
    )
    .await
}

/// Öffentlicher Einstiegspunkt für Spec 0028 (MCP), von
/// `app_shell::mcp_backend` genutzt — dieselbe Orchestrierungs-Funktion wie der
/// interne Chat-Flow oben, nur mit `origin` fest auf `ActionOrigin::Mcp`
/// (erzwingt die Verschärfung aus Abschnitt 5) und `round` fest auf `1`
/// (kein Auto-Continuation-Konzept für MCP-Aufrufe, s. Spec 0028,
/// Abschnitt 3 — es gibt keinen Chatverlauf, der automatisch fortgesetzt
/// werden müsste). Bewusst dieser schmale Wrapper statt
/// `handle_action_proposed` selbst `pub(crate)` zu machen: die
/// internen Parameter `round`/`origin` sollen von außerhalb dieses Moduls
/// nicht frei wählbar sein.
pub async fn handle_mcp_action_proposed(
    session: &Session,
    session_id: SessionId,
    action: AiAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    client_name: Option<String>,
) -> bool {
    // Spec 0096, A2: der dritte der drei KI-Wege — ein externer Agent über
    // MCP. Geschwärzt wird hier, vor `handle_action_proposed`, damit sowohl
    // der Bestätigungsdialog in der App als auch das daraus abgeleitete
    // Ergebnis an den MCP-Client (s. `app_shell::mcp_backend::CaptureEmitter`)
    // dieselbe geschwärzte Fassung sehen wie die gespeicherte Revision.
    let Some(action) = super::super::notes::redact_note_proposal(action, session.redactor.as_ref())
    else {
        return emit_action_error(
            session,
            emitter,
            session_id,
            super::super::notes::NOTE_PROPOSAL_FULLY_REDACTED_MESSAGE.to_string(),
            None,
            // MCP-Herkunft schreibt nie in die persistierte Historie — s.
            // `handle_action_proposed`s `persist`-Ableitung.
            false,
        )
        .await;
    };
    handle_action_proposed(
        session,
        session_id,
        action,
        emitter,
        profile_store,
        action_confirmations,
        ActionOrigin::Mcp { client_name },
        // Ein MCP-Aufruf ist genau eine Aktion — keine Nachbar-Aktionen.
        &std::sync::atomic::AtomicBool::new(false),
    )
    .await
}

/// Aktuelle Tags des Servers dieser Session, für die Filter-Engine-Auswertung
/// (`EvalContext::tags`) — bevorzugt frisch aus dem `ProfileStore` gelesen
/// (Tags können sich seit Sitzungsbeginn geändert haben), fällt auf die bei
/// `connect()` eingefrorene Kopie zurück, falls der Server inzwischen nicht
/// mehr auflösbar ist.
pub(super) async fn tags_for_session(
    session: &Session,
    profile_store: &dyn ProfileStore,
) -> Vec<String> {
    server_tags_lookup(session, profile_store).await.0
}

/// [`tags_for_session`] plus whether the profile store failed with anything
/// but "server not stored" (issue #843). `ServerNotFound` is normal (the
/// local pseudo-server has no row) and keeps the frozen tags silently; any
/// other error keeps them too but is reported, so the caller can escalate.
/// Logs the server id and error kind only.
pub(super) async fn server_tags_lookup(
    session: &Session,
    profile_store: &dyn ProfileStore,
) -> (Vec<String>, bool) {
    match profile_store.get_server(&session.server_id).await {
        Ok(server) => (server.tags, false),
        Err(ssh_manager_core::profiles::ProfileError::ServerNotFound(_)) => {
            (session.tags.clone(), false)
        }
        Err(_) => {
            // Error kind only, never the message (ADR 0086).
            tracing::warn!(
                server_id = %session.server_id.0,
                error_kind = "store_error",
                "server tags could not be read — rules use the tags from connect time"
            );
            (session.tags.clone(), true)
        }
    }
}

/// Issue #696: the rules behind an automatic run, from the trace of the
/// evaluation that made the decision (see `confirmation_explain::auto_exec_rules`).
pub(super) async fn auto_exec_rules_for(
    session: &Session,
    profile_store: &dyn ProfileStore,
    action: &AiAction,
    evaluation: &EvaluationTrace,
) -> Vec<crate::dto::AutoExecRuleDto> {
    // Only filter-evaluated actions carry rule matches; notes never auto-run.
    if !matches!(
        action,
        AiAction::SuggestCommand { .. } | AiAction::ReadRemoteFile { .. }
    ) {
        return Vec::new();
    }
    let tags = tags_for_session(session, profile_store).await;
    let ctx = EvalContext {
        server_id: session.server_id,
        tags,
    };
    crate::confirmation_explain::auto_exec_rules(session.filter_engine.as_ref(), &ctx, evaluation)
        .await
}

/// Spec 0020, Abschnitt 4.1/4.2: `ReadRemoteFile`/`WriteRemoteFile` werden
/// für die Filter-Engine-Auswertung auf Pseudokommandos abgebildet
/// (`sftp-read <pfad>`/`sftp-write <pfad>`) — dieselbe Präzedenz-Kette wie
/// für Shell-Kommandos, kein zweites paralleles Regelkonzept.
pub(super) fn sftp_read_pseudo_command(path: &str) -> String {
    format!("sftp-read {path}")
}

pub(super) fn sftp_write_pseudo_command(path: &str) -> String {
    format!("sftp-write {path}")
}

/// Spec 0026, Abschnitt 2, Punkt 5: synchron beim Erzeugen des
/// `chat-action-proposed`-Events berechnet, für exakt dieselben drei
/// Aktionstypen, die die Filter-Engine kennt (`evaluate_action`) —
/// `ReadRemoteFile`/`WriteRemoteFile` nutzen dieselbe Pseudokommando-
/// Abbildung wie dort (Spec 0020, Abschnitt 4.1), keine zweite
/// Mapping-Logik. `None` für `ProposeNoteUpdate`/`GenerateDocument`, die
/// Spec 0026 nicht abdeckt (kein ausführbares Kommando/kein Dateipfad).
pub(super) fn pseudo_command_for_risk_classification(action: &AiAction) -> Option<String> {
    match action {
        AiAction::SuggestCommand { command } => Some(command.clone()),
        AiAction::ReadRemoteFile { path } => Some(sftp_read_pseudo_command(path)),
        AiAction::WriteRemoteFile { path, .. } => Some(sftp_write_pseudo_command(path)),
        AiAction::ProposeNoteUpdate { .. }
        | AiAction::GenerateDocument { .. }
        | AiAction::AskUser { .. } => None,
    }
}

/// Menschen-/KI-lesbare Kurzbeschreibung einer Aktion für
/// `MessageContent::ActionRejected.command` (Spec 0021, Abschnitt 3) — für
/// `SuggestCommand` das Kommando selbst, für `ReadRemoteFile`/
/// `WriteRemoteFile` dieselbe Pseudokommando-Form wie in der
/// Filter-Engine-Auswertung (Spec 0020). `ProposeNoteUpdate` folgt "demselben
/// Muster ... aber ohne eigene Sonderbehandlung" (Spec 0021, Abschnitt 3,
/// letzter Absatz) — bekommt trotzdem eine sinnvolle Kurzbezeichnung statt
/// eines rohen Debug-Werts. `GenerateDocument` erreicht diese Funktion nie
/// (durchläuft weder `evaluate_action` noch einen Bestätigungsdialog, s.
/// dort) — der `unreachable!()`-Arm hält dieselbe Invariante fest.
/// Spec 0066: eine wegen Stopp nicht gestartete Auto-Ausführung wird wie
/// ein abgebrochenes Kommando gemeldet (Spec 0027, `cancelled: true`,
/// leere Ausgabe) — die Aktionskarte zeigt dadurch „abgebrochen" statt
/// dauerhaft „läuft", und die KI sieht im Verlauf, dass nichts lief.
pub(super) async fn skip_auto_exec_after_stop(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: &AiAction,
    emitter: &dyn EventEmitter,
    persist: bool,
) -> bool {
    let command = match action {
        AiAction::SuggestCommand { command } => command.clone(),
        other => describe_rejected_action(other),
    };
    emit_chat_action_result(
        emitter,
        session_id,
        action_id,
        ActionResultPayload::Command {
            command: command.clone(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            cancelled: true,
            truncated: false,
        },
    );
    push_history_scoped(
        session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command,
                output: CommandOutput {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: None,
                    truncated: false,
                },
                cancelled: true,
                waiting_for_input: false,
            },
        },
        persist,
    )
    .await;
    false
}

/// `AiAction::SuggestCommand` läuft durch die Filter-Engine;
/// `AiAction::ProposeNoteUpdate` verlangt **immer** eine Bestätigung,
/// unabhängig von der Filter-Engine (Spec 0003, Abschnitt 5.2 — explizit
/// wiederholt in Spec 0007, Abschnitt 6, letzter Punkt). `ReadRemoteFile`/
/// `WriteRemoteFile` laufen ebenfalls durch die Filter-Engine (Spec 0020,
/// Abschnitt 4.1/4.2, Punkt 1) — `WriteRemoteFile` bekommt dabei aber nie
/// `AutoExec` (Abschnitt 4.2, Punkt 2: "Auch bei einer Allow-Regel wird nie
/// ohne Anzeige geschrieben").
pub(super) async fn evaluate_action(
    session: &Session,
    action: &AiAction,
    profile_store: &dyn ProfileStore,
) -> (EvaluationTrace, bool) {
    match action {
        AiAction::SuggestCommand { command } => {
            let (tags, tags_unavailable) = server_tags_lookup(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            let trace = session
                .filter_engine
                .evaluate_explained(command, &ctx)
                .await;
            (trace, tags_unavailable)
        }
        AiAction::ProposeNoteUpdate { .. } => (
            EvaluationTrace {
                decision: Decision::Confirm {
                    reason: "Notiz-Aktualisierungen erfordern immer eine manuelle Bestätigung"
                        .to_string(),
                    code: "FILTER_NOTE_UPDATE_REQUIRES_CONFIRM".to_string(),
                },
                matched_rule: None,
                matched_rule_origin: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
                kind: Default::default(),
                segment_index: None,
                checks: Vec::new(),
            },
            false,
        ),
        AiAction::GenerateDocument { .. } => unreachable!(
            "GenerateDocument wird bereits in run_one_round abgefangen \
             (Spec 0012: kein Filter-Engine-/Bestätigungspfad) und erreicht \
             evaluate_action nie"
        ),
        AiAction::AskUser { .. } => unreachable!(
            "AskUser is intercepted in run_one_round (issue #545: no filter/confirmation path) \
             and never reaches evaluate_action"
        ),
        AiAction::ReadRemoteFile { path } => {
            let (tags, tags_unavailable) = server_tags_lookup(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            let trace = session
                .filter_engine
                .evaluate_explained(&sftp_read_pseudo_command(path), &ctx)
                .await;
            (trace, tags_unavailable)
        }
        AiAction::WriteRemoteFile { path, .. } => {
            let (tags, tags_unavailable) = server_tags_lookup(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            let mut trace = session
                .filter_engine
                .evaluate_explained(&sftp_write_pseudo_command(path), &ctx)
                .await;
            if matches!(trace.decision, Decision::AutoExec) {
                trace.decision = Decision::Confirm {
                    reason: "Dateischreibvorgänge werden immer zur Bestätigung angezeigt \
                             (Spec 0020, Abschnitt 4.2)"
                        .to_string(),
                    code: "FILTER_FILE_WRITE_REQUIRES_CONFIRM".to_string(),
                };
            }
            (trace, tags_unavailable)
        }
    }
}
