//! Issue #816: the one public way to close an open session completely.
//!
//! [`close_session`] revokes the elevated file-browser channel, removes the
//! session's local path grants, rejects a pending confirmation, cancels a
//! running AI command and waits for it, disconnects the transport, sends the
//! `Disconnected` status event and removes the edit-copy directory — in that
//! order. The `disconnect` command, the backup import ([`AppOpenSessions`])
//! and any embedding app crate or extension use this path and no other.
//!
//! **Embedding crates must not** call `SessionManager::remove` themselves:
//! it skips the revocation of the elevated channel and the other steps, so a
//! "closed" session would keep an elevated channel or grants alive. A guard
//! test scans the sources for such calls (Spec 0084, A2.1).
//!
//! The elevated-channel registry itself stays crate-private; only this
//! complete sequence is public.

use std::sync::Arc;
use std::time::Duration;

use app_logic::events::{emit_connection_status_changed, ConnectionStatus, EventEmitter};
use app_logic::local_path_grants::LocalPathGrants;
use app_logic::session::{Session, SessionManager};
use app_logic::state::{AppState, SessionId};
use tauri::{AppHandle, Manager, Runtime};

use crate::elevated_sftp::ElevatedSftpRegistry;
use crate::event_emitter::TauriEventEmitter;

/// Upper bound for each of "running command winds down" and "transport
/// disconnects", so closing never hangs on a dead connection.
const CLOSE_STEP_TIMEOUT: Duration = Duration::from_secs(2);

/// Everything the close sequence touches, borrowed from the app state.
pub(crate) struct CloseContext<'a> {
    pub(crate) sessions: &'a SessionManager,
    pub(crate) elevated: &'a ElevatedSftpRegistry,
    pub(crate) grants: &'a LocalPathGrants,
    pub(crate) mcp_sessions: &'a app_logic::mcp_sessions::McpSessionRegistry,
    pub(crate) confirmations: &'a app_logic::confirmation::ConfirmationRegistry<
        app_logic::state::ActionId,
        app_logic::dto::ActionUserDecision,
    >,
    pub(crate) cancellations:
        &'a app_logic::confirmation::ConfirmationRegistry<app_logic::state::ActionId, ()>,
    pub(crate) emitter: &'a dyn EventEmitter,
}

impl<'a> CloseContext<'a> {
    pub(crate) fn new(
        state: &'a AppState,
        elevated: &'a ElevatedSftpRegistry,
        grants: &'a LocalPathGrants,
        emitter: &'a dyn EventEmitter,
    ) -> Self {
        Self {
            sessions: &state.sessions,
            elevated,
            grants,
            mcp_sessions: &state.mcp.sessions,
            confirmations: &state.pending_action_confirmations,
            cancellations: &state.running_command_cancellations,
            emitter,
        }
    }

