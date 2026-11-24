//! Emission of activity events from a [`Session`] (issue #1505, see
//! `crate::activity`). Kept in the session module so `activity` does not
//! depend on `session`.

use ssh_manager_core::ai::OutputRedactor;
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::filter::EvalContext;

use chrono::Utc;
use ssh_manager_core::sync::lock_tolerating_poison;

use super::Session;
use crate::activity::{
    ActivityEvent, ActivityEventKind, ActivitySessionKind, ActivitySource, ConnectionChange,
    RuleSnapshot, SessionEndReason, SessionRef,
};

/// How often the user-activity tick looks at the session.
pub const USER_ACTIVITY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

impl Session {
    /// Builds and queues one event. Callers check [`Session::activity_active`]
    /// before they build `kind`, so nothing is built without an observer.
    async fn push_activity(&self, source: ActivitySource, kind: ActivityEventKind) {
        let Some(reporting) = self.activity_reporting() else {
            return;
        };
        // The chat id is only known to observers from `ChatStarted` on.
        // The flag is read under the chat id lock, which "new chat" holds while
        // it swaps the id and resets the flag, so an unannounced id never leaks.
        let chat_session_id = {
            let id = self.persistence.chat_session_id.lock().await;
            if self
                .activity_chat_announced
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                (*id).map(|i| i.to_string())
            } else {
                None
            }
        };
        let is_end = matches!(kind, ActivityEventKind::SessionEnded { .. });
        let is_start = matches!(kind, ActivityEventKind::SessionStarted { .. });
        #[cfg(test)]
        crate::activity::CONSTRUCTED.with(|c| c.set(c.get() + 1));
        // Held across the check and the push, so no event can slip in after
        // the end event is queued (issue #1539).
        let mut ended = lock_tolerating_poison(&self.activity_ended);
        if *ended {
            return;
        }
        // Issue #1545: no start once the exit began, or it would never get
        // its end. The started flag is set under the same lock, so the exit
        // sequence sees either a complete start or none.
        if is_start {
            if self
                .activity_exiting
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return;
            }
            self.activity_started
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        *ended = is_end;
        reporting.hub.push(ActivityEvent {
            at: Utc::now(),
            session: SessionRef {
                runtime_session_id: reporting.runtime_session_id.clone(),
                chat_session_id,
                server_id: (!crate::dto::is_local(self.server_id))
                    .then(|| self.server_id.0.to_string()),
            },
            source,
            redaction_incomplete: self
                .safety
                .redaction_incomplete
                .load(std::sync::atomic::Ordering::SeqCst),
            kind,
        });
    }

    /// `true` if an observer listens to this session.
    pub(crate) fn activity_active(&self) -> bool {
        self.activity_reporting().is_some()
    }

    /// Tells the observers that this session started.
    pub async fn emit_session_started(&self, kind: ActivitySessionKind, mcp_client: Option<&str>) {
        let Some(reporting) = self.activity_reporting() else {
            return;
        };
        let source = match kind {
            ActivitySessionKind::Terminal => ActivitySource::User,
            ActivitySessionKind::Mcp => ActivitySource::McpAgent,
        };
        let kind = ActivityEventKind::SessionStarted {
            kind,
            mcp_client: mcp_client.map(str::to_string),
            server: reporting.server.clone(),
            time_zone: reporting.time_zone.clone(),
        };
        self.push_activity(source, kind).await;
    }

    /// Issue #1545: from now on `emit_session_started` queues nothing. Takes
    /// the lock that `push_activity` holds across a start, so a start either
    /// finished before this returns (and is visible via `activity_started`)
    /// or is refused.
    pub(crate) fn refuse_activity_start(&self) {
        let _guard = lock_tolerating_poison(&self.activity_ended);
        self.activity_exiting
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// `true` once `SessionStarted` was queued for this session.
    pub(crate) fn activity_started(&self) -> bool {
        self.activity_started
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Forgets that the chat was announced; called by "new chat" while it
    /// holds the chat id lock.
    pub fn reset_chat_announced(&self) {
        self.activity_chat_announced
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Tells the observers that the first chat message of this session
    /// arrived; only the first call per session emits (a reconnect or a
    /// resumed chat keeps the session, so it does not repeat). Sessions
    /// without a chat session (local pseudo-server) emit nothing.
    pub async fn emit_chat_started_once(&self) {
        if !self.activity_active() || !self.activity_started() {
            return;
        }
        let Some(chat_id) = *self.persistence.chat_session_id.lock().await else {
            return;
        };
        if self
            .activity_chat_announced
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        self.push_activity(
            ActivitySource::User,
            ActivityEventKind::ChatStarted {
                chat_session_id: chat_id.to_string(),
            },
        )
        .await;
    }

    /// Tells the observers that the connection of this session changed.
    pub async fn emit_connection(&self, change: ConnectionChange, source: ActivitySource) {
        if self.activity_active() {
            self.push_activity(source, ActivityEventKind::Connection { change })
                .await;
        }
    }

    /// Notes that the user did something in this session (terminal input,
    /// chat message, confirmation). Only sets a flag: it is cheap enough for
    /// the keystroke path, stores nothing about the input and does nothing
    /// without an observer.
    pub fn note_user_input(&self) {
        if let Some(reporting) = self.activity_reporting() {
            reporting
                .input_seen
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Tells the observers that a file write failed after it started.
    pub(crate) async fn emit_file_write_failed(
        &self,
        source: LedgerSource,
        action_id: uuid::Uuid,
        path: &str,
        error_code: &str,
    ) {
        if !self.activity_active() {
            return;
        }
        let kind = ActivityEventKind::FileWriteFailed {
            action_id: action_id.to_string(),
            path: self.redactor.redact_text(path),
            error_code: error_code.to_string(),
        };
        self.push_activity(source.into(), kind).await;
    }

    /// Tells the observers that this session ended.
    pub async fn emit_session_ended(&self, reason: SessionEndReason) {
        if self.activity_active() {
            self.push_activity(
                ActivitySource::User,
                ActivityEventKind::SessionEnded { reason },
            )
            .await;
        }
    }

    /// Emits the event matching `redacted`, content that already went
    /// through the central ledger redaction. Content kinds without an event
    /// (connection events, activity ticks) emit nothing.
    pub(crate) async fn emit_activity_for_content(
        &self,
        source: LedgerSource,
        redacted: &LedgerEntryContent,
        action_id: Option<uuid::Uuid>,
    ) {
        if !self.activity_active() {
            return;
        }
        let action = action_id.map(|id| id.to_string());
        let kind = match redacted {
            LedgerEntryContent::AiMessage { text } => {
                ActivityEventKind::AiMessage { text: text.clone() }
            }
            LedgerEntryContent::CommandProposed { command } => {
                let Some(action_id) = action else {
                    return missing_action_id();
                };
                ActivityEventKind::CommandProposed {
                    action_id,
                    command: command.clone(),
                }
            }
            LedgerEntryContent::Decision {
                outcome,
                reason,
                code,
                matched_rule,
                matched_rule_origin: _,
            } => {
                let Some(action_id) = action else {
                    return missing_action_id();
                };
                let rule = match matched_rule {
                    Some(id) => self.rule_snapshot(id).await,
                    None => None,
                };
                ActivityEventKind::Decision {
                    action_id,
                    outcome: (*outcome).into(),
                    code: code.clone(),
                    // The reason is generated by the filter, but may quote
                    // part of the command: masked like any other text.
                    reason: reason.as_deref().map(|r| self.redactor.redact_text(r)),
                    rule,
                }
            }
            LedgerEntryContent::CommandExecuted {
                command,
                output,
                cancelled,
            } => {
                let Some(action_id) = action else {
                    return missing_action_id();
                };
                ActivityEventKind::CommandExecuted {
                    action_id,
                    command: command.clone(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    exit_code: output.exit_code,
                    truncated: output.truncated,
                    cancelled: *cancelled,
                }
            }
            LedgerEntryContent::FileRead {
                path,
                bytes,
                sha256,
            } => {
                let Some(action_id) = action else {
                    return missing_action_id();
                };
                ActivityEventKind::FileRead {
                    action_id,
                    path: path.clone(),
                    size: *bytes,
                    sha256: sha256.clone(),
                }
            }
            LedgerEntryContent::FileWritten {
                path,
                backup_path,
                bytes_before,
                bytes_after,
                sha256_before,
                sha256_after,
                sudo,
            } => {
                let Some(action_id) = action else {
                    return missing_action_id();
                };
                ActivityEventKind::FileWritten {
                    action_id,
                    path: path.clone(),
                    backup_path: backup_path.clone(),
                    size_before: *bytes_before,
                    size_after: *bytes_after,
                    sha256_before: sha256_before.clone(),
                    sha256_after: sha256_after.clone(),
                    sudo: *sudo,
                }
            }
            // Replaced by SessionStarted/SessionEnded; no content at all.
            LedgerEntryContent::Connection { .. } | LedgerEntryContent::ActivityTick => return,
        };
        self.push_activity(source.into(), kind).await;
    }

    /// The rule as currently stored; `None` if it was deleted meanwhile.
    async fn rule_snapshot(&self, id: &ssh_manager_core::filter::RuleId) -> Option<RuleSnapshot> {
        let ctx = EvalContext {
            server_id: self.server_id,
            tags: self.tags.clone(),
        };
        let rule = self
            .safety
            .filter_engine
            .rules_by_ids(&ctx, std::slice::from_ref(id))
            .await
            .into_iter()
            .next()?;
        Some(RuleSnapshot {
            id: rule.id.0,
            pattern: rule.pattern,
            action: rule.action,
            scope: rule.scope,
            origin: rule.origin,
        })
    }
}

/// An action event without its action id would be unusable for the
/// observer; this is a bug of the caller, so it is loud in tests.
fn missing_action_id() {
    debug_assert!(false, "activity event for an action without an action id");
    tracing::warn!("activity event dropped: the action id is missing");
}

/// Starts the once-a-minute user-activity tick of `session` in `tasks` (its
/// session scope). The tick runs off every input path: input only sets a
/// flag, this task reads it. It emits one `UserActivity` per interval in
/// which there was input or a command started by the user was running
/// (a command started through MCP does not count). Without an observer no
/// task is started. Holds only a weak reference to the session.
pub fn spawn_user_activity_tick(
    session: &std::sync::Arc<Session>,
    tasks: &crate::tasks::TaskScope,
) {
    if !session.activity_active() {
        return;
    }
    let session = std::sync::Arc::downgrade(session);
    tasks.spawn("activity-tick", async move {
        let mut ticker = tokio::time::interval_at(
            tokio::time::Instant::now() + USER_ACTIVITY_INTERVAL,
            USER_ACTIVITY_INTERVAL,
        );
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let Some(session) = session.upgrade() else {
                return;
            };
            let Some(reporting) = session.activity_reporting() else {
                return;
            };
            let input = reporting
                .input_seen
                .swap(false, std::sync::atomic::Ordering::Relaxed);
            if input || session.transport.user_command_running() {
                session
                    .push_activity(ActivitySource::User, ActivityEventKind::UserActivity)
                    .await;
            }
        }
    });
}
