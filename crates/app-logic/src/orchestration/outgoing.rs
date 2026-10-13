//! Issue #464 (Spec 0038): the last step before text leaves for the server.
//!
//! [`prepare_for_send`] runs the edition's [`OutgoingTransform`] on the final
//! approved text. It is called only from the two execution functions of AI and
//! MCP actions (`execute_suggested_command`, `execute_write_remote_file`),
//! never on user terminal input. The prepared text is returned to the caller
//! as a [`SecretString`] and must go to the transport call only: not into an
//! event, the history, the ledger or a log line.

use secrecy::SecretString;

use ssh_manager_core::ai::OutputRedactor;
use ssh_manager_core::filter::matches_hard_blacklist;
use ssh_manager_core::outgoing::{OutgoingKind, OutgoingScope};

use crate::session::Session;

/// Shown to the AI when the prepared command hits the hard blacklist. Does
/// not quote the prepared text.
pub(crate) const PREPARED_BLOCKED_MESSAGE: &str =
    "The command was not sent: after preparation it matched the hard blacklist.";

/// `Ok(None)`: no transform registered, send the original text unchanged.
/// `Ok(Some(text))`: send this text instead. `Err(message)`: send nothing;
/// the message goes to the AI as the action result.
pub(crate) async fn prepare_for_send(
    session: &Session,
    kind: OutgoingKind,
    text: &str,
) -> Result<Option<SecretString>, String> {
    let Some(transform) = session.outgoing_transform.as_ref() else {
        return Ok(None);
    };
    let scope = OutgoingScope {
        server_id: session.server_id,
        tags: session.tags.clone(),
    };
    let prepared = match transform.prepare_outgoing(&scope, kind, text).await {
        Ok(prepared) => prepared,
        // The extension is trusted, but its message still goes through the
        // session redactor before it reaches the UI and the AI.
        Err(rejection) => return Err(session.redactor.redact_text(&rejection.message)),
    };
    // Defence in depth (hard blacklist only, never a user rule): the core
    // re-checks a transformed command so a faulty transform cannot produce a
    // blacklisted command from an approved one. A hit rejects; the message is
    // generic and the prepared text appears nowhere.
    if kind == OutgoingKind::Command {
        use secrecy::ExposeSecret;
        if matches_hard_blacklist(prepared.text().expose_secret()).await {
            return Err(PREPARED_BLOCKED_MESSAGE.to_string());
        }
    }
    // Before the command starts, so its own output is already masked.
    session.redactor.add_secret_values(prepared.secrets());
    Ok(Some(prepared.text().clone()))
}
