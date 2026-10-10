//! Property-based tests for the rule-based risk classifier (issue #255).
//!
//! Guarantees checked against generated input, at the `proptest` default
//! case count:
//!
//! 1. `classify` never panics, also for deeply nested `$(` and for strings
//!    up to twice the length limit.
//! 2. `classify` is deterministic.
//! 3. Chaining never lowers risk: `A <sep> B` is on each axis at least as
//!    risky as `A` and `B` alone.
//! 4. A red command stays red when wrapped in `sudo`, `$(…)` or `bash -c`.

use proptest::prelude::*;

use super::classifier::RuleBasedRiskClassifier;
use super::types::{RiskAssessment, RiskClassifier, RiskLevel};
use crate::filter::DEFAULT_MAX_COMMAND_LENGTH;

fn classify(command: &str) -> RiskAssessment {
    RuleBasedRiskClassifier.classify(command)
}

/// Characters that matter to the segmenter and to the patterns.
const SHELLISH: &str = "abcrmfsdkx019 \t\n;|&$()`'\"\\<>*?{}[]~#!=-./:_\u{0}\u{1b}\u{202e}é日🙂";

fn shellish_string(max_len: usize) -> impl Strategy<Value = String> {
    let alphabet: Vec<char> = SHELLISH.chars().collect();
    proptest::collection::vec(proptest::sample::select(alphabet), 0..=max_len)
        .prop_map(|chars| chars.into_iter().collect())
}

/// Arbitrary text, biased towards shell syntax, up to twice the length
/// limit.
fn arbitrary_command() -> impl Strategy<Value = String> {
    let alphabet: Vec<char> = SHELLISH.chars().collect();
    prop_oneof![
        any::<String>(),
        shellish_string(200),
        proptest::collection::vec(any::<char>(), 0..=2 * DEFAULT_MAX_COMMAND_LENGTH)
            .prop_map(|chars| chars.into_iter().collect::<String>()),
        proptest::collection::vec(
            proptest::sample::select(alphabet),
            DEFAULT_MAX_COMMAND_LENGTH..=2 * DEFAULT_MAX_COMMAND_LENGTH,
        )
        .prop_map(|chars| chars.into_iter().collect::<String>()),
        // Deeply nested substitutions, also past the length limit.
        (1usize..2 * DEFAULT_MAX_COMMAND_LENGTH / 2).prop_map(|depth| {
            let mut s = "$(".repeat(depth);
            s.push_str("rm -rf /");
            s.push_str(&")".repeat(depth));
            s
        }),
        (1usize..2 * DEFAULT_MAX_COMMAND_LENGTH).prop_map(|depth| "$(".repeat(depth)),
    ]
}

/// A single argument made of plain path/word characters.
fn arg() -> impl Strategy<Value = String> {
    "[a-z0-9_./-]{1,12}"
}

fn args() -> impl Strategy<Value = String> {
    proptest::collection::vec(arg(), 0..4).prop_map(|words| {
        words
            .into_iter()
            .map(|word| format!(" {word}"))
            .collect::<String>()
    })
}

/// Commands built from the red patterns (server and data axis) with
/// arbitrary arguments, together with the axis that has to be red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Server,
    Data,
}

fn red_command() -> impl Strategy<Value = (Axis, String)> {
    let server = prop_oneof![
        args().prop_map(|a| format!("rm -rf{a}")),
        args().prop_map(|a| format!("rm -fr{a}")),
        (arg(), arg()).prop_map(|(a, b)| format!("dd if={a} of=/dev/{b}")),
        arg().prop_map(|a| format!("mkfs.{a} /dev/sdb1")),
        args().prop_map(|a| format!("shutdown{a}")),
        args().prop_map(|a| format!("reboot{a}")),
        args().prop_map(|a| format!("poweroff{a}")),
        args().prop_map(|a| format!("halt{a}")),
        args().prop_map(|a| format!("iptables -F{a}")),
        arg().prop_map(|a| format!("chmod -R 777 /{a}")),
        args().prop_map(|a| format!("sftp-server{a}")),
        Just(":(){ :|:& };:".to_string()),
    ]
    .prop_map(|c| (Axis::Server, c));
    let data = prop_oneof![
        arg().prop_map(|a| format!("cat /{a}/id_rsa")),
        arg().prop_map(|a| format!("cat /{a}/.env")),
        arg().prop_map(|a| format!("less /{a}/credentials")),
        Just("cat /etc/shadow".to_string()),
        args().prop_map(|a| format!("printenv{a}")),
        args().prop_map(|a| format!("mysqldump{a}")),
        args().prop_map(|a| format!("pg_dump{a}")),
    ]
    .prop_map(|c| (Axis::Data, c));
    prop_oneof![server, data]
}

