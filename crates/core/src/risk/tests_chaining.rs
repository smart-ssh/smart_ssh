//! Chaining never lowers the risk rating: fixed cases the property tests
//! found (`property_tests::chaining_never_lowers_risk_for_arbitrary_text`).

use super::classifier::RuleBasedRiskClassifier;
use super::types::{RiskAssessment, RiskLevel};

fn classify(command: &str) -> RiskAssessment {
    RuleBasedRiskClassifier.classify(command)
}

// --- Issue #374: whitespace-padded segments in a chain -----------------

#[test]
fn chaining_whitespace_padded_segment_never_lowers_risk() {
    // Failing proptest case: `rm  ` alone matches `rm *`, but inside a
    // chain the segment is trimmed to `rm`.
    let alone = classify("rm  ");
    let chained = classify("a ;rm  ");
    assert!(alone.server_risk > RiskLevel::None);
    assert!(chained.server_risk >= alone.server_risk);
    assert!(chained.data_risk >= alone.data_risk);
}

#[test]
fn chaining_trailing_space_glob_commands_never_lower_risk() {
    for word in ["rm", "kill", "env", "printenv"] {
        for sep in [";", "&&", "||", "|", "\n"] {
            let part = format!("{word}  ");
            let alone = classify(&part);
            let chained = classify(&format!("a {sep}{part}"));
            assert!(
                chained.server_risk >= alone.server_risk && chained.data_risk >= alone.data_risk,
                "risk fell for {word:?} after {sep:?}"
            );
        }
    }
}

// --- Issue #433: unsplittable tail (here-doc operator) after a command ---

#[test]
fn chaining_command_with_unterminated_heredoc_never_lowers_risk() {
    let alone = classify("rm");
    assert!(alone.server_risk > RiskLevel::None);
    for tail in ["<<", "<", "<<-"] {
        for sep in [";", "&&", "||", "|", "\n"] {
            let joined = format!("rm{sep}{tail}");
            let chained = classify(&joined);
            assert!(
                chained.server_risk >= alone.server_risk && chained.data_risk >= alone.data_risk,
                "risk fell for {joined:?}: {chained:?}"
            );
        }
    }
}

#[test]
fn heredoc_fragments_never_lower_risk_for_red_commands() {
    let alone = classify("shutdown now");
    let chained = classify("shutdown now;<<");
    assert_eq!(alone.server_risk, RiskLevel::Red);
    assert_eq!(chained.server_risk, RiskLevel::Red);
}

// --- Text of a parameter expansion in a chain ----------------------------

#[test]
fn chaining_into_a_parameter_expansion_never_lowers_risk() {
    // Failing proptest case: `${a;rm;}` was read as one parameter
    // expansion, so the `rm` of `rm;}` was not classified.
    let alone = classify("rm;}");
    assert!(alone.server_risk > RiskLevel::None);
    for head in ["${a", "${a-", "${a:-", "${", "${#a"] {
        for sep in [";", "&&", "||", "|", "\n"] {
            let joined = format!("{head}{sep}rm;}}");
            let chained = classify(&joined);
            assert!(
                chained.server_risk >= alone.server_risk && chained.data_risk >= alone.data_risk,
                "risk fell for {joined:?}: {chained:?}"
            );
        }
    }
}
