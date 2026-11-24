//! Issue #1551: renamed private SSH key files stay red.

use super::classifier::RuleBasedRiskClassifier;
use super::types::RiskLevel;

fn classify(command: &str) -> super::types::RiskAssessment {
    RuleBasedRiskClassifier.classify(command)
}

#[test]
fn backup_id_rsa_is_rated_like_id_rsa() {
    for f in [
        "backup_id_rsa",
        "old_id_ed25519",
        "id_rsa.bak",
        "x_id_ecdsa_sk",
        "backup_id_rsa.bak",
    ] {
        let a = classify(&format!("cat ~/.ssh/{f}"));
        assert_eq!(a.data_risk, RiskLevel::Red, "{f}");
    }
    for f in ["backup_id_rsa.pub", "id_rsa.pub", "rapid_search.txt"] {
        let a = classify(&format!("cat ~/.ssh/{f}"));
        assert_ne!(a.data_risk, RiskLevel::Red, "{f}");
    }
}