    /// The full close sequence. Returns the removed session (`None` if there
    /// was none — a pending MCP session entry is still cleaned up then).
    pub(crate) async fn close(&self, session_id: SessionId) -> Option<Arc<Session>> {
        // Spec 0084, A2.1: session AND elevated channel go in this one
        // call; nothing else calls `SessionManager::remove`.
        let session = self.elevated.remove_session(self.sessions, session_id);
        // Issue #356: closing ends automatic reconnect attempts.
        if let Some(session) = &session {
            session.auto_reconnect.stop();
        }
        // Issue #89: local path grants expire with their session.
        self.grants.remove_session(session_id);
        // Spec 0017 §5 / Spec 0104 §5: reject the pending confirmation
        // before disconnecting (fail closed); also ends an MCP session when
        // no `Session` was found (closed during the host-key dialog).
        app_logic::session::reject_pending_confirmation_on_close(
            session_id,
            session.as_deref(),
            self.mcp_sessions,
            self.confirmations,
        );
        let session = session?;

        // Best-effort from here: a failing disconnect (connection already
        // dead) never keeps the session open; it is already removed.
        // Issue #328: cancel a running AI command (Spec 0027) and give it a
        // moment to wind down before disconnecting.
        session.transport.mark_closed();
        if let Some(action_id) = session.transport.running_action() {
            let _ = self.cancellations.resolve(&action_id, ());
        }
        let _ = tokio::time::timeout(CLOSE_STEP_TIMEOUT, session.transport.wait_until_idle()).await;
        let _ = tokio::time::timeout(CLOSE_STEP_TIMEOUT, async {
            session.transport.lock().await.disconnect().await
        })
        .await;
        // Dropping the sender ends the terminal actor on its next receive
        // without a second status event.
        *session.terminal.lock().unwrap() = None;

        tracing::info!(session_id = %session_id, "session disconnected");
        emit_connection_status_changed(
            self.emitter,
            session_id,
            ConnectionStatus::Disconnected,
            None,
        );

        // Spec 0054, part 4, item 6: leftover "open locally" copies of this
        // session. Best-effort; the directory usually does not exist.
        if let Ok(dir) = crate::commands::edit_session_dir(session_id) {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
        Some(session)
    }
}

/// Closes the session completely (see the module docs) and returns the
/// removed `Session` for callers with disconnect-specific follow-up work.
pub(crate) async fn close_session_returning<R: Runtime>(
    app: &AppHandle<R>,
    session_id: SessionId,
) -> Option<Arc<Session>> {
    let state = app.state::<AppState>();
    let elevated = app.state::<ElevatedSftpRegistry>();
    let grants = app.state::<LocalPathGrants>();
    let emitter = TauriEventEmitter(app.clone());
    CloseContext::new(&state, &elevated, &grants, &emitter)
        .close(session_id)
        .await
}

/// The only supported way for an embedding app crate or extension to close
/// an open session. Runs the complete sequence (elevated channel revoked,
/// local path grants removed, pending confirmation rejected, running command
/// cancelled and drained, transport disconnected, `Disconnected` event,
/// edit-copy directory removed). Returns `true` if a session was closed.
///
/// Never remove sessions with `SessionManager::remove` instead.
pub async fn close_session<R: Runtime>(app: &AppHandle<R>, session_id: SessionId) -> bool {
    close_session_returning(app, session_id).await.is_some()
}

/// Implements [`app_logic::backup::OpenSessions`] for the backup import: it
/// closes every open session except the local pseudo-server through
/// [`close_session`].
pub struct AppOpenSessions<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> AppOpenSessions<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl<R: Runtime> app_logic::backup::OpenSessions for AppOpenSessions<R> {
    fn affected(&self) -> usize {
        self.app.state::<AppState>().sessions.affected()
    }
    async fn close_affected(&self) {
        let state = self.app.state::<AppState>();
        let elevated = self.app.state::<ElevatedSftpRegistry>();
        let grants = self.app.state::<LocalPathGrants>();
        let emitter = TauriEventEmitter(self.app.clone());
        close_all_remote(&CloseContext::new(&state, &elevated, &grants, &emitter)).await;
    }
}

/// Closes every open session except the local pseudo-server (nil server
/// id), each through the full close sequence.
pub(crate) async fn close_all_remote(ctx: &CloseContext<'_>) {
    let ids: Vec<SessionId> = ctx
        .sessions
        .snapshot()
        .into_iter()
        .filter(|entry| !entry.server_id.0.is_nil())
        .map(|entry| entry.session_id)
        .collect();
    for id in ids {
        ctx.close(id).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use app_logic::confirmation::ConfirmationRegistry;
    use app_logic::dto::ActionUserDecision;
    use app_logic::events::TestEmitter;
    use app_logic::mcp_sessions::McpSessionRegistry;
    use app_logic::state::ActionId;
    use async_trait::async_trait;
    use ssh_manager_core::ssh::{
        CommandOutput, InteractiveShell, PtySize, SftpSession, SshError, SshTransport,
    };

    use super::*;
    use crate::elevated_sftp::enable;
    use crate::file_ops::OpKind;
    use crate::test_support::elevation::{access, fixture, working_transport, ElevationFixture};

    /// Everything a close touches besides the fixture's sessions/registry.
    struct Rig {
        f: ElevationFixture,
        grants: LocalPathGrants,
        mcp: McpSessionRegistry,
        confirmations: ConfirmationRegistry<ActionId, ActionUserDecision>,
        cancellations: ConfirmationRegistry<ActionId, ()>,
        emitter: TestEmitter,
    }

    impl Rig {
        fn new(transport: Box<dyn SshTransport>) -> Self {
            Self {
                f: fixture(transport),
                grants: LocalPathGrants::new(),
                mcp: McpSessionRegistry::new(),
                confirmations: ConfirmationRegistry::new(),
                cancellations: ConfirmationRegistry::new(),
                emitter: TestEmitter::default(),
            }
        }

        fn ctx(&self) -> CloseContext<'_> {
            CloseContext {
                sessions: &self.f.sessions,
                elevated: &self.f.registry,
                grants: &self.grants,
                mcp_sessions: &self.mcp,
                confirmations: &self.confirmations,
                cancellations: &self.cancellations,
                emitter: &self.emitter,
            }
        }
    }

    /// An elevated session with a grant, a pending confirmation and a
    /// running elevated file operation; returns the rig, the elevated slot,
    /// the operation guard and the receiver of the pending confirmation.
    async fn busy_rig() -> (
        Rig,
        crate::elevated_sftp::ElevatedSftpSlot,
        crate::file_ops::OpGuard,
        tokio::sync::oneshot::Receiver<ActionUserDecision>,
    ) {
        let rig = Rig::new(Box::new(working_transport()));
        enable(&rig.f.ctx(), "deploy", None, None, false, &access())
            .await
            .expect("precondition: enabling succeeds");
        let slot = rig
            .f
            .registry
            .slot(rig.f.session_id, &access())
            .expect("precondition: elevated slot");
        let op = rig.f.registry.ops().register(
            rig.f.session_id,
            uuid::Uuid::new_v4(),
            OpKind::Download,
            "/etc/shadow",
            true,
        );
        rig.grants
            .grant(rig.f.session_id, vec![std::env::temp_dir()]);
        assert!(rig
            .grants
            .check(rig.f.session_id, &std::env::temp_dir(), None)
            .is_ok());
        let action_id = uuid::Uuid::new_v4();
        let rx = rig.confirmations.register(action_id);
        *rig.f.session.pending_action.lock().unwrap() = Some(action_id);
        (rig, slot, op, rx)
    }

    async fn assert_fully_closed(
        rig: &Rig,
        slot: &crate::elevated_sftp::ElevatedSftpSlot,
        op: &crate::file_ops::OpGuard,
        rx: tokio::sync::oneshot::Receiver<ActionUserDecision>,
    ) {
        assert!(slot.is_revoked(), "the elevated slot must be revoked");
        assert_eq!(
            op.handle().stop_reason(),
            Some(crate::file_ops::StopReason::Revoked),
            "elevated operations must be stopped with Revoked"
        );
        assert!(
            rig.f.registry.slot(rig.f.session_id, &access()).is_none(),
            "no elevated entry may survive its session"
        );
        assert!(rig.f.sessions.get(rig.f.session_id).is_none());
        assert!(
            rig.grants
                .check(rig.f.session_id, &std::env::temp_dir(), None)
                .is_err(),
            "local path grants must be gone"
        );
        assert!(
            matches!(rx.await, Ok(ActionUserDecision::Deny)),
            "the pending confirmation must be rejected"
        );
        assert!(
            rig.emitter
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|(name, _)| name == "connection-status-changed"),
            "the status event must be sent"
        );
    }

