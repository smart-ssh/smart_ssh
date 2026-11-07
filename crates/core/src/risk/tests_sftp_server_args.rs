//! Issue #1192: `sftp-server` with the logging arguments stays red.

use super::classifier::RuleBasedRiskClassifier;
use super::sftp_server_invocation_reason;
use super::types::{RiskClassifier, RiskLevel};
use super::CommandCheckFinding;

#[test]
fn test_sftp_server_with_logging_arguments_is_red() {
    for command in [
        "sudo -n /usr/lib/openssh/sftp-server -l INFO -f AUTHPRIV",
        "sudo -n -u www-data /usr/lib/openssh/sftp-server -l INFO -f AUTHPRIV",
    ] {
        let assessment = RuleBasedRiskClassifier.classify(command);
        assert_eq!(assessment.server_risk, RiskLevel::Red, "not red: {command}");
    }
}

/// Issue #1208: sudo/doas options with (quoted) arguments stay part of the
/// prefix.
const BEHIND_OPTIONS: &[&str] = &[
    "sudo -S -k -p '[sudo] password:' /usr/lib/openssh/sftp-server",
    "sudo -p \"pw: \" sftp-server",
    "sudo --prompt='x y' sftp-server",
    "sudo --prompt 'x' sftp-server",
    "sudo -u root sftp-server",
    "sudo -S -k -p '[sudo] password:' -u root /usr/lib/openssh/sftp-server -l INFO -f AUTHPRIV",
    "doas -u root /usr/libexec/sftp-server",
    "sudo -uroot sftp-server",
    "sudo -p'x' sftp-server",
    "sudo -p'a b' sftp-server",
    "sudo --user=root -- sftp-server",
    "sh -c \"sudo -p 'x' sftp-server\"",
];

#[test]
fn test_sftp_server_behind_sudo_options_is_red_and_detected() {
    for command in BEHIND_OPTIONS {
        let assessment = RuleBasedRiskClassifier.classify(command);
        assert_eq!(assessment.server_risk, RiskLevel::Red, "not red: {command}");
        assert!(
            matches!(
                sftp_server_invocation_reason(command),
                Some(CommandCheckFinding::Match(_))
            ),
            "not detected: {command}"
        );
    }
}

#[test]
fn test_sftp_server_word_check_alone_handles_sudo_options() {
    // The word check must not depend on the red pattern.
    for command in BEHIND_OPTIONS {
        assert_eq!(
            super::classifier::sftp_server_word_check_for_tests(command),
            "found",
            "word check missed: {command}"
        );
    }
}

#[test]
fn test_sftp_server_word_check_skips_unquoted_option_argument() {
    assert_eq!(
        super::classifier::sftp_server_word_check_for_tests("sudo -D /tmp -H sftp-server"),
        "found"
    );
}

#[test]
fn test_sftp_server_mentions_stay_harmless() {
    for command in [
        "ls /usr/lib/openssh/sftp-server",
        "grep sftp-server /etc/ssh/sshd_config",
        "sudo cat /usr/lib/openssh/sftp-server",
        "sudo -p 'sftp-server' ls",
        "sudo --prompt 'sftp-server' ls",
        "sudo -u root ls",
    ] {
        assert_eq!(
            sftp_server_invocation_reason(command),
            None,
            "falsely detected: {command}"
        );
    }
}
