//! Issue #863: the steps of `handle_action_proposed` asserted directly.

use std::sync::atomic::Ordering::SeqCst;

use ssh_manager_core::ai::AiEvent;

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::propose::{
    apply_escalations, prepare_action, prepare_decision, second_opinion_escalation,
    settle_confirmation, ConfirmationSettlement, ProposeCtx, SecondOpinionOutcome,
};
use super::*;

fn cmd(c: &str) -> AiAction {
    AiAction::SuggestCommand {
        command: c.to_string(),
    }
}

fn outcome(data_risk: Option<RiskLevel>, unavailable: bool) -> SecondOpinionOutcome {
    SecondOpinionOutcome {
        data_risk,
        unavailable,
    }
}

fn ctx_for<'a>(
    session: &'a Session,
    emitter: &'a TestEmitter,
    registry: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
    origin: ActionOrigin,
) -> ProposeCtx<'a> {
    ProposeCtx {
        session,
        session_id: Uuid::new_v4(),
        emitter,
        profile_store: empty_store(),
        action_confirmations: registry,
        origin,
        earlier_rejection: test_fresh_rejection_flag(),
        action_id: Uuid::new_v4(),
        persist: true,
    }
}

async fn escalate(session: &Session, origin: ActionOrigin, command: &str) -> Decision {
    let emitter = TestEmitter::default();
    let registry = ConfirmationRegistry::default();
    let ctx = ProposeCtx {
        earlier_rejection: Box::leak(Box::default()),
        ..ctx_for(session, &emitter, &registry, origin)
    };
    let action = cmd(command);
    let (evaluation, tags_unavailable) = evaluate_action(session, &action, empty_store()).await;
    apply_escalations(&ctx, &action, &evaluation, tags_unavailable)
        .await
        .decision
}

#[test]
fn second_opinion_red_escalates_auto_exec_with_red_code() {
    let (_, code) =
        second_opinion_escalation(true, &outcome(Some(RiskLevel::Red), false), true, false)
            .unwrap();
    assert_eq!(code, "FILTER_RED_RISK_REQUIRES_CONFIRM");
}

#[test]
fn second_opinion_red_without_setting_does_not_escalate() {
    assert!(
        second_opinion_escalation(false, &outcome(Some(RiskLevel::Red), false), true, false)
            .is_none()
    );
}

#[test]
fn second_opinion_unavailable_escalates_even_without_red_setting() {
    let (_, code) = second_opinion_escalation(false, &outcome(None, true), true, false).unwrap();
    assert_eq!(code, "FILTER_SECOND_OPINION_UNAVAILABLE_REQUIRES_CONFIRM");
}

#[test]
fn second_opinion_escalation_only_touches_auto_exec_and_yields_to_stop() {
    let o = outcome(Some(RiskLevel::Red), true);
    assert!(second_opinion_escalation(true, &o, false, false).is_none());
    assert!(second_opinion_escalation(true, &o, true, true).is_none());
    // Red wins over "unavailable" when both apply.
    let (_, code) = second_opinion_escalation(true, &o, true, false).unwrap();
    assert_eq!(code, "FILTER_RED_RISK_REQUIRES_CONFIRM");
}

#[test]
fn second_opinion_no_finding_keeps_auto_exec() {
    assert!(
        second_opinion_escalation(true, &outcome(Some(RiskLevel::None), false), true, false)
            .is_none()
    );
}

#[tokio::test]
async fn settle_confirmation_maps_every_wait_outcome_to_a_denial_or_the_decision() {
    let id = Uuid::new_v4();
    assert!(matches!(
        settle_confirmation(id, Ok(Ok(ActionUserDecision::Approve))),
        ConfirmationSettlement::Decided(ActionUserDecision::Approve, RejectionReason::User)
    ));
    assert!(matches!(
        settle_confirmation(id, Ok(Ok(ActionUserDecision::DenyReconnect))),
        ConfirmationSettlement::Decided(ActionUserDecision::Deny, RejectionReason::Reconnect)
    ));
    let (tx, rx) = tokio::sync::oneshot::channel::<ActionUserDecision>();
    drop(tx);
    assert!(matches!(
        settle_confirmation(id, Ok(rx.await)),
        ConfirmationSettlement::SenderDropped
    ));
    let elapsed = tokio::time::timeout(
        std::time::Duration::ZERO,
        std::future::pending::<Result<ActionUserDecision, tokio::sync::oneshot::error::RecvError>>(
        ),
    )
    .await;
    assert!(matches!(
        settle_confirmation(id, elapsed),
        ConfirmationSettlement::Decided(ActionUserDecision::Deny, RejectionReason::Timeout)
    ));
}

#[tokio::test]
async fn escalation_step_mcp_origin_turns_auto_exec_into_confirm() {
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    let internal = escalate(&session, ActionOrigin::Internal, "ls -la").await;
    assert!(matches!(internal, Decision::AutoExec));
    let mcp = escalate(
        &session,
        ActionOrigin::Mcp {
            client_name: Some("c".to_string()),
        },
        "ls -la",
    )
    .await;
    assert!(matches!(mcp, Decision::Confirm { .. }));
}

#[tokio::test]
async fn escalation_step_red_risk_always_confirms() {
    let mut session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    session.set_red_risk_always_confirm_for_tests(true);
    let d = escalate(&session, ActionOrigin::Internal, "iptables -F").await;
    assert!(matches!(d, Decision::Confirm { .. }));
}

#[tokio::test]
async fn escalation_step_injection_suspected_confirms_and_consumes_flag() {
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    session.injection_suspected.store(true, SeqCst);
    let d = escalate(&session, ActionOrigin::Internal, "ls -la").await;
    assert!(matches!(d, Decision::Confirm { .. }));
    assert!(!session.injection_suspected.load(SeqCst));
}

#[tokio::test]
async fn prepare_action_normalises_remote_path_once() {
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    let emitter = TestEmitter::default();
    let registry = ConfirmationRegistry::default();
    let ctx = ctx_for(&session, &emitter, &registry, ActionOrigin::Internal);
    let mut action = AiAction::ReadRemoteFile {
        path: "/a/b/../c".to_string(),
    };
    prepare_action(&ctx, &mut action).await;
    assert!(matches!(&action, AiAction::ReadRemoteFile { path } if path == "/a/c"));
}

#[tokio::test]
async fn prepare_decision_registers_confirm_and_refuses_second_mcp_confirm() {
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    let emitter = TestEmitter::default();
    let registry = ConfirmationRegistry::default();
    let ctx = ctx_for(
        &session,
        &emitter,
        &registry,
        ActionOrigin::Mcp {
            client_name: Some("c".to_string()),
        },
    );
    let confirm = || Decision::Confirm {
        reason: "r".to_string(),
        code: "c".to_string(),
    };
    assert!(matches!(
        prepare_decision(&ctx, Decision::AutoExec),
        Some(PreparedDecision::AutoExec)
    ));
    let first = prepare_decision(&ctx, confirm());
    assert!(matches!(first, Some(PreparedDecision::Confirm { .. })));
    // A second exclusive MCP confirmation is refused while the first is held.
    assert!(prepare_decision(&ctx, confirm()).is_none());
}
