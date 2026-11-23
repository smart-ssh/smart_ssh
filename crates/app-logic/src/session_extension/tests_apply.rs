//! Issue #1541: the redactor swap and the "redaction incomplete" flag are one
//! step with respect to other rebuilds.

use super::*;
use crate::orchestration::test_support::test_session;
use crate::orchestration::test_support::MockSshTransport;
use std::sync::atomic::Ordering;

fn flag(session: &crate::session::Session) -> bool {
    session.safety.redaction_incomplete.load(Ordering::SeqCst)
}

fn apply(session: &crate::session::Session, incomplete: bool, between: impl FnOnce()) {
    let secrets: Vec<SecretString> = vec![];
    let (redactor, _) = redactor_with_secrets_checked(None, &secrets);
    apply_rebuilt_redactor(
        session,
        redactor,
        &secrets,
        !incomplete,
        incomplete,
        between,
    );
}

/// Another rebuild cannot run between a swap and its flag store: the lock is
/// held there, so the interleaving A swap, B swap + store, A store is
/// impossible. Without the lock `try_lock` would succeed.
#[test]
fn test_apply_lock_is_held_between_swap_and_store() {
    let session = test_session(vec![], MockSshTransport::default());
    apply(&session, true, || {
        assert!(REDACTOR_APPLY_LOCK.try_lock().is_err());
    });
    assert!(flag(&session));
}

#[test]
fn test_complete_after_incomplete_lowers_and_incomplete_after_complete_raises() {
    let session = test_session(vec![], MockSshTransport::default());
    apply(&session, true, || {});
    assert!(flag(&session));
    apply(&session, false, || {});
    assert!(!flag(&session));
    apply(&session, true, || {});
    assert!(flag(&session));
}

fn reconnect_redactor() -> Box<dyn OutputRedactor> {
    redactor_with_secrets_checked(None, &[]).0
}

/// Issue #1543: the reconnect swap holds the lock between swap and flag.
#[test]
fn test_reconnect_apply_lock_is_held_between_swap_and_store() {
    let session = test_session(vec![], MockSshTransport::default());
    apply_reconnect_redactor(&session, reconnect_redactor(), true, || {
        assert!(REDACTOR_APPLY_LOCK.try_lock().is_err());
    });
    assert!(flag(&session));
}

/// Issue #1543: incomplete reconnect lookup, complete rebuild in between,
/// reconnect swap last: the flag ends raised.
#[test]
fn test_incomplete_reconnect_swap_after_complete_rebuild_leaves_flag_raised() {
    let session = test_session(vec![], MockSshTransport::default());
    // The reconnect lookup (incomplete) happened first; a complete rebuild
    // then lowers the flag; the reconnect swap comes last.
    apply(&session, false, || {});
    assert!(!flag(&session));
    apply_reconnect_redactor(&session, reconnect_redactor(), true, || {});
    assert!(flag(&session));
}

/// A complete reconnect never lowers the flag a rebuild left raised.
#[test]
fn test_complete_reconnect_keeps_flag_as_rebuild_left_it() {
    let session = test_session(vec![], MockSshTransport::default());
    apply(&session, true, || {});
    apply_reconnect_redactor(&session, reconnect_redactor(), false, || {});
    assert!(flag(&session));
    apply(&session, false, || {});
    apply_reconnect_redactor(&session, reconnect_redactor(), false, || {});
    assert!(!flag(&session));
}
