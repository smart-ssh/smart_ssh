//! The host key decision of a connect (Spec 0007 §4/§6, Spec 0017, Spec 0069
//! A4, issues #37 and #546, ADR 0007): an unknown or changed key is shown to
//! the user, who trusts or rejects it. Only an explicit `Trust` for exactly
//! the registration (generation) this flow made stores the key; timeout,
//! cancel, abandon and reject never do.

use std::future::Future;

use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{ConnectLog, ConnectStepSnapshot, HostKeyDecision, HostKeyStore};

use crate::confirmation::{ConfirmationRegistry, RegistrationGeneration};
use crate::dto::HostKeyUserDecision;
use crate::error::{CommandError, CommandResult};
use crate::events::{
    emit_host_key_verification_ended, emit_host_key_verification_needed, EventEmitter, HostKeyKind,
    HostKeyPromptEndReason,
};
use crate::session::SessionManager;
use crate::state::SessionId;

/// Error of a connect that was cancelled by the user (`cancel_connect`).
pub fn connect_cancelled_error() -> CommandError {
    CommandError::with_code("Verbindungsaufbau abgebrochen", "SSH_CONNECT_CANCELLED")
}

/// What the flow needs from its surroundings.
pub struct HostKeyFlowContext<'a> {
    pub session_id: SessionId,
    pub server_id: ServerId,
    pub host_key_store: &'a dyn HostKeyStore,
    pub confirmations: &'a ConfirmationRegistry<SessionId, HostKeyUserDecision>,
    pub sessions: &'a SessionManager,
    pub emitter: &'a dyn EventEmitter,
    /// The step log of the running attempt.
    pub attempt_log: &'a ConnectLog,
    /// Reports a `connect-progress` snapshot (a no-op without a progress
    /// handle).
    pub progress: &'a (dyn Fn(Vec<ConnectStepSnapshot>) + Sync),
    /// An automatic reconnect never prompts and never trusts (issue #356).
    pub interactive: bool,
    /// How long to wait for the user.
    pub timeout: std::time::Duration,
}

