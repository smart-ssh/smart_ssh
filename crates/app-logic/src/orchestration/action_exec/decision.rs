//! Split out of `action_exec.rs` (issue #859, pure move).

use super::*;
use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, RejectionReason, Role};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent, LedgerSource};
use ssh_manager_core::filter::{Decision, EvalContext, RuleId, RuleOrigin};
use ssh_manager_core::profiles::{AiAction, ProfileStore};

use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{emit_chat_action_proposed, EventEmitter};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::super::approval::ApprovedAction;
use super::super::chat_turn::{ledger_source_for_origin, push_history_scoped, write_ledger_entry};

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_user_decision(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    user_decision: ActionUserDecision,
    // Spec 0046, Fund 4: nur für den `ActionUserDecision::Deny`-Zweig
    // relevant — `RejectionReason::User` für eine echte Nutzer-Ablehnung,
    // `RejectionReason::Timeout` für die vom Backend-Sicherheitsnetz
    // fabrizierte Ablehnung, wenn `PENDING_ACTION_CONFIRM_TIMEOUT` ablief,
    // ohne dass je eine Entscheidung eintraf.
    deny_reason: RejectionReason,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    origin: ActionOrigin,
    matched_rule: Option<RuleId>,
    matched_rule_origin: Option<RuleOrigin>,
    // spec-reviewer-Fund (Review dieses Schritts): die ursprüngliche
    // `Decision::Confirm { reason, code }`, die zum Bestätigungsdialog
    // führte — z. B. `FILTER_MCP_ORIGIN_REQUIRES_CONFIRM`/
    // `FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM`/
    // `FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM`. Vorher gingen diese
    // Eskalationsgründe für den finalen Ledger-`Decision`-Eintrag
    // verloren (`reason`/`code` liefen dort fest auf `None`), obwohl sie
    // hier längst vorlagen — ein Leser des Ledgers konnte dann nicht
    // rekonstruieren, WARUM überhaupt eine Bestätigung nötig war, nur DASS
    // eine Regel gegriffen hätte.
    confirm_reason: String,
    confirm_code: String,
    // Spec 0068, Teil 3 (Review-Fund): der im Dialog tatsächlich
    // angekündigte Wert (`usesStoredSudoPassword`) — nur damit darf ein
    // Schreib-Fallback Sudo nutzen.
    sudo_fallback_announced: bool,
    // Spec 0068, Teil 4 (zweite Review-Runde): auch ein vom Nutzer
    // bearbeitetes, dann regelbasiert blockiertes Kommando zählt als
    // Ablehnung für die übrigen Aktionen derselben Antwort.
    earlier_rejection: &std::sync::atomic::AtomicBool,
) -> bool {
    // Spec 0040, Abschnitt 4: MCP origin never persists, also not via this
    // confirmation-dialog path (see `handle_action_proposed`).
    let env = DecisionEnv {
        session,
        session_id,
        action_id,
        emitter,
        profile_store,
        persist: !matches!(origin, ActionOrigin::Mcp { .. }),
        origin: &origin,
        sudo_fallback_announced,
        earlier_rejection,
    };
    let record = DecisionRecord::new(
        &deny_reason,
        &origin,
        confirm_reason,
        confirm_code,
        matched_rule,
        matched_rule_origin,
    );
    match user_decision {
        ActionUserDecision::Deny | ActionUserDecision::DenyReconnect => {
            env.reject(&record, &action, deny_reason).await
        }
        ActionUserDecision::Approve => env.approve(&record, action).await,
        ActionUserDecision::EditThenApprove { command: edited } => {
            env.edit_then_approve(action, edited).await
        }
    }
}

/// Everything the decision steps share.
struct DecisionEnv<'a> {
    session: &'a Session,
    session_id: SessionId,
    action_id: ActionId,
    emitter: &'a dyn EventEmitter,
    profile_store: &'a dyn ProfileStore,
    persist: bool,
    origin: &'a ActionOrigin,
    /// Spec 0068, Teil 3: the value announced in the dialog.
    sudo_fallback_announced: bool,
    /// Spec 0068, Teil 4: set when an edited command is blocked by a rule.
    earlier_rejection: &'a std::sync::atomic::AtomicBool,
}

