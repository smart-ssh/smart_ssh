//! Issue #688: the post-engine escalation chain logs one default-level line
//! `filter decision escalated` (kinds, code, action kind, session id,
//! `command_len`) — never the command, a path or a reason.

use ssh_manager_core::ai::AiEvent;

use crate::events::TestEmitter;
use crate::test_support::log_capture;

use super::super::test_support::*;
use super::*;

const MARKER: &str = "zq9marker";

fn nested_watch(levels: usize) -> String {
    let mut command = "ls -la".to_string();
    for _ in 0..levels {
        command = format!("watch '{}'", command.replace('\'', "'\\''"));
    }
    command
}

fn lines_with(lines: &[String], message: &str) -> Vec<serde_json::Value> {
    lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["fields"]["message"] == message)
        .collect()
}

fn cmd(command: &str) -> AiAction {
    AiAction::SuggestCommand {
        command: command.to_string(),
    }
}

/// Runs the action to its dialog (denying a confirmation) with the given
/// rejection flag and origin.
async fn run(session: &Session, action: AiAction, origin: ActionOrigin, rejected: bool) {
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let flag = std::sync::atomic::AtomicBool::new(rejected);
    let fut = handle_action_proposed(
        session,
        Uuid::new_v4(),
        action,
        &emitter,
        &profile_store,
        &confirmations,
        origin,
        &flag,
    );
    let responder = deny_first_proposed_action(&emitter, &confirmations);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            async {
                fut.await;
            },
            responder
        )
    })
    .await
    .expect("dialog must end");
}

fn allow_all() -> Session {
    test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default())
}

fn assert_no_content(lines: &[String]) {
    assert!(
        !lines
            .iter()
            .any(|l| l.contains(MARKER) || l.contains("id_rsa") || l.contains("erfordert")),
        "no command, path or reason at info: {lines:?}"
    );
}

/// One case per escalation code of the chain.
#[tokio::test]
async fn test_each_escalation_code_is_logged() {
    use std::sync::atomic::Ordering::SeqCst;
    type Setup = fn(&mut Session);
    let secret = format!("cat ~/.ssh/id_rsa_{MARKER}");
    let internal = || ActionOrigin::Internal;
    let cases: Vec<(&str, String, Setup, bool, bool, &str)> = vec![
        (
            "secret",
            secret.clone(),
            |_| {},
            false,
            false,
            "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM",
        ),
        (
            "not assessable",
            nested_watch(4),
            |_| {},
            false,
            false,
            "FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM",
        ),
        (
            "sftp",
            format!("sftp-server {MARKER}"),
            |_| {},
            false,
            false,
            "FILTER_SFTP_SERVER_REQUIRES_CONFIRM",
        ),
        (
            "red",
            format!("iptables -F {MARKER}"),
            |_| {},
            false,
            false,
            "FILTER_RED_RISK_REQUIRES_CONFIRM",
        ),
        (
            "post ingest",
            format!("ls {MARKER}"),
            |s| {
                s.set_post_ingest_policy_for_tests(
                    ssh_manager_core::profiles::PostIngestPolicy::Strict,
                );
                s.untrusted_content_ingested.store(true, SeqCst);
            },
            false,
            false,
            "FILTER_POST_INGEST_REQUIRES_CONFIRM",
        ),
        (
            "suspected",
            format!("ls {MARKER}"),
            |s| s.injection_suspected.store(true, SeqCst),
            false,
            false,
            "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM",
        ),
        (
            "unavailable",
            format!("ls {MARKER}"),
            |s| s.injection_check_unavailable.store(true, SeqCst),
            false,
            false,
            "FILTER_INJECTION_CHECK_UNAVAILABLE_REQUIRES_CONFIRM",
        ),
        (
            "mcp",
            format!("ls {MARKER}"),
            |_| {},
            true,
            false,
            "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM",
        ),
        (
            "sudo",
            format!("sudo ls {MARKER}"),
            |s| {
                s.set_sudo_password_for_tests(Some(secrecy::SecretString::from(
                    "hunter2".to_string(),
                )))
            },
            false,
            false,
            "FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM",
        ),
        (
            "rejected",
            format!("ls {MARKER}"),
            |_| {},
            false,
            true,
            "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
        ),
    ];
    for (name, command, setup, mcp, rejected, code) in cases {
        let mut session = allow_all();
        setup(&mut session);
        log_capture::start_recording();
        let origin = if mcp {
            ActionOrigin::Mcp { client_name: None }
        } else {
            internal()
        };
        run(&session, cmd(&command), origin, rejected).await;
        let lines = log_capture::recorded_lines_at_info_or_above();
        let escalated = lines_with(&lines, "filter decision escalated");
        assert_eq!(escalated.len(), 1, "{name}: {lines:?}");
        let f = &escalated[0]["fields"];
        assert_eq!(f["code"], code, "{name}");
        assert_eq!(f["engine_decision"], "AutoExec", "{name}");
        assert_eq!(f["final_decision"], "Confirm", "{name}");
        assert_eq!(f["action_kind"], "command", "{name}");
        assert_eq!(f["command_len"], command.len() as u64, "{name}");
        assert!(f["session_id"].is_string(), "{name}");
        assert_eq!(
            lines_with(&lines, "filter engine decision").len(),
            1,
            "{name}"
        );
        assert_no_content(&lines);
    }
}

/// The first step that set the decision names the code (secret path wins
/// over red risk).
#[tokio::test]
async fn test_first_step_in_the_chain_names_the_code() {
    let session = allow_all();
    log_capture::start_recording();
    run(
        &session,
        cmd("cat /etc/shadow"),
        ActionOrigin::Internal,
        false,
    )
    .await;
    let lines = log_capture::recorded_lines_at_info_or_above();
    let escalated = lines_with(&lines, "filter decision escalated");
    assert_eq!(escalated.len(), 1, "{lines:?}");
    assert_eq!(
        escalated[0]["fields"]["code"],
        "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM"
    );
}

/// File reads are labelled `file_read`.
#[tokio::test]
async fn test_file_read_escalation_has_action_kind_file_read() {
    let session = allow_all();
    log_capture::start_recording();
    run(
        &session,
        AiAction::ReadRemoteFile {
            path: "/home/u/.ssh/id_rsa".to_string(),
        },
        ActionOrigin::Internal,
        false,
    )
    .await;
    let lines = log_capture::recorded_lines_at_info_or_above();
    let escalated = lines_with(&lines, "filter decision escalated");
    assert_eq!(escalated.len(), 1, "{lines:?}");
    assert_eq!(escalated[0]["fields"]["action_kind"], "file_read");
    assert_no_content(&lines);
}

/// Unchanged `AutoExec`, and an engine `Confirm`/`Deny`: no escalation line.
#[tokio::test]
async fn test_no_escalation_line_when_decision_is_unchanged() {
    let session = allow_all();
    log_capture::start_recording();
    let (decision, _) = proposed_decision_code(&session, cmd("ls -la")).await;
    assert!(matches!(decision, Decision::AutoExec));
    let lines = log_capture::recorded_lines_at_info_or_above();
    assert!(lines_with(&lines, "filter decision escalated").is_empty());

    // Engine already says Confirm (no rules at all) and Deny (hard blacklist).
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    for command in ["ls -la", "rm -rf /"] {
        log_capture::start_recording();
        let _ = proposed_decision_code(&session, cmd(command)).await;
        let lines = log_capture::recorded_lines_at_info_or_above();
        assert!(
            lines_with(&lines, "filter decision escalated").is_empty(),
            "{command}: {lines:?}"
        );
    }
}
