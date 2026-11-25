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
        "backupid_rsa",
        "key2id_ed25519",
    ] {
        let a = classify(&format!("cat ~/.ssh/{f}"));
        assert_eq!(a.data_risk, RiskLevel::Red, "{f}");
    }
    for f in ["backup_id_rsa.pub", "id_rsa.pub", "rapid_search.txt"] {
        let a = classify(&format!("cat ~/.ssh/{f}"));
        assert_ne!(a.data_risk, RiskLevel::Red, "{f}");
    }
}

/// Issue #1564: glob reads that can match a private key name are red.
#[test]
fn glob_reads_of_private_key_names_are_red() {
    for c in [
        "cat ~/.ssh/backup_id_*",
        "cat ~/.ssh/*_id_*",
        "cat ~/.ssh/*backup_id_*",
        "cat ~/.ssh/b*_id_*",
        "cat ~/.ssh/id_*",
        "cat ~/.ssh/id_r*",
        "cat ~/.ssh/old_id_ed2551?",
        "tail -n 5 ~/.ssh/*_id_rsa",
        "cat \"$HOME/.ssh/backup_id_*\"",
        "cat ~/.ssh/{id,backup_id}_rsa*",
        "cat ~/.ssh/{old,backup}_id_*",
        "cat ~/.ssh/backup{,2}_id_*",
        "cat ~/.ssh/[bo]*_id_*",
        "cat ~/.ssh/back[u]p_id_*",
        "cat ~/.ssh/{a,b}*_id_*",
        "cat ~/.ssh/[!bx]_id_*",
        "cat ~/.ssh/[!b]?_id_*",
        "cat ~/.ssh/{a,{b,c}}_id_*",
        "cat $(echo ~/.ssh/backup_id_*)",
    ] {
        assert_eq!(classify(c).data_risk, RiskLevel::Red, "{c}");
    }
}

#[test]
fn glob_reads_of_other_files_stay_non_red() {
    for c in [
        "cat ~/.ssh/*.pub",
        "cat ~/.ssh/known_hosts*",
        "cat ~/.ssh/known_*",
        "cat ~/notes/rapid_*",
        "cat ~/notes/bid_*",
        "cat ~/notes/rabid_*",
        "cat ~/notes/web_bid_*",
        "cat ~/docs/*.txt",
        "cat ~/.ssh/*",
        "cat ~/.ssh/i*",
        "cat ~/.ssh/*id*",
    ] {
        assert_ne!(classify(c).data_risk, RiskLevel::Red, "{c}");
    }
}

#[test]
fn secret_path_check_escalates_prefixed_key_globs() {
    // Consistency guard only: these globs are already caught by the `id_` path
    // hint (`glob_has_secret_hint`), so no input makes the `x_id_*` entries in
    // `SECRET_FILE_CANDIDATES` the deciding factor; they keep the candidate
    // list in step with the glob rating.
    for c in ["cat /srv/keys/*_id_*", "cat /srv/keys/x_id_*"] {
        assert!(super::secret_path_read_reason(c).is_some(), "{c}");
    }
}

/// Issue #1593: glued redirects and backticks do not hide a key glob.
#[test]
fn glob_reads_with_glued_redirect_or_backticks_are_red() {
    for c in [
        "cat ~/.ssh/backup_id_*>/tmp/x",
        "cat ~/.ssh/id_*>>/tmp/x",
        "cat ~/.ssh/id_*</dev/null",
        "echo `cat ~/.ssh/backup_id_*`",
    ] {
        assert_eq!(classify(c).data_risk, RiskLevel::Red, "{c}");
    }
}

#[test]
fn glob_reads_with_glued_redirect_stay_non_red_for_near_misses() {
    for c in [
        "cat ~/.ssh/*.pub>/tmp/x",
        "cat ~/.ssh/known_*>/tmp/x",
        "cat ~/.ssh/rapid_*>/tmp/x",
        "cat ~/.ssh/bid_*>/tmp/x",
    ] {
        assert_ne!(classify(c).data_risk, RiskLevel::Red, "{c}");
    }
}