/// The ledger `Decision` fields of the user's decision (Spec 0057, §1.1).
struct DecisionRecord {
    source: LedgerSource,
    reason: Option<String>,
    code: Option<String>,
    matched_rule: Option<RuleId>,
    matched_rule_origin: Option<RuleOrigin>,
}

impl DecisionRecord {
    /// `LedgerSource::User` only for a real user decision; the timeout case
    /// (Spec 0046, Fund 4) had no human decide, so it gets the origin of the
    /// proposal and the specific code `TIMEOUT`. Otherwise the original
    /// escalation reason/code is kept so the ledger shows WHY a confirmation
    /// was needed.
    fn new(
        deny_reason: &RejectionReason,
        origin: &ActionOrigin,
        confirm_reason: String,
        confirm_code: String,
        matched_rule: Option<RuleId>,
        matched_rule_origin: Option<RuleOrigin>,
    ) -> Self {
        let (source, reason, code) = if matches!(deny_reason, RejectionReason::Timeout) {
            (
                ledger_source_for_origin(origin),
                Some(
                    "Bestätigung nicht innerhalb des Zeitfensters erfolgt, als Ablehnung \
                     behandelt (Spec 0046, Fund 4)"
                        .to_string(),
                ),
                Some("TIMEOUT".to_string()),
            )
        } else {
            (LedgerSource::User, Some(confirm_reason), Some(confirm_code))
        };
        Self {
            source,
            reason,
            code,
            matched_rule,
            matched_rule_origin,
        }
    }

    fn content(&self, outcome: LedgerDecisionOutcome) -> LedgerEntryContent {
        LedgerEntryContent::Decision {
            outcome,
            reason: self.reason.clone(),
            code: self.code.clone(),
            matched_rule: self.matched_rule.clone(),
            matched_rule_origin: self.matched_rule_origin,
        }
    }
}

