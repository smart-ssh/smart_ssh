//! The one funnel that feeds the activity observers (issue #1505, #1506).

use super::activity_redaction::redact_activity_content;
use crate::session::Session;
use crate::state::ActionId;
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};

/// The one funnel: redacts `content` once and feeds the result to the
/// activity observers. The core keeps no record of it (issue #1506); without
/// an observer nothing is built. `action` goes into the event.
pub(crate) async fn emit_activity_event(
    session: &Session,
    source: LedgerSource,
    content: LedgerEntryContent,
    action: Option<ActionId>,
) {
    if !session.activity_active() {
        return;
    }
    let redacted = redact_activity_content(session.redactor.as_ref(), content);
    session
        .emit_activity_for_content(source, &redacted, action)
        .await;
}
