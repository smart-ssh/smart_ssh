//! Connect check of the session extension (Spec 0038, section 2a).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;

struct Check {
    refuse: Option<&'static str>,
    calls: AtomicUsize,
}

impl Check {
    fn new(refuse: Option<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            refuse,
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl SessionExtension for Check {
    async fn check_connect(&self, _scope: &ServerScope) -> Result<(), ConnectRefusal> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.refuse {
            Some(m) => Err(ConnectRefusal::new(m)),
            None => Ok(()),
        }
    }
}

fn lookup(scope: bool, incomplete: bool) -> ScopeLookup {
    ScopeLookup {
        scope: scope.then(|| ServerScope {
            server_id: ServerId::new(),
            group_chain: Vec::new(),
            tags: Vec::new(),
        }),
        incomplete,
    }
}

#[tokio::test]
async fn test_first_refusal_wins_and_later_extensions_are_not_asked() {
    let (a, b, c) = (
        Check::new(None),
        Check::new(Some("first")),
        Check::new(Some("second")),
    );
    let composite = CompositeSessionExtension::new(vec![a.clone(), b.clone(), c.clone()]);
    let err = check_connect_allowed(Some(&composite), &lookup(true, false))
        .await
        .unwrap_err();
    assert_eq!(err.message, "first");
    assert_eq!(a.calls.load(Ordering::SeqCst), 1);
    assert_eq!(b.calls.load(Ordering::SeqCst), 1);
    assert_eq!(c.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_default_extension_and_no_extension_allow() {
    let composite = CompositeSessionExtension::new(vec![Arc::new(RecordingExtension::default())]);
    assert!(
        check_connect_allowed(Some(&composite), &lookup(true, false))
            .await
            .is_ok()
    );
    assert!(check_connect_allowed(None, &lookup(false, true))
        .await
        .is_ok());
}

#[tokio::test]
async fn test_unresolved_or_incomplete_scope_refuses_without_asking() {
    let ext = Check::new(None);
    for l in [
        lookup(false, false),
        lookup(true, true),
        lookup(false, true),
    ] {
        assert!(check_connect_allowed(Some(ext.as_ref()), &l).await.is_err());
    }
    assert_eq!(ext.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn test_refusal_becomes_command_error_with_stable_code() {
    let err: ssh_manager_core::ipc_error::CommandError = ConnectRefusal::new("nope").into();
    assert_eq!(err.code, Some("CONNECT_REFUSED"));
    assert_eq!(err.message, "nope");
}
