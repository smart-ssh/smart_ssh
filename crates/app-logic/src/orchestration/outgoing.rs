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
use ssh_manager_core::filter::{matches_hard_blacklist, split_top_level_segments};
use ssh_manager_core::outgoing::{OutgoingKind, ServerScope};
use ssh_manager_core::profiles::ProfileStore;

use crate::dto::is_local;
use crate::session::Session;
use crate::session_extension::scope_for_id;

/// Shown to the AI when the prepared command hits the hard blacklist. Does
/// not quote the prepared text.
pub(crate) const PREPARED_BLOCKED_MESSAGE: &str =
    "The command was not sent: after preparation it matched the hard blacklist.";

/// Scope handed to the transform. Same lookup as the session extension
/// (`scope_for_id`), built at call time so a server moved to another group
/// gets its current chain. A failing group lookup leaves the chain empty
/// (logged there); a server that no longer exists falls back to the
/// session's own data.
pub(crate) async fn outgoing_scope(
    session: &Session,
    profile_store: &dyn ProfileStore,
) -> ServerScope {
    let local_tags = is_local(session.server_id).then(|| session.tags.clone());
    scope_for_id(profile_store, session.server_id, local_tags)
        .await
        .unwrap_or_else(|| ServerScope {
            server_id: session.server_id,
            group_chain: Vec::new(),
            tags: session.tags.clone(),
        })
}

/// Shown to the AI when the prepared command has another number of top-level
/// segments than the approved one. Does not quote the prepared text.
pub(crate) const PREPARED_STRUCTURE_MESSAGE: &str =
    "The command was not sent: after preparation its structure differed from the approved command.";

/// Defence in depth: a prepared command must keep the top-level structure of
/// the approved one. Uses the filter engine's own parser. Command
/// substitutions are deliberately not counted (the scanner ignores quotes).
pub(crate) fn same_segment_structure(original: &str, prepared: &str) -> bool {
    if original == prepared {
        return true;
    }
    match (
        split_top_level_segments(original),
        split_top_level_segments(prepared),
    ) {
        (Some(a), Some(b)) => a.len() == b.len(),
        _ => false,
    }
}

/// `Ok(None)`: no transform registered, send the original text unchanged.
/// `Ok(Some(text))`: send this text instead. `Err(message)`: send nothing;
/// the message goes to the AI as the action result.
pub(crate) async fn prepare_for_send(
    session: &Session,
    profile_store: &dyn ProfileStore,
    kind: OutgoingKind,
    text: &str,
) -> Result<Option<SecretString>, String> {
    let Some(transform) = session.outgoing_transform.as_ref() else {
        return Ok(None);
    };
    let scope = outgoing_scope(session, profile_store).await;
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
    //
    // Issue #697: an original that itself matches the hard blacklist reached
    // this point only because the user explicitly confirmed it (the filter
    // turns a hit into `Confirm`). Rejecting its prepared form would make that
    // confirmation lead nowhere, so the re-check applies only when the
    // original did NOT match. Decision: a confirmed blacklisted command that
    // the (trusted, edition) transform turns into another blacklisted command
    // is sent as well.
    if kind == OutgoingKind::Command {
        use secrecy::ExposeSecret;
        if matches_hard_blacklist(prepared.text().expose_secret()).await
            && !matches_hard_blacklist(text).await
        {
            return Err(PREPARED_BLOCKED_MESSAGE.to_string());
        }
        if !same_segment_structure(text, prepared.text().expose_secret()) {
            return Err(PREPARED_STRUCTURE_MESSAGE.to_string());
        }
    }
    // Before the command starts, so its own output is already masked.
    session.redactor.add_secret_values(prepared.secrets());
    Ok(Some(prepared.text().clone()))
}
