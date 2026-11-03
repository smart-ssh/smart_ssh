//! Issue #546: progress reporting and cancel handling of a running `connect`.

use std::sync::Arc;

use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::ConnectLog;

use app_logic::error::CommandError;
use app_logic::events::{emit_connect_progress, EventEmitter};
use app_logic::state::AppState;

/// Issue #546: what a running `connect` needs to report its progress and to
/// notice a cancel.
pub(crate) struct ConnectProgress {
    attempt_id: String,
    emitter: Arc<dyn EventEmitter>,
    guard: app_logic::connect_attempts::ConnectAttemptGuard,
}

impl ConnectProgress {
    pub(crate) fn new(
        state: &AppState,
        attempt_id: String,
        emitter: Arc<dyn EventEmitter>,
    ) -> Self {
        let guard = state.connect_attempts.register(&attempt_id);
        Self {
            attempt_id,
            emitter,
            guard,
        }
    }

    /// Resolves once this attempt is cancelled (`cancel_connect`).
    pub(super) async fn cancelled(&self) {
        self.guard.cancelled().await;
    }

    pub(super) fn emit(
        &self,
        server_id: ServerId,
        attempt: u32,
        steps: Vec<ssh_manager_core::ssh::ConnectStepSnapshot>,
    ) {
        emit_connect_progress(
            self.emitter.as_ref(),
            &self.attempt_id,
            server_id,
            attempt,
            steps,
        );
    }

    /// A step log that reports every change as a `connect-progress` event.
    pub(super) fn log(&self, server_id: ServerId, attempt: u32) -> ConnectLog {
        let emitter = self.emitter.clone();
        let attempt_id = self.attempt_id.clone();
        ConnectLog::with_sink(Arc::new(move |steps| {
            emit_connect_progress(emitter.as_ref(), &attempt_id, server_id, attempt, steps);
        }))
    }
}

pub(super) fn cancelled_error() -> CommandError {
    app_logic::connect::host_key_flow::connect_cancelled_error()
}

/// Runs `fut` unless the attempt is cancelled first (`None`). Without a
/// progress handle there is nothing to cancel. `fut` is dropped on cancel,
/// which releases its sockets and tunnel channels.
pub(super) async fn or_cancelled<F: std::future::Future>(
    progress: Option<&ConnectProgress>,
    fut: F,
) -> Option<F::Output> {
    match progress {
        Some(p) => tokio::select! {
            out = fut => Some(out),
            () = p.guard.cancelled() => None,
        },
        None => Some(fut.await),
    }
}