    #[tokio::test]
    async fn test_close_revokes_channel_removes_grants_and_rejects_confirmation() {
        let (rig, slot, op, rx) = busy_rig().await;
        let closed = rig.ctx().close(rig.f.session_id).await;
        assert!(closed.is_some());
        assert_fully_closed(&rig, &slot, &op, rx).await;
    }

    /// Issue #816: the import path used to skip the grants (and the running
    /// command). It now goes through the same close.
    #[tokio::test]
    async fn test_backup_import_close_affected_does_the_full_close() {
        let (rig, slot, op, rx) = busy_rig().await;
        // The local pseudo-server (nil server id) is skipped; this fixture
        // session has a real server id.
        close_all_remote(&rig.ctx()).await;
        assert_fully_closed(&rig, &slot, &op, rx).await;
    }

    /// Issue #974: `close_all_remote` closes every remote session but leaves
    /// the local pseudo-server session (nil server id) open and untouched.
    #[tokio::test]
    async fn test_close_all_remote_keeps_the_local_pseudo_server_session() {
        let (rig, slot, op, rx) = busy_rig().await;
        let local = Arc::new(
            app_logic::test_support::session_with_transport_and_server_id(
                Box::new(working_transport()),
                app_logic::dto::LOCAL_SERVER_ID,
            ),
        );
        let local_id = SessionId::new_v4();
        rig.f.sessions.insert(local_id, local);
        rig.grants.grant(local_id, vec![std::env::temp_dir()]);

        close_all_remote(&rig.ctx()).await;

        assert_fully_closed(&rig, &slot, &op, rx).await;
        assert!(
            rig.f.sessions.get(local_id).is_some(),
            "the pseudo-server session must stay in the registry"
        );
        assert!(
            rig.grants
                .check(local_id, &std::env::temp_dir(), None)
                .is_ok(),
            "the pseudo-server session's grants must stay"
        );
        let events = rig.emitter.events.lock().unwrap();
        let status_events = events
            .iter()
            .filter(|(name, _)| name == "connection-status-changed")
            .count();
        assert_eq!(
            status_events, 1,
            "only the remote session gets a status event"
        );
        let local_text = local_id.to_string();
        assert!(
            events
                .iter()
                .all(|(_, payload)| !payload.to_string().contains(&local_text)),
            "no event may mention the pseudo-server session"
        );
    }