/// Commands covering red, yellow and harmless patterns with arbitrary
/// arguments — the typical neighbours in a chain.
fn plain_command() -> impl Strategy<Value = String> {
    prop_oneof![
        red_command().prop_map(|(_, c)| c),
        args().prop_map(|a| format!("rm{a}")),
        args().prop_map(|a| format!("kill{a}")),
        args().prop_map(|a| format!("systemctl restart{a}")),
        args().prop_map(|a| format!("ls{a}")),
        args().prop_map(|a| format!("echo{a}")),
        (arg(), arg()).prop_map(|(a, b)| format!("cat {a} {b}")),
    ]
}

const JOINERS: [&str; 5] = [";", "&&", "||", "|", "\n"];

fn joiner() -> impl Strategy<Value = &'static str> {
    proptest::sample::select(JOINERS.to_vec())
}

fn assert_chain_not_lower(a: &str, b: &str, sep: &str) -> Result<(), TestCaseError> {
    let joined = format!("{a}{sep}{b}");
    prop_assume!(joined.len() <= DEFAULT_MAX_COMMAND_LENGTH);
    let (ra, rb, rj) = (classify(a), classify(b), classify(&joined));
    prop_assert!(
        rj.server_risk >= ra.server_risk.max(rb.server_risk),
        "server risk fell: {:?} < max({:?}, {:?}) for {joined:?}",
        rj.server_risk,
        ra.server_risk,
        rb.server_risk
    );
    prop_assert!(
        rj.data_risk >= ra.data_risk.max(rb.data_risk),
        "data risk fell: {:?} < max({:?}, {:?}) for {joined:?}",
        rj.data_risk,
        ra.data_risk,
        rb.data_risk
    );
    Ok(())
}

fn assert_red_on_axis(
    assessment: &RiskAssessment,
    axis: Axis,
    command: &str,
) -> Result<(), TestCaseError> {
    let level = match axis {
        Axis::Server => assessment.server_risk,
        Axis::Data => assessment.data_risk,
    };
    prop_assert_eq!(level, RiskLevel::Red, "not red ({:?}): {:?}", axis, command);
    Ok(())
}

proptest! {
    #[test]
    fn classify_never_panics(command in arbitrary_command()) {
        let _ = classify(&command);
    }

    #[test]
    fn classify_is_deterministic(command in arbitrary_command()) {
        prop_assert_eq!(classify(&command), classify(&command));
    }

    #[test]
    fn chaining_never_lowers_risk_for_known_commands(
        a in plain_command(), b in plain_command(), sep in joiner()
    ) {
        assert_chain_not_lower(&a, &b, sep)?;
    }

    #[test]
    fn chaining_never_lowers_risk_for_arbitrary_text(
        a in shellish_string(200), b in shellish_string(200), sep in joiner()
    ) {
        assert_chain_not_lower(&a, &b, sep)?;
    }

    #[test]
    fn red_command_is_red_on_its_own((axis, command) in red_command()) {
        assert_red_on_axis(&classify(&command), axis, &command)?;
    }

    #[test]
    fn red_stays_red_when_wrapped((axis, command) in red_command()) {
        for wrapped in [
            format!("sudo {command}"),
            format!("$({command})"),
            format!("bash -c '{command}'"),
        ] {
            assert_red_on_axis(&classify(&wrapped), axis, &wrapped)?;
        }
    }
}
