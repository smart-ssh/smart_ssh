//! Issue #1020, Spec 0114 §4: what a chat needs to be rebuilt after the
//! webview missed events (Android background): the history plus the
//! confirmation that still waits. The proposal kept here is display data
//! for the card only; a decision always goes through the confirmation
//! registry.

use serde::Serialize;

use crate::dto::ChatHistoryEntryDto;
use crate::session::Session;
use crate::state::ActionId;

/// History plus the still-waiting confirmation. `pending_proposal` has the
/// shape of the `chat-action-proposed` event payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResyncDto {
    pub history: Vec<ChatHistoryEntryDto>,
    pub pending_proposal: Option<serde_json::Value>,
}

impl Session {
    pub(crate) fn lock_proposal(&self) -> std::sync::MutexGuard<'_, crate::session::ChatTurnState> {
        ssh_manager_core::sync::lock_tolerating_poison(&self.conn.chat_turn)
    }

    /// Keeps the card data of a proposal that waits for confirmation (a
    /// `Confirm` decision); other decisions are not stored. Cleared with the
    /// confirmation (`ConfirmationCleanup`).
    pub(crate) fn remember_proposal(
        &self,
        action_id: ActionId,
        payload: Option<serde_json::Value>,
    ) {
        let Some(payload) = payload.filter(|p| p["decision"].get("Confirm").is_some()) else {
            return;
        };
        self.lock_proposal().pending_proposal = Some((action_id, payload));
    }

    /// The decision of a proposal sent as `AutoExec` was raised to `Confirm`.
    pub(crate) fn escalate_proposal(
        &self,
        action_id: ActionId,
        payload: Option<serde_json::Value>,
        reason: &str,
        code: &str,
    ) {
        if let Some(mut payload) = payload {
            payload["decision"] = serde_json::json!({"Confirm": {"reason": reason, "code": code}});
            self.lock_proposal().pending_proposal = Some((action_id, payload));
        }
    }

    /// The AI second opinion changed the data risk of the stored proposal.
    pub(crate) fn review_proposal(
        &self,
        action_id: ActionId,
        data_risk: ssh_manager_core::risk::RiskLevel,
        reason: &Option<String>,
    ) {
        let mut guard = self.lock_proposal();
        if let Some((id, payload)) = guard.pending_proposal.as_mut() {
            if *id == action_id {
                if let Some(risk) = payload.get_mut("riskAssessment") {
                    risk["dataRisk"] = serde_json::json!(data_risk);
                    risk["dataRiskReason"] = serde_json::json!(reason);
                    risk["aiReviewed"] = serde_json::json!(true);
                }
            }
        }
    }

    /// The card data of the waiting confirmation, if any.
    pub fn pending_proposal(&self) -> Option<(ActionId, serde_json::Value)> {
        self.lock_proposal().pending_proposal.clone()
    }
}
