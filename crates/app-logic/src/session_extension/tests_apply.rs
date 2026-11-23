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
