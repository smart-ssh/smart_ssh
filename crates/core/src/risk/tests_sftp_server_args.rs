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
            super::sftp_word_check::sftp_server_word_check_for_tests(command),
            "found",
            "word check missed: {command}"
        );
    }
}

#[test]
fn test_sftp_server_word_check_skips_unquoted_option_argument() {
    assert_eq!(
        super::sftp_word_check::sftp_server_word_check_for_tests("sudo -D /tmp -H sftp-server"),
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

/// Issue #1230: wrapper arguments before `sftp-server` in a quoted command.
const BEHIND_WRAPPERS_QUOTED: &[&str] = &[
    "timeout 5m /usr/lib/openssh/sftp-server -l 'INFO'",
    "timeout -s KILL 1h /usr/lib/openssh/sftp-server -f \"AUTHPRIV\"",
    "timeout -k 10s 2m sftp-server -l 'INFO'",
    "nice -n 10 timeout 5m sftp-server -l 'INFO'",
    "env -u HOME timeout 1h \"/usr/lib/openssh/sftp-server\"",
    "stdbuf -oL sftp-server -l 'INFO'",
    "ionice -c 2 sftp-server -l 'INFO'",
    "runuser -u root -- sftp-server -l 'INFO'",
];

#[test]
fn test_sftp_server_behind_quoted_wrapper_arguments_is_red_and_detected() {
    for command in BEHIND_WRAPPERS_QUOTED {
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
fn test_sftp_server_wrapper_mentions_stay_harmless() {
    for command in [
        "timeout 5m cat '/usr/lib/openssh/sftp-server'",
        "sudo -p 'sftp-server' ls",
        "timeout -s KILL 1h ls 'sftp-server'",
    ] {
        assert!(
            sftp_server_invocation_reason(command).is_none(),
            "false positive: {command}"
        );
    }
}

#[test]
fn test_sftp_server_behind_su_user_command_is_detected() {
    assert!(matches!(
        sftp_server_invocation_reason("su root -c 'sftp-server -l INFO'"),
        Some(CommandCheckFinding::Match(_))
    ));
}

/// Issue #1232: quoted segments behind wrappers are rated red too.
const QUOTED_RED: &[&str] = &[
    "timeout 5m env FOO='a b' sftp-server -l 'x'",
    "su root -c \"timeout 5m sftp-server -l x\"",
    "env A='1 2' B=\"3 4\" nice -n 5 timeout 1m /usr/lib/openssh/sftp-server -l 'x'",
    "su root -c \"env X='a b' sh -c 'timeout 5m sftp-server -l y'\"",
    "echo hi && timeout 5m env FOO='a b' sftp-server",
];

#[test]
fn test_quoted_sftp_server_behind_wrappers_is_red_and_detected() {
    for command in QUOTED_RED {
        assert_eq!(
            RuleBasedRiskClassifier.classify(command).server_risk,
            RiskLevel::Red,
            "not red: {command}"
        );
        assert!(
            super::sftp_word_check::quoted_sftp_server_invocation(command),
            "pattern walk missed: {command}"
        );
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
fn test_quoted_sftp_server_mentions_stay_harmless() {
    for command in [
        "sudo -p 'sftp-server' ls",
        "timeout 5m cat '/usr/lib/openssh/sftp-server'",
        "env FOO='sftp-server x' ls",
        "echo 'sftp-server -l x'",
        "timeout 5m env FOO='a b' ls 'sftp-server'",
    ] {
        assert!(
            !super::sftp_word_check::quoted_sftp_server_invocation(command),
            "walk false positive: {command}"
        );
        // The broad word check (b) deliberately flags quoted code strings.
        if !command.starts_with("echo") {
            assert_eq!(
                sftp_server_invocation_reason(command),
                None,
                "falsely detected: {command}"
            );
        }
    }
}
