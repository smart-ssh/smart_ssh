//! Issue #1173: `connect-warnings` event.

use serde::Serialize;

use super::{emit, EventEmitter};
use crate::event_names as names;
use crate::state::SessionId;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ConnectWarningsPayload {
    session_id: SessionId,
    warnings: Vec<ssh_manager_core::ssh::ConnectWarning>,
    /// Number of hops of the connection; the UI names the hop in a chain.
    hop_count: usize,
}

/// Issue #1173: the connection of `session_id` is established, but some hop
/// uses outdated SSH algorithms. Emitted once per established connection
/// (also a reconnect), only when `warnings` is not empty. UI only: the
/// connection is not affected.
pub fn emit_connect_warnings(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    warnings: Vec<ssh_manager_core::ssh::ConnectWarning>,
    hop_count: usize,
) {
    if warnings.is_empty() {
        return;
    }
    emit(
        emitter,
        names::CONNECT_WARNINGS,
        &ConnectWarningsPayload {
            session_id,
            warnings,
            hop_count,
        },
    );
}
