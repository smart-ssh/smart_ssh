//! Property-based tests for the filter engine (issue #253, Spec 0002).
//!
//! Complements the fixed cases in `adversarial_tests.rs` (#13): the same
//! safety guarantees, checked against generated input. The policies are the
//! ones from that suite (`Allow "*"` + `Deny "rm *"`, `Allow "*"` alone).
//!
//! Generators stay within shell syntax the parser handles. Input the engine
//! rejects as ambiguous simply counts as "not `AutoExec`", which every
//! property accepts.

use std::sync::OnceLock;

use proptest::prelude::*;

use super::adversarial_tests::{allow_all, ctx, deny_rm, engine, StaticPolicyStore};
use super::*;

/// One runtime for all cases; `evaluate` is `async`, proptest bodies are sync.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("tokio runtime")
    })
    .block_on(future)
}

fn eval(rules: Vec<Rule>, cmd: &str) -> Decision {
    block_on(engine(rules).evaluate(cmd, &ctx()))
}

fn is_autoexec(decision: &Decision) -> bool {
    matches!(decision, Decision::AutoExec)
}

// --- Generators --------------------------------------------------------------

/// A plain shell word: letters, digits and a few path/flag characters.
fn word() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_./=-]{1,10}"
}

/// Zero or more plain words, space-separated, with a leading space when
/// non-empty.
fn args() -> impl Strategy<Value = String> {
    proptest::collection::vec(word(), 0..4).prop_map(|words| {
        words
            .into_iter()
            .map(|w| format!(" {w}"))
            .collect::<String>()
    })
}

/// A harmless command used as prefix / neighbour segment.
fn harmless() -> impl Strategy<Value = String> {
    (
        proptest::sample::select(vec!["ls", "echo", "cd", "pwd", "true", "cat", "date", "id"]),
        args(),
    )
        .prop_map(|(cmd, a)| format!("{cmd}{a}"))
}

/// Arbitrary text, biased towards characters that matter to the parser, up to
/// twice the default length limit in characters.
fn arbitrary_command() -> impl Strategy<Value = String> {
    let shellish: Vec<char> =
        "abcxyz019 \t\n;|&$()`'\"\\<>*?{}[]~#!=-./:\u{0}\u{7}\u{1b}\u{202e}\u{200b}é日🙂"
            .chars()
            .collect();
    prop_oneof![
        any::<String>(),
        proptest::collection::vec(any::<char>(), 0..=2 * DEFAULT_MAX_COMMAND_LENGTH)
            .prop_map(|c| c.into_iter().collect()),
        proptest::collection::vec(proptest::sample::select(shellish.clone()), 0..200)
            .prop_map(|c| c.into_iter().collect()),
        proptest::collection::vec(
            proptest::sample::select(shellish),
            DEFAULT_MAX_COMMAND_LENGTH..=2 * DEFAULT_MAX_COMMAND_LENGTH,
        )
        .prop_map(|c| c.into_iter().collect()),
    ]
}

fn whitespace() -> impl Strategy<Value = String> {
    "[ \t]{0,4}"
}

/// `rm` followed by at least one word, so `Deny "rm *"` can match.
fn rm_invocation() -> impl Strategy<Value = String> {
    (word(), args()).prop_map(|(first, rest)| format!("rm {first}{rest}"))
}

const JOINERS: [&str; 5] = [";", "&&", "||", "|", "\n"];

/// Commands that contain `rm <args>` in some segment, reached through one of
/// the joiners or a substitution.
fn chained_rm() -> impl Strategy<Value = String> {
    let joined = (
        harmless(),
        proptest::sample::select(JOINERS.to_vec()),
        rm_invocation(),
        proptest::bool::ANY,
        whitespace(),
    )
        .prop_map(|(p, j, rm, rm_first, ws)| {
            if rm_first {
                format!("{rm}{ws}{j}{ws}{p}")
            } else {
                format!("{p}{ws}{j}{ws}{rm}")
            }
        });
    let dollar =
        (harmless(), rm_invocation(), proptest::bool::ANY).prop_map(|(p, rm, inner_first)| {
            if inner_first {
                format!("echo $({rm}; {p})")
            } else {
                format!("{p} $({rm})")
            }
        });
    let backtick = (harmless(), rm_invocation()).prop_map(|(p, rm)| format!("{p} `{rm}`"));
    let three_segments = (
        harmless(),
        proptest::sample::select(JOINERS.to_vec()),
        harmless(),
        proptest::sample::select(JOINERS.to_vec()),
        rm_invocation(),
    )
        .prop_map(|(a, j1, b, j2, rm)| format!("{a} {j1} {b} {j2} {rm}"));
    prop_oneof![joined, dollar, backtick, three_segments]
}

