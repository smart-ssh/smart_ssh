//! The one funnel that feeds ledger and activity observers (issue #1505).

use super::ledger_redaction::redact_ledger_entry_content;
use crate::session::Session;
use crate::state::ActionId;
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};

/// The one funnel: redacts `content` once and feeds the result to both the
/// ledger and the activity observers (issue #1505), so the two cannot
/// diverge. `ledger_link` is stored with the ledger entry, `activity_action`
/// goes into the event.
pub(crate) async fn write_ledger_entry_and_event(
    session: &Session,
    source: LedgerSource,
    content: LedgerEntryContent,
    ledger_link: Option<ActionId>,
    activity_action: Option<ActionId>,
) {
    // A chat session records under its own id; an MCP session (no chat
    // session) under its recorded session (issue #1444). Same redaction.
    let ledger_target = match &session.persistence.ledger_store {
        Some(store) => {
            let recorded_session_id = match *session.persistence.chat_session_id.lock().await {
                Some(id) => Some(id),
                None => *session.persistence.mcp_activity_session_id.lock().await,
            };
            recorded_session_id.map(|id| (store, id))
        }
        None => None,
    };
    if ledger_target.is_none() && !session.activity_active() {
        return;
    }
    let redacted = redact_ledger_entry_content(session.redactor.as_ref(), content);
    // The event is queued first: delivery is off this path, and the ledger
    // write below may take a moment.
    session
        .emit_activity_for_ledger_content(source, &redacted, activity_action)
        .await;
    let Some((store, chat_session_id)) = ledger_target else {
        return;
    };
    let meta = ssh_manager_core::audit::LedgerEntryMeta {
        action_id: ledger_link.map(|id| id.to_string()),
        ..Default::default()
    };
    if let Err(err) = store
        .append(chat_session_id, source, &redacted, &meta)
        .await
    {
        tracing::warn!(error = %err, "ledger entry persistence failed");
    }
}
