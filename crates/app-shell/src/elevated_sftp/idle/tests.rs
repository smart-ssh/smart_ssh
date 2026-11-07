//! Issue #1193: the idle timer (paused virtual time).

use std::sync::Arc;
use std::time::Duration;

use ssh_manager_core::ssh::mock::MockSftpSession;

use super::*;
use crate::elevated_sftp::{ElevatedSftp, ElevatedSftpRegistry};
use crate::file_ops::OpKind;

const MIN: Duration = Duration::from_secs(60);

struct Setup {
    registry: ElevatedSftpRegistry,
    session_id: SessionId,
    slot: ElevatedSftpSlot,
    notified: tokio::sync::mpsc::UnboundedReceiver<(SessionId, u32)>,
}

fn setup(minutes: u32) -> Setup {
    let registry = ElevatedSftpRegistry::default();
    registry.idle().set_minutes(minutes);
    registry.idle().set_spawner(Arc::new(|_, fut| {
        tokio::spawn(fut);
    }));
    let (tx, notified) = tokio::sync::mpsc::unbounded_channel();
    registry
        .idle()
        .set_notifier(Arc::new(move |session_id, minutes| {
            let _ = tx.send((session_id, minutes));
        }));
    let session_id = SessionId::new_v4();
    registry.insert_for_tests(
        session_id,
        ElevatedSftp::new("root".to_string(), Box::new(MockSftpSession::new())),
    );
    let slot = registry
        .slot(session_id, &crate::test_support::elevation::access())
        .expect("inserted");
    registry.start_idle_timer_for_tests(session_id);
    Setup {
        registry,
        session_id,
        slot,
        notified,
    }
}

/// Lets the timer task run after the virtual clock moved.
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    settle().await;
}

#[test]
fn test_stored_value_parsing_is_fail_safe() {
    use serde_json::json;
    assert_eq!(from_stored(None), 15);
    for m in [5, 15, 30, 60] {
        assert_eq!(from_stored(Some(json!(m))), m);
    }
    assert_eq!(from_stored(Some(json!(0))), NEVER);
    for bad in [
        json!(7),
        json!("never"),
        json!(-1),
        json!(true),
        json!(null),
    ] {
        assert_eq!(from_stored(Some(bad)), 15);
    }
    assert!(is_allowed(30) && !is_allowed(7));
}

#[tokio::test(start_paused = true)]
async fn test_default_timeout_revokes_idle_channel_after_15_minutes() {
    let mut s = setup(DEFAULT_IDLE_MINUTES);
    advance(14 * MIN).await;
    assert!(!s.slot.is_revoked(), "still within 15 minutes");
    advance(MIN + Duration::from_secs(1)).await;
    assert!(s.slot.is_revoked());
    assert_eq!(s.notified.recv().await, Some((s.session_id, 15)));
    let access = crate::test_support::elevation::access();
    assert!(s.registry.slot(s.session_id, &access).is_none());
    assert!(
        s.slot.lock_owned(&access).await.is_none(),
        "channel value dropped like after a manual switch-off"
    );
}

#[tokio::test(start_paused = true)]
async fn test_running_operation_postpones_switch_off_until_it_ended() {
    let mut s = setup(15);
    let guard = s
        .registry
        .ops()
        .register(
            s.session_id,
            uuid::Uuid::new_v4(),
            OpKind::Upload,
            "/x",
            true,
        )
        .with_activity(s.slot.activity_handle());
    advance(120 * MIN).await;
    assert!(!s.slot.is_revoked(), "never interrupted while running");
    drop(guard);
    settle().await;
    advance(14 * MIN).await;
    assert!(!s.slot.is_revoked(), "timer restarted when the op ended");
    advance(MIN + Duration::from_secs(1)).await;
    assert!(s.slot.is_revoked());
    assert_eq!(s.notified.recv().await, Some((s.session_id, 15)));
}

#[tokio::test(start_paused = true)]
async fn test_activity_restarts_the_timer() {
    let s = setup(15);
    advance(10 * MIN).await;
    s.slot.touch();
    advance(10 * MIN).await;
    assert!(!s.slot.is_revoked());
    advance(6 * MIN).await;
    assert!(s.slot.is_revoked());
}

#[tokio::test(start_paused = true)]
async fn test_never_disables_the_timer() {
    let s = setup(NEVER);
    advance(48 * 60 * MIN).await;
    assert!(!s.slot.is_revoked());
}

#[tokio::test(start_paused = true)]
async fn test_changing_the_setting_applies_to_a_running_timer() {
    let s = setup(NEVER);
    advance(20 * MIN).await;
    s.registry.idle().set_minutes(5);
    advance(Duration::from_secs(1)).await;
    assert!(
        s.slot.is_revoked(),
        "5 minutes since last activity are over"
    );
}

#[tokio::test(start_paused = true)]
async fn test_manual_revoke_ends_the_timer_without_notification() {
    let mut s = setup(15);
    s.registry.revoke_session(s.session_id);
    settle().await;
    advance(60 * MIN).await;
    assert!(s.slot.is_revoked());
    assert!(
        s.notified.try_recv().is_err(),
        "manual switch-off is silent"
    );
}
