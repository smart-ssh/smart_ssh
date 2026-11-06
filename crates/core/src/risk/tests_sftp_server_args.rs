//! Issue #1192: `sftp-server` with the logging arguments stays red.

use super::classifier::RuleBasedRiskClassifier;
use super::types::{RiskClassifier, RiskLevel};

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
