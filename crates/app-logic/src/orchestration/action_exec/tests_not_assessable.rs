//! Issue #109: Ein Kommando, das für die Secret-Pfad- bzw.
//! `sftp-server`-Prüfung zu lang oder zu tief verschachtelt ist, wird
//! weiterhin bestätigt, aber mit eigenem Code
//! (`FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM`) — der Dialog darf
//! nicht behaupten, es lese eine Datei mit Zugangsdaten oder starte
//! `sftp-server`.
//!
//! Die End-zu-End-Fälle laufen mit Allow-für-alles: Ohne die beiden Glieder
//! landete das Kommando auf `AutoExec`.

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::{EvalContext, FilterEngine};
use ssh_manager_core::risk::CommandCheckFinding;

use super::super::test_support::*;
use super::*;

const NOT_ASSESSABLE: &str = "FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM";
const SECRET: &str = "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM";
const SFTP: &str = "FILTER_SFTP_SERVER_REQUIRES_CONFIRM";
const INJECTION: &str = "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM";

/// Quoting wie `shell_words::quote` für Text mit Sonderzeichen.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// `inner` in `levels` verschachtelten `watch '…'`-Aufrufen. Bewusst nicht
/// `bash -c`: das setzt die Filter-Engine schon selbst auf `Confirm`
/// (`FILTER_PARSE_AMBIGUOUS`), der Fall erreichte die Glieder nie mit
/// `AutoExec`.
fn nested_watch(inner: &str, levels: usize) -> String {
    let mut command = inner.to_string();
    for _ in 0..levels {
        command = format!("watch {}", quote(&command));
    }
    command
}

/// Harmloser Code, tiefer verschachtelt als die Secret-Pfad-Prüfung geht.
fn too_nested_harmless() -> String {
    nested_watch("ls -la", 4)
}

/// Eine bloße Erwähnung von `sftp-server`, tiefer verschachtelt als die
/// `sftp-server`-Prüfung geht.
fn too_nested_sftp_server_mention() -> String {
    nested_watch("ls /usr/lib/openssh/sftp-server -la", 4)
}

fn allow_everything_session(transport: MockSshTransport) -> Session {
    test_session_allowing_everything(vec![AiEvent::Done], transport)
}