/// Handles `ConnectOutcome::PendingHostKeyConfirmation`. `Ok(())` means the
/// user trusted the key, it is stored, and the caller retries with the same
/// target. Every other end is an error that already carries the step log:
/// `SSH_HOST_KEY_NOT_TRUSTED` (automatic reconnect or reject),
/// `SSH_HOST_KEY_CONFIRM_TIMEOUT`, `SSH_CONNECTION_ABANDONED` and
/// `SSH_CONNECT_CANCELLED` (`cancel` resolved while waiting).
#[expect(clippy::cognitive_complexity, reason = "split planned")]
pub async fn resolve_pending_host_key(
    ctx: HostKeyFlowContext<'_>,
    host: String,
    port: u16,
    raw_key: Vec<u8>,
    decision: HostKeyDecision,
    cancel: impl Future<Output = ()>,
) -> CommandResult<()> {
    let HostKeyFlowContext {
        session_id,
        server_id,
        host_key_store,
        confirmations,
        sessions,
        emitter,
        attempt_log,
        progress,
        interactive,
        timeout,
    } = ctx;
    if !interactive {
        // Issue #356, Spec 0107 §8: an automatic attempt
        // never opens the host key dialog and never trusts
        // a key; the user decides with the Reconnect button.
        tracing::info!(
            session_id = %session_id,
            host = %host,
            port,
            "automatic reconnect stops: host key needs confirmation",
        );
        return Err(CommandError::with_code(
            format!("Verbindung zu {host}:{port} nicht möglich (Host-Key nicht vertraut)"),
            "SSH_HOST_KEY_NOT_TRUSTED",
        )
        .with_connect_log(attempt_log.snapshot()));
    }
    let key_type = crate::host_key_store::offered_key_type(&raw_key);
    let (kind, fingerprint, expected_fingerprint) = match decision {
        HostKeyDecision::Unknown { fingerprint } => (HostKeyKind::Unknown, fingerprint, None),
        HostKeyDecision::Mismatch {
            expected_fingerprint,
            actual_fingerprint,
        } => (
            HostKeyKind::Mismatch,
            actual_fingerprint,
            Some(expected_fingerprint),
        ),
        HostKeyDecision::Trusted => {
            unreachable!("PendingHostKeyConfirmation wird nur für Unknown/Mismatch gebaut")
        }
    };

    tracing::info!(
        session_id = %session_id,
        host = %host,
        port,
        kind = ?kind,
        "host key verification needed",
    );

    let (generation, rx) = confirmations.register_tracked(session_id);
    // Spec 0017, Abschnitt 2: solange `connect()` hier auf die
    // Nutzerentscheidung wartet, existiert `session_id` noch in
    // keiner `Session` (die wird erst nach erfolgreichem
    // Aufbau eingefügt) — ohne diesen Eintrag würde ein
    // Frontend-Reload während eines offenen Host-Key-Dialogs den
    // zugehörigen Tab in der wiederhergestellten Tab-Leiste
    // verlieren.
    sessions.register_pending_connection(session_id, server_id);
    // Issue #546: the host key step waits for the decision.
    progress(ConnectStepSnapshot::from_records(
        &attempt_log.snapshot(),
        true,
    ));
    emit_host_key_verification_needed(
        emitter,
        session_id,
        generation.as_u64(),
        host.clone(),
        port,
        kind,
        fingerprint,
        expected_fingerprint,
        key_type,
    );

    let wait = tokio::select! {
        out = wait_for_host_key_decision(confirmations, session_id, generation, rx, timeout) => {
            Some(out)
        }
        () = cancel => None,
    };
    sessions.clear_pending_connection(session_id);
    let Some(wait) = wait else {
        // Issue #546: cancelled while the dialog is open.
        // Release the registration and tell the frontend the
        // prompt is over; the key is not trusted.
        confirmations.cancel_if_current(&session_id, generation);
        let _ = finish_host_key_wait(
            emitter,
            session_id,
            generation,
            HostKeyWait::Abandoned,
            &host,
            port,
        );
        attempt_log.fail_running("SSH_CONNECT_CANCELLED");
        progress(attempt_log.progress_snapshot());
        return Err(connect_cancelled_error().with_connect_log(attempt_log.snapshot()));
    };
    // Issue #37: jeder Ausgang des Wartens meldet dem
    // Frontend das Ende genau dieser Abfrage (`generation`),
    // damit ein offener Dialog nach Timeout/Abbruch von
    // selbst schließt. Timeout/Abbruch bleiben Ablehnung.
    let user_decision = finish_host_key_wait(emitter, session_id, generation, wait, &host, port)
        .map_err(|err| err.with_connect_log(attempt_log.snapshot()));
    // Issue #546: the dialog is over — a final, plain
    // snapshot (the host key step shows its real result)
    // unless the attempt is retried after Trust.
    let user_decision = match user_decision {
        Ok(d) => d,
        Err(err) => {
            progress(attempt_log.progress_snapshot());
            return Err(err);
        }
    };
    match user_decision {
        HostKeyUserDecision::Trust => {
            tracing::info!(session_id = %session_id, host = %host, port, "host key trusted");
            host_key_store.trust(&host, port, &raw_key)?;
            // Erneuter Versuch mit demselben `target` — s. ADR 0007.
            Ok(())
        }
        HostKeyUserDecision::Reject => {
            tracing::warn!(
                session_id = %session_id,
                host = %host,
                port,
                "host key rejected, connection aborted",
            );
            progress(attempt_log.progress_snapshot());
            // Spec 0069, Teil A4: Code zusätzlich zur
            // bisherigen, host:port-tragenden Meldung
            // (Text unverändert).
            Err(CommandError::with_code(
                format!("Verbindung zu {host}:{port} abgelehnt (Host-Key nicht vertraut)"),
                "SSH_HOST_KEY_NOT_TRUSTED",
            )
            .with_connect_log(attempt_log.snapshot()))
        }
    }
}

