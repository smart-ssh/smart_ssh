//! Lookup helper for commands that act on an open session.

use std::sync::Arc;

use super::{Session, SessionManager};
use crate::error::{CommandError, CommandResult, SESSION_NOT_FOUND};
use crate::ids::SessionId;

/// The error for a session that is no longer open: stable code
/// [`SESSION_NOT_FOUND`] (issue #883), English message; the UI text comes
/// from the code.
pub fn session_not_found() -> CommandError {
    CommandError::with_code("session not found", SESSION_NOT_FOUND)
}

/// Looks up `id` or fails with [`session_not_found`].
pub fn require_session(sessions: &SessionManager, id: SessionId) -> CommandResult<Arc<Session>> {
    sessions.get(id).ok_or_else(session_not_found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #883: an unknown id yields the code, not a code-less message.
    #[test]
    fn test_issue_883_require_session_unknown_id_has_code() {
        let sessions = SessionManager::new();
        let err = require_session(&sessions, uuid::Uuid::new_v4())
            .err()
            .expect("unknown id");
        assert_eq!(err.code, Some(SESSION_NOT_FOUND));
        assert_eq!(err.message, "session not found");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "SESSION_NOT_FOUND");
    }
}