    #[tokio::test]
    async fn test_close_of_unknown_session_returns_none() {
        let rig = Rig::new(Box::new(working_transport()));
        assert!(rig.ctx().close(uuid::Uuid::new_v4()).await.is_none());
        assert!(rig.f.sessions.get(rig.f.session_id).is_some());
    }

    struct OrderTransport {
        idle: Arc<AtomicBool>,
        disconnected_while_busy: Arc<AtomicBool>,
    }

    #[async_trait]
    impl SshTransport for OrderTransport {
        async fn execute(&mut self, _c: &str) -> Result<CommandOutput, SshError> {
            unreachable!()
        }
        async fn open_shell(&mut self, _s: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
            unreachable!()
        }
        async fn open_sftp_via_exec(&mut self, _c: &str) -> Result<Box<dyn SftpSession>, SshError> {
            unreachable!()
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            if !self.idle.load(Ordering::SeqCst) {
                self.disconnected_while_busy.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
    }

    /// Issue #816: a close (as run by the backup import) while an AI command
    /// runs cancels it and waits for it to wind down before disconnecting.
    #[tokio::test]
    async fn test_close_cancels_running_command_and_waits_for_idle_before_disconnect() {
        let idle = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let rig = Rig::new(Box::new(OrderTransport {
            idle: idle.clone(),
            disconnected_while_busy: busy.clone(),
        }));
        let action_id = uuid::Uuid::new_v4();
        let cancelled = rig.cancellations.register(action_id);
        let session = rig.f.session.clone();
        let (idle2, session2) = (idle.clone(), session.clone());
        let command = tokio::spawn(async move {
            let slot = session2.transport.begin_command(action_id).await;
            cancelled.await.expect("the command must be cancelled");
            // Winding down takes a moment; the close must wait for it.
            tokio::task::yield_now().await;
            idle2.store(true, Ordering::SeqCst);
            drop(slot);
        });
        // The command must hold its slot before the close starts.
        while session.transport.running_action().is_none() {
            tokio::task::yield_now().await;
        }
        tokio::time::timeout(Duration::from_secs(10), close_all_remote(&rig.ctx()))
            .await
            .expect("close must finish");
        command.await.unwrap();
        assert!(idle.load(Ordering::SeqCst));
        assert!(
            !busy.load(Ordering::SeqCst),
            "the transport must not be disconnected before the command is idle"
        );
    }

    // ---- Guard (Spec 0084, A2.1): nobody but the close path removes sessions.

    /// Workspace-relative paths allowed to call `SessionManager::remove`:
    /// the single removal point, the manager's own definition and the MCP
    /// session map (a different map that happens to be called `sessions`).
    const REMOVE_ALLOWLIST: &[&str] = &[
        "app-shell/src/elevated_sftp.rs",
        // Holds the scan itself (its self-test plants call text).
        "app-shell/src/sessions.rs",
        "app-logic/src/session.rs",
        "app-logic/src/mcp_sessions.rs",
    ];

    fn is_test_file(rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        rel.contains("/tests/")
            || name == "tests.rs"
            || name == "test_support.rs"
            || name.ends_with("_tests.rs")
    }

    /// Calls of `SessionManager::remove` in `source` (whitespace-insensitive,
    /// so a call split over lines is found too).
    fn removes_session(source: &str) -> bool {
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        compact.contains("sessions.remove(") || compact.contains("SessionManager::remove(")
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "target") {
                    rust_files(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn test_guard_scan_finds_a_planted_remove_call() {
        assert!(removes_session("state.sessions.remove(id);"));
        assert!(removes_session("state\n    .sessions\n    .remove(id);"));
        assert!(removes_session("SessionManager::remove(&m, id)"));
        assert!(!removes_session("registry.remove_session(&sessions, id);"));
    }

    #[test]
    fn test_only_the_close_path_removes_sessions() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        rust_files(&crates, &mut files);
        assert!(files.len() > 100, "the scan must see the workspace sources");
        let mut offenders = Vec::new();
        for file in files {
            let rel = file
                .strip_prefix(&crates)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_file(&rel) || REMOVE_ALLOWLIST.contains(&rel.as_str()) {
                continue;
            }
            if removes_session(&std::fs::read_to_string(&file).unwrap()) {
                offenders.push(rel);
            }
        }
        assert!(
            offenders.is_empty(),
            "SessionManager::remove must only be called by ElevatedSftpRegistry::remove_session \
             (use app_shell::sessions::close_session): {offenders:?}"
        );
    }
}