async fn decide(session: &Session, command: &str) -> (Decision, serde_json::Value) {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            session,
            AiAction::SuggestCommand {
                command: command.to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden")
}

/// Vorbedingungen: die Beispiele landen ohne die beiden Glieder auf
/// `AutoExec`, sind nicht rot, und die Prüfungen melden „nicht prüfbar".
#[tokio::test]
async fn test_examples_reach_the_links_with_autoexec() {
    let engine = FilterEngine::new(AllowEverythingPolicyStore);
    let ctx = EvalContext {
        server_id: ssh_manager_core::shared::ServerId::new(),
        tags: Vec::new(),
    };
    for command in [too_nested_harmless(), too_nested_sftp_server_mention()] {
        assert!(
            matches!(engine.evaluate(&command, &ctx).await, Decision::AutoExec),
            "{command} muss mit Allow-Regel auf AutoExec landen, sonst prüft der Test nichts"
        );
        let assessment = RuleBasedRiskClassifier.classify(&command);
        assert_ne!(assessment.server_risk, RiskLevel::Red, "{command}");
        assert_ne!(assessment.data_risk, RiskLevel::Red, "{command}");
    }
    assert!(matches!(
        ssh_manager_core::risk::secret_path_read_reason(&too_nested_harmless()),
        Some(CommandCheckFinding::NotAssessable(_))
    ));
    assert_eq!(
        ssh_manager_core::risk::sftp_server_invocation_reason(&too_nested_harmless()),
        None
    );
    assert!(matches!(
        ssh_manager_core::risk::sftp_server_invocation_reason(&too_nested_sftp_server_mention()),
        Some(CommandCheckFinding::NotAssessable(_))
    ));
}

/// Zu tief verschachtelte Shell-Code-Strings: `Confirm` mit dem neuen Code,
/// nicht mit dem Secret-Pfad-Code.
#[tokio::test]
async fn test_too_nested_command_gets_not_assessable_code() {
    let command = too_nested_harmless();
    let session =
        allow_everything_session(MockSshTransport::default().with_response(&command, output("")));
    let (decision, payload) = decide(&session, &command).await;
    assert!(
        matches!(&decision, Decision::Confirm { code, .. } if code == NOT_ASSESSABLE),
        "{payload}"
    );
}

/// Eine zu tief verschachtelte Erwähnung von `sftp-server`: ebenfalls der
/// neue Code, weder `FILTER_SFTP_SERVER_REQUIRES_CONFIRM` noch der
/// Secret-Pfad-Code. (Das `sftp-server`-Glied selbst sieht „nicht prüfbar"
/// nur, wenn das Secret-Glied davor nichts meldet; seine Abbildung prüft
/// `test_check_finding_confirm_always_confirms` direkt.)
#[tokio::test]
async fn test_too_nested_sftp_server_mention_gets_not_assessable_code() {
    let command = too_nested_sftp_server_mention();
    let session =
        allow_everything_session(MockSshTransport::default().with_response(&command, output("")));
    let (decision, payload) = decide(&session, &command).await;
    assert!(
        matches!(&decision, Decision::Confirm { code, .. } if code == NOT_ASSESSABLE),
        "{payload}"
    );
}

/// Echte Treffer behalten ihren Code.
#[tokio::test]
async fn test_real_matches_keep_their_codes() {
    for (command, expected) in [("cat /etc/shadow", SECRET), ("sftp-server", SFTP)] {
        let session = allow_everything_session(
            MockSshTransport::default().with_response(command, output("")),
        );
        let (decision, payload) = decide(&session, command).await;
        assert!(
            matches!(&decision, Decision::Confirm { code, .. } if code == expected),
            "{command}: {payload}"
        );
    }
}

/// Bei Injection-Verdacht bleibt dessen Code vorrangig — auch für „nicht
/// prüfbar".
#[tokio::test]
async fn test_injection_code_takes_precedence_over_not_assessable() {
    for command in [too_nested_harmless(), too_nested_sftp_server_mention()] {
        let session = allow_everything_session(
            MockSshTransport::default().with_response(&command, output("")),
        );
        session
            .injection_suspected
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (decision, payload) = decide(&session, &command).await;
        assert!(
            matches!(&decision, Decision::Confirm { code, .. } if code == INJECTION),
            "{command}: {payload}"
        );
    }
}

// --- reine Funktion `check_finding_confirm` ------------------------------

/// Kein „nicht prüfbarer" Fall wird je `AutoExec` oder etwas anderes als
/// `Confirm` — in jeder Kombination aus Variante, Glied und Verdachts-Flag.
#[test]
fn test_check_finding_confirm_always_confirms() {
    for finding in [
        CommandCheckFinding::Match("Treffer"),
        CommandCheckFinding::NotAssessable("zu lang"),
    ] {
        for match_code in [SECRET, SFTP] {
            for injection in [false, true] {
                let decision = check_finding_confirm(finding, match_code, injection);
                let Decision::Confirm { code, reason } = decision else {
                    panic!("{finding:?}/{match_code}/{injection}: {decision:?}");
                };
                let expected = match (injection, finding) {
                    (true, _) => INJECTION,
                    (false, CommandCheckFinding::NotAssessable(_)) => NOT_ASSESSABLE,
                    (false, CommandCheckFinding::Match(_)) => match_code,
                };
                assert_eq!(code, expected, "{finding:?}/{match_code}/{injection}");
                assert!(reason.contains(finding.reason()), "{reason}");
            }
        }
    }
}

/// `Deny` bleibt `Deny`: das Glied greift nur bei `AutoExec`. Ein
/// zu tief verschachteltes Kommando, das eine Deny-Regel trifft, wird nicht
/// in ein `Confirm` umgewandelt.
#[tokio::test]
async fn test_deny_stays_deny_for_not_assessable_command() {
    struct DenyEverything;
    #[async_trait::async_trait]
    impl ssh_manager_core::filter::PolicyStore for DenyEverything {
        async fn rules_for(
            &self,
            _scope: &ssh_manager_core::filter::EffectiveScope,
        ) -> Vec<ssh_manager_core::filter::Rule> {
            vec![ssh_manager_core::filter::Rule {
                id: RuleId("deny-all".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 10,
                origin: RuleOrigin::User,
            }]
        }
    }
    let command = too_nested_harmless();
    let mut session = test_session(
        vec![AiEvent::Done],
        MockSshTransport::default().with_response(&command, output("")),
    );
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyEverything)));
    let (decision, payload) = decide(&session, &command).await;
    assert!(matches!(decision, Decision::Deny { .. }), "{payload}");
}

/// Über der Längenschranke (das Mehrbyte-Beispiel aus `tests_red_risk.rs`):
/// Die Filter-Engine verlangt dafür schon selbst `Confirm`
/// (`FILTER_COMMAND_TOO_LONG`), deshalb hier direkt über Prüfung und
/// Abbildung — das Ergebnis ist der neue Code, nicht der Secret-Pfad-Code.
#[test]
fn test_overlong_command_maps_to_not_assessable_code() {
    // „€" sind 3 Bytes, 1 Zeichen.
    let command = format!("iptables -F -m comment --comment \"{}\"", "€".repeat(1400));
    assert!(ssh_manager_core::filter::exceeds_command_length_limit(
        &command,
        ssh_manager_core::filter::DEFAULT_MAX_COMMAND_LENGTH
    ));
    let finding = ssh_manager_core::risk::secret_path_read_reason(&command)
        .expect("Überlänge muss eskalieren");
    let Decision::Confirm { code, .. } = check_finding_confirm(finding, SECRET, false) else {
        panic!("muss Confirm sein");
    };
    assert_eq!(code, NOT_ASSESSABLE);
    let finding = ssh_manager_core::risk::sftp_server_invocation_reason(&command)
        .expect("Überlänge muss eskalieren");
    let Decision::Confirm { code, .. } = check_finding_confirm(finding, SFTP, false) else {
        panic!("muss Confirm sein");
    };
    assert_eq!(code, NOT_ASSESSABLE);
}
