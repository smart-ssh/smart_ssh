//! Issue #826: the decision path and the rule-suggestion verdict
//! (`state_independent_escalation_code`) share `escalate`, so for neutral
//! transient inputs they report the same code.

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::profiles::PostIngestPolicy;

use super::super::test_support::*;
use super::*;

/// Every combination of the session booleans over representative commands:
/// the code of the real decision path equals the verdict's code.
#[tokio::test]
async fn test_main_path_and_verdict_report_the_same_code() {
    use std::sync::atomic::Ordering::SeqCst;
    let commands = [
        "ls -la",
        "cat ~/.ssh/id_rsa",
        "sftp-server",
        "iptables -F",
        "sudo systemctl restart nginx",
    ];
    for command in commands {
        for bits in 0u8..16 {
            let (suspected, unavailable, ingested, red) =
                (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
            let mut session =
                test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
            session.set_post_ingest_policy_for_tests(PostIngestPolicy::Strict);
            session.set_red_risk_always_confirm_for_tests(red);
            session
                .safety
                .untrusted_content_ingested
                .store(ingested, SeqCst);
            session.safety.injection_suspected.store(suspected, SeqCst);
            session
                .safety
                .injection_check_unavailable
                .store(unavailable, SeqCst);

            let verdict = state_independent_escalation_code(&session, command, None);
            // The verdict must not consume anything.
            assert_eq!(session.safety.injection_suspected.load(SeqCst), suspected);
            assert_eq!(
                session.safety.injection_check_unavailable.load(SeqCst),
                unavailable
            );

            let action = AiAction::SuggestCommand {
                command: command.to_string(),
            };
            let (decision, _) = proposed_decision_code(&session, action).await;
            let main = match &decision {
                Decision::Confirm { code, .. } | Decision::Deny { code, .. } => Some(code.clone()),
                Decision::AutoExec => None,
            };
            assert_eq!(
                main.as_deref(),
                verdict,
                "{command} suspected={suspected} unavailable={unavailable} \
                 ingested={ingested} red={red}"
            );
        }
    }
}
