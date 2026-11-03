//! Issue #866: the named steps behind `handle_user_decision` and
//! `execute_suggested_command`.

use async_trait::async_trait;
use secrecy::SecretString;
use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::{EffectiveScope, FilterEngine, PolicyRules, PolicyStore, Rule};
use uuid::Uuid;

use crate::confirmation::{resolve_action_confirmation, ACTION_CONFIRMATION_UNKNOWN};
use crate::events::TestEmitter;

use super::super::test_support::*;
use super::execute::{CommandReport, PreparedCommand};
use super::*;

struct DenyRm;
#[async_trait]
impl PolicyStore for DenyRm {
    async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
        PolicyRules::complete(vec![Rule {
            id: ssh_manager_core::filter::RuleId("deny-rm".to_string()),
            pattern: ssh_manager_core::filter::Pattern::Glob("rm *".to_string()),
            action: ssh_manager_core::filter::RuleAction::Deny,
            scope: ssh_manager_core::filter::Scope::Global,
            priority: 0,
            origin: ssh_manager_core::filter::RuleOrigin::User,
        }])
    }
}

async fn edit_then_approve(edited: &str) -> (Vec<String>, bool) {
    let transport = MockSshTransport::default().with_response("echo ok", output("ok"));
    let executed = transport.executed_handle();
    let mut session = test_session(vec![AiEvent::Done], transport);
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyRm)));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let rejection = std::sync::atomic::AtomicBool::new(false);
    handle_user_decision(
        &session,
        Uuid::new_v4(),
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "ls".to_string(),
        },
        ActionUserDecision::EditThenApprove {
            command: edited.to_string(),
        },
        RejectionReason::User,
        &emitter,
        &profile_store,
        ActionOrigin::Internal,
        None,
        None,
        String::new(),
        String::new(),
        false,
        &rejection,
    )
    .await;
    let ran = executed.lock().unwrap().clone();
    (ran, rejection.load(std::sync::atomic::Ordering::SeqCst))
}

/// An edited command is evaluated again: a hard `Deny` stops it before the
/// transport is touched.
#[tokio::test]
async fn test_edited_command_is_reevaluated_and_denied_before_it_runs() {
    let (ran, rejected) = edit_then_approve("rm -rf /tmp/x").await;
    assert!(ran.is_empty(), "{ran:?}");
    assert!(rejected);
}

/// An edit the filter does not deny runs (the edit click is the confirmation).
#[tokio::test]
async fn test_edited_command_that_passes_reevaluation_runs() {
    let (ran, rejected) = edit_then_approve("echo ok").await;
    assert_eq!(ran, vec!["echo ok".to_string()]);
    assert!(!rejected);
}

/// A decision for an action id that is unknown or already answered is
/// rejected with the distinct code and resolves nothing.
#[test]
fn test_decision_for_stale_or_unknown_action_id_is_rejected() {
    let registry = ConfirmationRegistry::new();
    let id = Uuid::new_v4();
    let err = resolve_action_confirmation(&registry, &id, ActionUserDecision::Approve)
        .expect_err("unknown id");
    assert_eq!(err.code, Some(ACTION_CONFIRMATION_UNKNOWN));

    let _rx = registry.register(id);
    resolve_action_confirmation(&registry, &id, ActionUserDecision::Deny).unwrap();
    let err = resolve_action_confirmation(&registry, &id, ActionUserDecision::Approve)
        .expect_err("stale id");
    assert_eq!(err.code, Some(ACTION_CONFIRMATION_UNKNOWN));
}

/// Output is redacted before it reaches the history and the result event.
#[tokio::test]
async fn test_command_report_redacts_output_before_history_and_event() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let emitter = TestEmitter::default();
    let report = CommandReport {
        session: &session,
        session_id: Uuid::new_v4(),
        action_id: Uuid::new_v4(),
        emitter: &emitter,
        persist: true,
        ledger_source: LedgerSource::Ai,
    };
    report
        .success(
            "cat db.conf".to_string(),
            output("password=hunter2geheim"),
            false,
            false,
        )
        .await;
    let history = session.context.lock().await.history.clone();
    // `CommandOutput` bytes debug-print as numbers: decode them as well.
    let mut dump = format!("{history:?}");
    for m in &history {
        if let MessageContent::CommandResult { output, .. } = &m.content {
            dump.push_str(&String::from_utf8_lossy(&output.stdout));
        }
    }
    assert!(!dump.contains("hunter2geheim"), "{dump}");
    let events = format!("{:?}", emitter.events.lock().unwrap().clone());
    assert!(!events.contains("hunter2geheim"), "{events}");
    assert!(dump.contains("CommandResult"), "{dump}");
}

fn secret(s: &str) -> Option<std::sync::Arc<SecretString>> {
    Some(std::sync::Arc::new(SecretString::from(s.to_string())))
}

/// The stored sudo password goes to stdin only for a command that was
/// rewritten to read it from there.
#[test]
fn test_sudo_password_is_sent_on_stdin_only_with_the_stdin_flag() {
    let with_sudo = PreparedCommand::new("sudo ls".to_string(), None, secret("pw"));
    let (text, stdin) = with_sudo.wire_text_and_stdin();
    assert_eq!(text, "sudo -S ls");
    assert_eq!(stdin.as_deref(), Some(b"pw\n".as_slice()));

    // Command without elevation: password stored but never sent.
    let plain = PreparedCommand::new("ls".to_string(), None, secret("pw"));
    let (text, stdin) = plain.wire_text_and_stdin();
    assert_eq!(text, "ls");
    assert!(stdin.is_none());

    // The user already supplied `-S`: not rewritten, nothing on stdin.
    let own_flag = PreparedCommand::new("sudo -S ls".to_string(), None, secret("pw"));
    let (text, stdin) = own_flag.wire_text_and_stdin();
    assert_eq!(text, "sudo -S ls");
    assert!(stdin.is_none());

    // No stored password: nothing rewritten.
    let none = PreparedCommand::new("sudo ls".to_string(), None, None);
    let (text, stdin) = none.wire_text_and_stdin();
    assert_eq!(text, "sudo ls");
    assert!(stdin.is_none());
}
