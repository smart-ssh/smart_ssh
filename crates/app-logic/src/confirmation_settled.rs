//! Issue #1019: a pending confirmation settled by the backend without the
//! user (reconnect, timeout). The chat card learns about it through the
//! `action-confirmation-settled` event; the model reads its own reason.

use serde::Serialize;
use ssh_manager_core::ai::RejectionReason;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::ActionUserDecision;
use crate::events::EventEmitter;
use crate::session::Session;
use crate::state::{ActionId, SessionId};

/// Why the backend settled a pending confirmation on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationSettledReason {
    Reconnect,
    Timeout,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionConfirmationSettledPayload {
    session_id: SessionId,
    action_id: ActionId,
    reason: ConfirmationSettledReason,
}

/// Emits `action-confirmation-settled` (ids and reason only) when the
/// rejection was fabricated by the backend; a user decision or a filter
/// block emits nothing.
pub fn emit_settled_for_rejection(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    action_id: ActionId,
    rejection: &RejectionReason,
) {
    let reason = match rejection {
        RejectionReason::Reconnect => ConfirmationSettledReason::Reconnect,
        RejectionReason::Timeout => ConfirmationSettledReason::Timeout,
        RejectionReason::User | RejectionReason::Blocked(_) => return,
    };
    let payload = ActionConfirmationSettledPayload {
        session_id,
        action_id,
        reason,
    };
    match serde_json::to_value(&payload) {
        Ok(value) => emitter.emit_event("action-confirmation-settled", value),
        Err(err) => tracing::warn!(error = %err, "action-confirmation-settled not serializable"),
    }
}

/// History text (chat view) for a rejected action.
pub fn rejection_reason_text(reason: RejectionReason) -> String {
    match reason {
        RejectionReason::User => "Vom Nutzer abgelehnt.".to_string(),
        RejectionReason::Blocked(reason) => reason,
        // Spec 0046, Fund 4: no user actively rejected here.
        RejectionReason::Timeout => "Zeitüberschreitung — nicht innerhalb der zulässigen Zeit \
             beantwortet, automatisch abgelehnt."
            .to_string(),
        RejectionReason::Reconnect => "Verworfen, weil die Verbindung neu aufgebaut wurde — der \
             Nutzer hat nicht entschieden."
            .to_string(),
    }
}

impl Session {
    /// Like [`Session::reject_pending_confirmation`] (fail closed, same
    /// channel as a click on "Reject"), but the rejection is booked as
    /// `RejectionReason::Reconnect`. Never approves. Returns whether this
    /// call discarded the confirmation.
    pub fn discard_pending_confirmation_for_reconnect(
        &self,
        confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    ) -> bool {
        let Some(action_id) = *crate::poison::lock_tolerating_poison(&self.pending_action) else {
            return false;
        };
        match confirmations.resolve(&action_id, ActionUserDecision::DenyReconnect) {
            Ok(()) => true,
            Err(err) => {
                tracing::debug!(error = %err, "pending confirmation already settled on reconnect");
                false
            }
        }
    }
}