impl DecisionEnv<'_> {
    /// Hands the approved action to `execute_action`.
    async fn run(&self, action: AiAction, source: LedgerSource) -> bool {
        execute_action(
            self.session,
            self.session_id,
            self.action_id,
            ApprovedAction::from_user_approval(action),
            self.emitter,
            self.profile_store,
            self.persist,
            source,
            self.sudo_fallback_announced,
        )
        .await
    }

    /// Spec 0021, Abschnitt 3, Fall 3: tell the AI that the human (or the
    /// timeout, Spec 0046 Fund 4) rejected, so it can react instead of the
    /// chat staying silent.
    async fn reject(
        &self,
        record: &DecisionRecord,
        action: &AiAction,
        reason: RejectionReason,
    ) -> bool {
        if let AiAction::SuggestCommand { .. } = action {
            write_ledger_entry(
                self.session,
                record.source,
                record.content(LedgerDecisionOutcome::Rejected),
            )
            .await;
        }
        push_history_scoped(
            self.session,
            ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::ActionRejected {
                    command: describe_rejected_action(action),
                    reason,
                },
            },
            self.persist,
        )
        .await;
        true
    }

    async fn approve(&self, record: &DecisionRecord, action: AiAction) -> bool {
        if let AiAction::SuggestCommand { .. } = &action {
            write_ledger_entry(
                self.session,
                record.source,
                record.content(LedgerDecisionOutcome::Confirmed),
            )
            .await;
        }
        self.run(action, record.source).await
    }

    async fn edit_then_approve(&self, action: AiAction, edited: String) -> bool {
        let effective_action = match &action {
            AiAction::SuggestCommand { .. } => match self.reevaluate_edited(edited).await {
                Some(edited_action) => edited_action,
                None => return true,
            },
            // Neither `ProposeNoteUpdate` nor `ReadRemoteFile`/`WriteRemoteFile`
            // offers an edit field (Spec 0020, Abschnitt 4.2) — should
            // `EditThenApprove` arrive anyway, the original action runs unchanged.
            AiAction::ProposeNoteUpdate { .. }
            | AiAction::ReadRemoteFile { .. }
            | AiAction::WriteRemoteFile { .. } => action,
            AiAction::AskUser { .. } => unreachable!(
                "AskUser needs no confirmation and never reaches execution, see run_one_round"
            ),
            AiAction::GenerateDocument { .. } => {
                unreachable!("GenerateDocument braucht nie eine Bestätigung, s. evaluate_action")
            }
        };
        self.run(effective_action, LedgerSource::User).await
    }

    /// An edited command must not bypass the check: it goes through the
    /// filter engine again. Only a hard `Deny` overrides the user's approval;
    /// a repeated `Confirm` does not, since the click on "run" in the edit
    /// dialog *is* the required confirmation. Returns the action to run, or
    /// `None` when it was blocked (already reported).
    async fn reevaluate_edited(&self, edited: String) -> Option<AiAction> {
        let session = self.session;
        // Spec 0057, §1.1: the edited text is a proposal of its own, written
        // by the user, hence `LedgerSource::User`.
        write_ledger_entry(
            session,
            LedgerSource::User,
            LedgerEntryContent::CommandProposed {
                command: edited.clone(),
            },
        )
        .await;
        // A store failure is logged by the lookup; only `Deny` matters in
        // this re-evaluation, so the frozen tags stay in use (issue #843).
        let (tags, _tags_unavailable) = server_tags_lookup(session, self.profile_store).await;
        let ctx = EvalContext {
            server_id: session.server_id,
            tags,
        };
        let re_evaluation = session
            .filter_engine
            .evaluate_explained(&edited, &ctx)
            .await;
        if let Decision::Deny { reason, code } = re_evaluation.decision {
            self.earlier_rejection
                .store(true, std::sync::atomic::Ordering::SeqCst);
            write_ledger_entry(
                session,
                ledger_source_for_origin(self.origin),
                LedgerEntryContent::Decision {
                    outcome: LedgerDecisionOutcome::Rejected,
                    reason: Some(reason.clone()),
                    code: Some(code.clone()),
                    matched_rule: re_evaluation.matched_rule,
                    matched_rule_origin: re_evaluation.matched_rule_origin,
                },
            )
            .await;
            self.report_blocked_edit(edited, reason, code).await;
            return None;
        }
        write_ledger_entry(
            session,
            LedgerSource::User,
            LedgerEntryContent::Decision {
                outcome: LedgerDecisionOutcome::Confirmed,
                reason: None,
                code: None,
                matched_rule: re_evaluation.matched_rule,
                matched_rule_origin: re_evaluation.matched_rule_origin,
            },
        )
        .await;
        Some(AiAction::SuggestCommand { command: edited })
    }

    /// Spec 0021, Abschnitt 3, Fall 4: the blocked edit is shown as a denied
    /// proposal and recorded in the history like any filter-blocked proposal.
    async fn report_blocked_edit(&self, edited: String, reason: String, code: String) {
        let blocked = AiAction::SuggestCommand {
            command: edited.clone(),
        };
        let risk_assessment = risk_assessment_for_action(&blocked);
        emit_chat_action_proposed(
            self.emitter,
            self.session_id,
            Uuid::new_v4(),
            blocked,
            Decision::Deny {
                reason: reason.clone(),
                code,
            },
            None,
            false,
            None,
            None,
            None,
            risk_assessment,
            self.origin.clone(),
            Vec::new(),
        );
        push_history_scoped(
            self.session,
            ChatMessage {
                role: Role::ActionResult,
                content: MessageContent::ActionRejected {
                    command: edited,
                    reason: RejectionReason::Blocked(reason),
                },
            },
            self.persist,
        )
        .await;
    }
}