/// One template per hard-blacklist entry, in the order of
/// `hard_blacklist_patterns()`. `{}` is replaced by generated arguments.
const BLACKLIST_TEMPLATES: [&str; 12] = [
    "rm -rf{}",
    "rm -fr{}",
    "rm --recursive --force{}",
    "rm --force --recursive{}",
    "dd if=/dev/zero{} of=/dev/sda",
    "mkfs.ext4{}",
    ":(){ :|:& };:",
    ":()  {  :  |  :  &  }  ;  :",
    "tee{} /etc/shadow",
    "shutdown{}",
    "systemctl reboot{}",
    "init 0{}",
];

fn blacklisted_command() -> impl Strategy<Value = String> {
    (
        proptest::sample::select(BLACKLIST_TEMPLATES.to_vec()),
        args(),
        whitespace(),
        whitespace(),
    )
        .prop_map(|(template, a, lead, trail)| {
            format!("{lead}{}{trail}", template.replace("{}", &a))
        })
}

// --- Sanity: generators are not vacuous --------------------------------------

#[test]
fn blacklist_templates_cover_every_hard_blacklist_pattern() {
    // A new hard-blacklist entry needs a template here.
    assert_eq!(
        hard_blacklist_patterns().len(),
        BLACKLIST_TEMPLATES.len(),
        "hard blacklist changed: add a template to BLACKLIST_TEMPLATES"
    );
    for template in BLACKLIST_TEMPLATES {
        let cmd = template.replace("{}", "");
        assert!(
            blacklist::matching_entry(&cmd).is_some(),
            "template {template:?} is not caught by the hard blacklist"
        );
    }
}

#[test]
fn chained_rm_templates_are_denied_without_the_allow_rule_interfering() {
    // Fixed instances of every shape the generator produces: the deny rule
    // must really be reachable, otherwise property 3 would pass vacuously.
    for cmd in [
        "ls; rm a",
        "ls && rm a",
        "ls || rm a",
        "ls | rm a",
        "ls\nrm a",
        "echo $(rm a; ls)",
        "ls `rm a`",
    ] {
        assert!(
            matches!(
                eval(vec![allow_all(), deny_rm()], cmd),
                Decision::Deny { .. }
            ),
            "{cmd:?} should be denied"
        );
    }
}

// --- Properties ----------------------------------------------------------------

proptest! {
    /// 1. Never panics, always yields a `Decision`.
    #[test]
    fn evaluate_never_panics(cmd in arbitrary_command()) {
        for rules in [vec![allow_all(), deny_rm()], vec![allow_all()], vec![]] {
            let decision = eval(rules, &cmd);
            // Reaching this line means a `Decision` came back; also check it
            // is one of the three variants (the enum is exhaustive).
            match decision {
                Decision::AutoExec | Decision::Confirm { .. } | Decision::Deny { .. } => {}
            }
        }
    }

    /// 2. Same command and context give the same decision.
    #[test]
    fn evaluate_is_deterministic(cmd in arbitrary_command()) {
        let engine = engine(vec![allow_all(), deny_rm()]);
        let context = ctx();
        let first = block_on(engine.evaluate(&cmd, &context));
        let second = block_on(engine.evaluate(&cmd, &context));
        prop_assert_eq!(first, second);
    }

    /// 3. Chaining or substitution cannot hide `rm` from the deny rule.
    #[test]
    fn chaining_cannot_hide_a_denied_command(cmd in chained_rm()) {
        let decision = eval(vec![allow_all(), deny_rm()], &cmd);
        prop_assert!(!is_autoexec(&decision), "{cmd:?} was AutoExec");
    }

    /// 4. The hard blacklist holds under `Allow "*"`.
    #[test]
    fn hard_blacklist_holds_under_allow_all(cmd in blacklisted_command()) {
        for rules in [vec![allow_all()], vec![allow_all(), deny_rm()]] {
            let decision = eval(rules, &cmd);
            prop_assert!(!is_autoexec(&decision), "{cmd:?} was AutoExec");
        }
    }

    /// 5. Over-long commands are never `AutoExec`, for a configured limit and
    /// for the default one.
    #[test]
    fn over_long_commands_are_never_autoexec(
        padding in 1usize..300,
        limit_seed in any::<usize>(),
        extra in 1usize..2000,
    ) {
        let cmd = format!("echo {}", "a".repeat(padding));
        let limit = limit_seed % cmd.len(); // 0..len, so cmd.len() > limit
        let configured = FilterEngine::with_max_command_length(
            StaticPolicyStore { rules: vec![allow_all()] },
            limit,
        );
        let decision = block_on(configured.evaluate(&cmd, &ctx()));
        prop_assert!(!is_autoexec(&decision), "len {} > limit {limit}", cmd.len());

        let long = format!("echo {}", "a".repeat(DEFAULT_MAX_COMMAND_LENGTH + extra));
        let decision = eval(vec![allow_all()], &long);
        prop_assert!(!is_autoexec(&decision), "default limit not enforced");
    }
}