/// Ausgang des Wartens auf die Host-Key-Entscheidung (Spec 0068, Teil 5b).
#[derive(Debug, PartialEq, Eq)]
pub enum HostKeyWait {
    Decided(HostKeyUserDecision),
    /// Sender gedroppt (Neu-Registrierung, App-Ende) — Abbruch.
    Abandoned,
    /// Frist abgelaufen — gilt als Ablehnung, der Host-Key wird **nie**
    /// durch Zeitablauf vertraut.
    TimedOut,
}

/// Spec 0068, Teil 5b: wartet höchstens `timeout` auf `confirm_host_key`
/// (analog zum Confirm-Timeout aus Spec 0046, Fund 4). Geht ein Frontend
/// verloren, hängt `connect()` damit nicht mehr ewig. Beim Zeitablauf wird
/// nur der Eintrag DIESER Registrierung abgeräumt (`generation`), nie einer,
/// den ein `connect()`-Retry unter derselben `SessionId` neu angelegt hat.
pub async fn wait_for_host_key_decision(
    registry: &ConfirmationRegistry<SessionId, HostKeyUserDecision>,
    session_id: SessionId,
    generation: RegistrationGeneration,
    rx: tokio::sync::oneshot::Receiver<HostKeyUserDecision>,
    timeout: std::time::Duration,
) -> HostKeyWait {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(decision)) => HostKeyWait::Decided(decision),
        Ok(Err(_)) => HostKeyWait::Abandoned,
        Err(_elapsed) => {
            registry.cancel_if_current(&session_id, generation);
            HostKeyWait::TimedOut
        }
    }
}

/// Issue #37: schließt ein Warten auf die Host-Key-Entscheidung ab. Meldet
/// `host-key-verification-ended` für genau diese Registrierung
/// (`generation`) — bei jedem Ausgang, damit das Frontend die passende
/// Abfrage schließen kann — und bildet den Ausgang auf das Ergebnis von
/// `connect()` ab. Timeout und Abbruch bleiben Fehler (nie Vertrauen,
/// Spec 0068, Teil 5b); das Ereignis selbst entscheidet nichts.
pub fn finish_host_key_wait(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    generation: RegistrationGeneration,
    wait: HostKeyWait,
    host: &str,
    port: u16,
) -> CommandResult<HostKeyUserDecision> {
    let reason = match wait {
        HostKeyWait::Decided(_) => HostKeyPromptEndReason::Decided,
        HostKeyWait::Abandoned => HostKeyPromptEndReason::Abandoned,
        HostKeyWait::TimedOut => HostKeyPromptEndReason::TimedOut,
    };
    emit_host_key_verification_ended(emitter, session_id, generation.as_u64(), reason);
    match wait {
        HostKeyWait::Decided(decision) => Ok(decision),
        HostKeyWait::Abandoned => {
            // Spec 0069, Teil A5 (spec-reviewer-Fund, Review dieses
            // Schritts): der dritte Ausgang derselben Host-Key-Wartestelle
            // (neben Reject/TimedOut, die bereits einen Code tragen) zeigte
            // bislang rohen deutschen Text auch in der englischen UI. Neuer,
            // additiver Code (E3).
            Err(CommandError::with_code(
                "Verbindungsaufbau abgebrochen",
                "SSH_CONNECTION_ABANDONED",
            ))
        }
        HostKeyWait::TimedOut => {
            tracing::warn!(
                session_id = %session_id,
                host = %host,
                port,
                "host key confirmation timed out, treating as rejected",
            );
            // Spec 0069, Teil A4: Code zusätzlich zur bisherigen,
            // host:port-tragenden Meldung (Text unverändert).
            Err(CommandError::with_code(
                format!(
                    "Verbindung zu {host}:{port} abgebrochen: Host-Key-Bestätigung \
                     nicht rechtzeitig beantwortet (nicht vertraut)"
                ),
                "SSH_HOST_KEY_CONFIRM_TIMEOUT",
            ))
        }
    }
}

#[cfg(test)]
mod tests;
