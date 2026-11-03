//! Vorschlag → Bewertung → Ausführung einer `AiAction` (Spec 0007,
//! Abschnitt 6 ff.) — s. Moduldoc in `orchestration.rs` für den
//! vollständigen Kontext (Spec 0083: reine Verschiebung aus
//! `orchestration.rs`, keine Verhaltensänderung).

mod decision;
mod execute;
mod injection;
mod logging;
mod propose;
mod rejected_action;
mod risk;

pub(crate) use execute::emit_action_error;
pub(crate) use injection::{
    announce_second_opinion_setup_failure, check_for_injected_instructions, INJECTION_REASON_MARKER,
};
pub(crate) use propose::handle_action_proposed;
pub use propose::handle_mcp_action_proposed;
pub(crate) use risk::state_independent_escalation_code;

// Sibling items reach each other (and the test modules) through this module.
use self::{
    decision::*, execute::*, injection::*, logging::*, propose::*, rejected_action::*, risk::*,
};
use super::pending_confirmation::PendingConfirmation;

// Imports the test modules below reach through `use super::*`.
#[cfg(test)]
#[allow(unused_imports)]
use uuid::Uuid;

#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::ai::{
    truncate_for_second_opinion, ChatMessage, MessageContent, OutputRedactor, RejectionReason,
    Role, DEFAULT_SECOND_OPINION_MAX_LEN,
};
#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent, LedgerSource};
#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::filter::{
    exceeds_command_length_limit, Decision, EvalContext, EvaluationTrace, RuleId, RuleOrigin,
    DEFAULT_MAX_COMMAND_LENGTH,
};
#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::profiles::{AiAction, PostIngestPolicy, ProfileStore};
#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::risk::{RiskAssessment, RiskClassifier, RiskLevel, RuleBasedRiskClassifier};
#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::ssh::SshError;
#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::ssh::{CommandOutput, ExecOutcome};

#[cfg(test)]
#[allow(unused_imports)]
use ssh_manager_core::profiles::NoteTargetSelector;

#[cfg(test)]
#[allow(unused_imports)]
use crate::confirmation::ConfirmationRegistry;
#[cfg(test)]
#[allow(unused_imports)]
use crate::confirmation_settled::emit_settled_for_rejection;
#[cfg(test)]
#[allow(unused_imports)]
use crate::dto::{ActionOrigin, ActionUserDecision};
#[cfg(test)]
#[allow(unused_imports)]
use crate::events::{
    emit_action_decision_escalated, emit_chat_action_proposed, emit_chat_action_result,
    emit_chat_error, emit_chat_injection_check, emit_risk_assessment_updated, ActionResultPayload,
    EventEmitter, InjectionCheckState,
};
#[cfg(test)]
#[allow(unused_imports)]
use crate::session::Session;
#[cfg(test)]
#[allow(unused_imports)]
use crate::state::{ActionId, SessionId};

#[cfg(test)]
#[allow(unused_imports)]
use super::approval::{ApprovedAction, ApprovedCommand};
#[cfg(test)]
#[allow(unused_imports)]
use super::chat_turn::{
    ledger_source_for_origin, push_history_scoped, wait_for_ai_request_slot,
    wait_for_rate_limit_budget, write_ledger_entry, PENDING_ACTION_CONFIRM_TIMEOUT,
};
#[cfg(test)]
#[allow(unused_imports)]
use super::notes::{execute_note_update, note_target_preview_for_action};
#[cfg(test)]
#[allow(unused_imports)]
use super::pending_confirmation::ANOTHER_CONFIRMATION_PENDING_MESSAGE;
#[cfg(test)]
#[allow(unused_imports)]
use super::remote_files::{
    execute_read_remote_file, execute_write_remote_file, previous_file_content_for_action,
};

// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_core;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_check_unavailable;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_files_and_ledger;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_outgoing;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_pending_confirmation;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_red_risk;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_red_risk_second_opinion;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_not_assessable;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_escalation_log;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_escalation;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_live_output;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_auto_exec_rules;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_server_tags;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_path_normalisation;

// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_propose_steps;

/// Spec 0088, A1.4: Die Entscheidung der Filter-Engine, angereichert um das
/// bereits registrierte Warten.
///
/// Spiegelt [`Decision`] Variante für Variante, trägt im `Confirm`-Zweig aber
/// zusätzlich den [`PendingConfirmation`]-Guard. Vorher lag der Empfänger
/// daneben in einem `Option<oneshot::Receiver<…>>`, das der `Confirm`-Zweig
/// per `expect("confirm_rx muss registriert sein")` auspackte — eine
/// Invariante, die nur im Text stand. Jetzt trägt sie der Typ: Ein
/// `Confirm`-Zweig ohne Empfänger lässt sich nicht konstruieren.
pub(super) enum PreparedDecision<'a> {
    AutoExec,
    Deny {
        reason: String,
        code: String,
    },
    Confirm {
        reason: String,
        code: String,
        pending: PendingConfirmation<'a>,
    },
}

/// Test helper: a store with nothing in it that outlives the call expression.
#[cfg(test)]
fn empty_store() -> &'static crate::test_support::InMemoryProfileStore {
    Box::leak(Box::default())
}
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_injection_reason;
