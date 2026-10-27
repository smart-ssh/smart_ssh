//! Extension API of `app-logic` (Spec 0038, "Extension API").
//!
//! This module is the **supported surface for extension crates** that plug
//! into an embedding app through `app_shell::Wiring`. Items in here change
//! only together with a changelog entry and the spec. Everything outside
//! this module (the other `pub mod` of the crate, the public fields of
//! [`AppState`]) may change without notice.
//!
//! Everything is re-exported, nothing is moved: the original paths keep
//! working. The `AppState` fields stay `pub` for `app-shell`; extensions use
//! the accessor functions below instead.
//!
//! Closing a session is not part of this module: the one public way for an
//! extension to close sessions is the session close path of `app-shell`.

use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::shared::ServerId;

// --- State ---------------------------------------------------------------
// The type only (for `app.try_state::<AppState>()`); its fields are not part
// of the facade.
pub use crate::state::AppState;

// --- Session extension ---------------------------------------------------
pub use crate::session_extension::{
    rebuild_redactors_for_sessions, rebuild_server_redactors, scope_for_id, ServerScope,
    SessionExtension,
};
pub use crate::system_prompt::PromptLanguage;

// --- Chat content --------------------------------------------------------
pub use crate::chat_content_extension::{
    ChatContentError, ChatContentExtension, PendingContent, RedactedBlock, ResolvedBlock,
};
pub use crate::orchestration::accept_extension_content;

// --- AI usage ------------------------------------------------------------
pub use crate::ai_usage::{AiUsageObserver, AiUsagePurpose, AiUsageRecord};

// --- Backup / snapshot ---------------------------------------------------
pub use crate::backup::{
    apply_changes, start_import_snapshot, BackupOpError, Change, ChangeSet, HostKeyRow,
    OpenSessions, Snapshot, KNOWN_SECTIONS,
};
pub use crate::host_key_store::FileHostKeyStore;

// --- Events --------------------------------------------------------------
pub use crate::events::{emit_connection_status_changed, ConnectionStatus, EventEmitter};

// --- Errors and helpers --------------------------------------------------
pub use crate::dto::{is_local, DocumentFormat};
pub use crate::error::CommandError;

/// The profile store (servers, groups, notes).
pub fn profile_store(state: &AppState) -> &dyn ProfileStore {
    state.profile_store.as_ref()
}

/// The backup store: consistent snapshot and backup key access.
pub fn backup_store(state: &AppState) -> &persistence_sqlite::SqliteBackupStore {
    &state.backup_store
}

/// The file-backed store of trusted host keys.
pub fn file_host_keys(state: &AppState) -> &FileHostKeyStore {
    state.file_host_keys.as_ref()
}

/// Ids of the servers that have at least one open session, each once,
/// sorted by id.
pub fn open_session_server_ids(state: &AppState) -> Vec<ServerId> {
    server_ids_of(&state.sessions)
}

/// Rebuilds the redactor of every server that has an open session (and of
/// no other), e.g. after the secrets an extension knows have changed. Goes
/// through [`rebuild_server_redactors`]; literal secrets only extend the
/// redactor.
pub async fn rebuild_redactors_for_open_sessions(state: &AppState) {
    for server_id in open_session_server_ids(state) {
        rebuild_server_redactors(state, server_id).await;
    }
}

pub(crate) fn server_ids_of(sessions: &crate::session::SessionManager) -> Vec<ServerId> {
    let mut ids: Vec<ServerId> = sessions
        .snapshot()
        .into_iter()
        .map(|e| e.server_id)
        .collect();
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

/// Test doubles, for tests of extension crates.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    pub use crate::test_support::*;
}
