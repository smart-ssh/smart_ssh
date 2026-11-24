//! Terminal output coalescing (issue #922).

use super::{EventEmitter, SessionId};

/// Issue #922: longest time terminal output is held back to be merged with
/// the following chunks into one `terminal-output` event.
pub const TERMINAL_COALESCE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Issue #922: size at which collected terminal output is emitted at once,
/// without waiting for [`TERMINAL_COALESCE_INTERVAL`].
pub const TERMINAL_COALESCE_MAX_BYTES: usize = 64 * 1024;

/// Capacity of the terminal command channel (keystrokes, resize). A sender
/// that would exceed it waits (issue #922).
pub const TERMINAL_COMMAND_CAPACITY: usize = 64;

pub(super) fn flush_terminal_output(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    pending: &mut Vec<u8>,
    flush_at: &mut Option<tokio::time::Instant>,
) {
    *flush_at = None;
    if !pending.is_empty() {
        crate::events::emit_terminal_output(emitter, session_id, pending);
        pending.clear();
    }
}
