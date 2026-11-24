//! Backslash escapes, `#` comments, line continuations and substitutions.
//! The segment scanner used to track quotes only, so an escaped quote, a
//! quote inside a comment or a backslash-newline made it see different
//! commands than the shell runs; a substitution was not read as a command
//! list of its own.

use super::*;
/// The four inputs that hid a command from the scanner. Each one runs
/// something other than `echo` in bash.
const HIDDEN_BEHIND_ESCAPE_OR_COMMENT: [&str; 4] = [
    // An escaped quote opened a fake quoted region over `; rm -rf / ;`.
    "echo \\' ; rm -rf / ; echo \\'",
    // A quote inside a comment opened a fake quoted region over the next line.
    "echo a #'\nrm -rf /\n#'",
    // An escaped double quote hid the redirection and the second command.
    "echo \\\" > /tmp/o; echo RAN; echo \"\\\"\"",
    // A line continuation split the program name.
    "r\\\nm -rf /",
];

#[tokio::test]
async fn test_adv_escaped_quote_hides_rm() {
    assert_denied_by_rm_rule(HIDDEN_BEHIND_ESCAPE_OR_COMMENT[0]).await;
}

#[tokio::test]
async fn test_adv_comment_quote_hides_rm() {
    assert_denied_by_rm_rule(HIDDEN_BEHIND_ESCAPE_OR_COMMENT[1]).await;
}

#[tokio::test]
async fn test_adv_escaped_double_quote_hides_commands() {
    assert_never_autoexec(HIDDEN_BEHIND_ESCAPE_OR_COMMENT[2]).await;
}

#[tokio::test]
async fn test_adv_line_continuation_hides_rm() {
    assert_denied_by_rm_rule(HIDDEN_BEHIND_ESCAPE_OR_COMMENT[3]).await;
}

/// Without a deny rule the hard blacklist sees the hidden `rm -rf /`.
#[tokio::test]
async fn test_adv_hidden_rm_reaches_hard_blacklist() {
    for cmd in [
        HIDDEN_BEHIND_ESCAPE_OR_COMMENT[0],
        HIDDEN_BEHIND_ESCAPE_OR_COMMENT[1],
        HIDDEN_BEHIND_ESCAPE_OR_COMMENT[3],
    ] {
        let trace = engine(vec![allow_all()])
            .evaluate_explained(cmd, &ctx())
            .await;
        assert!(
            trace_has_blacklist_entry(&trace),
            "{cmd:?}: hard blacklist not reached: {trace:?}"
        );
        assert!(!matches!(trace.decision, Decision::AutoExec), "{cmd:?}");
    }
}

/// An `Allow "echo *"` rule alone does not let the hidden commands run.
#[tokio::test]
async fn test_adv_allow_echo_only_hidden_commands() {
    let rules = || {
        vec![
            rule("allow-echo", "echo *", RuleAction::Allow, 100),
            deny_rm(),
        ]
    };
    for cmd in HIDDEN_BEHIND_ESCAPE_OR_COMMENT {
        let decision = engine(rules()).evaluate(cmd, &ctx()).await;
        assert!(
            !matches!(decision, Decision::AutoExec),
            "{cmd:?} under allow-echo: got AutoExec"
        );
    }
    for cmd in [
        HIDDEN_BEHIND_ESCAPE_OR_COMMENT[0],
        HIDDEN_BEHIND_ESCAPE_OR_COMMENT[1],
        HIDDEN_BEHIND_ESCAPE_OR_COMMENT[3],
    ] {
        let decision = engine(rules()).evaluate(cmd, &ctx()).await;
        assert!(
            matches!(decision, Decision::Deny { .. }),
            "{cmd:?} under allow-echo + deny-rm: expected Deny, got {decision:?}"
        );
    }
}

/// More shapes of the same kind: an escape or a comment never lets a hidden
/// command through, and the hidden `rm` is denied where it is a segment.
#[tokio::test]
async fn test_adv_escape_and_comment_variants() {
    for cmd in [
        "\\rm -rf /",
        "r\\m -rf /",
        "echo a\\\n; rm -rf /",
        "echo \"a\\\nb\"; rm -rf /",
        "echo a # x\nrm -rf /",
        "echo a;#'\nrm -rf /",
        "echo $'\\'' ; rm -rf / ; echo '",
        "echo ${x:- #}; rm -rf /",
        "echo $(#'\nrm -rf /)",
        "echo `echo \\` ; rm -rf / ; echo \\``",
    ] {
        assert_never_autoexec(cmd).await;
    }
    for cmd in [
        "\\rm -rf /",
        "r\\m -rf /",
        "echo a # x\nrm -rf /",
        "echo a;#'\nrm -rf /",
        "echo ${x:- #}; rm -rf /",
        "echo $(#'\nrm -rf /)",
        "echo a\\\n; rm -rf /",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// Documented behaviour: a line continuation, a comment or a backslash
/// inside backticks makes the command need confirmation (fail closed), with
/// the "not safely analysable" code, even under an `Allow "*"` rule. Quoted
/// `\` and `#`, and a `#` inside a word, stay ordinary text.
#[tokio::test]
async fn test_adv_continuation_and_comment_need_confirmation() {
    for cmd in [
        "echo a # comment",
        "# only a comment\nls",
        "ls \\\n -la",
        "echo \"a\\\nb\"",
    ] {
        let decision = engine(vec![allow_all(), deny_rm()])
            .evaluate(cmd, &ctx())
            .await;
        match decision {
            Decision::Confirm { code, .. } => {
                assert_eq!(code, "FILTER_PARSE_AMBIGUOUS", "{cmd:?}");
            }
            other => panic!("{cmd:?}: expected Confirm, got {other:?}"),
        }
    }
    // Inside backticks a backslash can nest a substitution: never AutoExec.
    assert_never_autoexec("echo `echo \\`date\\``").await;
    for cmd in [
        "echo 'a\\ b'",
        "echo \"a # b\"",
        "echo 'a # b'",
        "echo a#b",
        "echo $#",
        "echo \"a\\\"b\"",
        "ls -la",
        "ps aux | grep java",
        "grep -rn foo . && echo done",
    ] {
        assert_benign_autoexec(cmd).await;
    }
}

/// `inner` nested in `levels` `watch '…'` calls with the `'\''` idiom.
fn nested_single_quotes(inner: &str, levels: usize) -> String {
    let mut command = inner.to_string();
    for _ in 0..levels {
        command = format!("watch '{}'", command.replace('\'', "'\\''"));
    }
    command
}

/// Documented behaviour: a plain unquoted backslash escape is resolved
/// exactly and evaluated against the rules like quoted text, so it stays
/// `AutoExec` under a matching Allow rule, and a deny rule behind it wins.
#[tokio::test]
async fn test_adv_plain_escapes_follow_the_rules() {
    let cases = [
        ("echo *", "echo a\\ b".to_string()),
        ("echo *", "echo 'it'\\''s'".to_string()),
        ("find *", "find . -exec ls {} \\;".to_string()),
        ("find *", "find /tmp -name x -exec cat {} \\;".to_string()),
        ("ls *", "ls my\\ dir".to_string()),
        // bash prints "; rm -rf /" here, it runs no rm.
        ("echo *", "echo \\; rm -rf /".to_string()),
        ("watch *", nested_single_quotes("ls -la", 4)),
    ];
    for (allow, cmd) in cases {
        let decision = engine(vec![
            rule("allow", allow, RuleAction::Allow, 100),
            deny_rm(),
        ])
        .evaluate(&cmd, &ctx())
        .await;
        assert!(
            matches!(decision, Decision::AutoExec),
            "{cmd:?} under Allow {allow:?}: expected AutoExec, got {decision:?}"
        );
    }
    for cmd in [
        "\\rm -rf /",
        "r\\m x",
        "echo \\' ; rm x ; echo \\'",
        "echo \\\\; ls; rm x",
    ] {
        let decision = engine(vec![allow_all(), deny_rm()])
            .evaluate(cmd, &ctx())
            .await;
        assert!(
            matches!(decision, Decision::Deny { .. }),
            "{cmd:?}: expected Deny via deny-rm, got {decision:?}"
        );
    }
}

/// Bash 5.3 runs `${ cmd; }` and `${| cmd; }` as command substitutions, also
/// inside double quotes. They were read as parameter expansions.
#[tokio::test]
async fn test_adv_function_substitution_hides_rm() {
    for cmd in [
        "echo \"${ rm -rf /; }\"",
        "echo ${ rm -rf /; }",
        "echo ${| rm -rf /; }",
        "echo \"${\trm -rf /; }\"",
        "echo \"${\nrm -rf /\n}\"",
        "echo \"${ echo \")\"; rm -rf /; }\"",
        "echo \"${ { echo a; }; rm -rf /; }x\"",
        "echo \"${x:-${ rm -rf /; }}\"",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// After a `)` that closes a subshell or a case pattern a new word begins,
/// so a `#` there starts a comment in bash. Read as code, the quote in it
/// hid the next line.
#[tokio::test]
async fn test_adv_comment_after_closing_paren_hides_rm() {
    for cmd in [
        "(true)#'\nrm -rf /\n#'",
        "(true)#\"\nrm -rf /\n#\"",
        "case x in x)#'\nrm -rf /\n#'\nesac",
        "echo $( (true)#'\nrm -rf /\n#'\n)",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// Inside `"$(...)"` bash reads a new command list, with its own quotes and
/// comments. The double quote used to end at the first inner `"`, and the
/// substitution at the first `)`, quoted or not.
#[tokio::test]
async fn test_adv_quotes_and_comments_inside_quoted_substitution() {
    for cmd in [
        "echo \"$(echo \"a\"; rm -rf /)\"",
        "echo \"$(echo \")\"; rm -rf /)\"",
        "x=\"$(printf '%s' \"$(rm -rf /)\")\"",
        "echo \"$(#'\nrm -rf /\n)\"",
        "echo \"$(#\"\n(:)#\"\nrm -rf /\n#\"\"\n)\"",
        "echo \"$(echo \"it's\"; rm -rf /)\"",
        "echo \"$(echo ')'; rm -rf /)\"",
        "echo $(echo ')'; rm -rf /)",
        "echo $(true\nrm -rf /)",
        "echo \"$(case x in x) echo \")\";; esac; rm -rf /)\"",
        "echo \"${x:-$(echo \"}\"; rm -rf /)}\"",
        "echo \"`rm -rf /`\"",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// `case` patterns and reserved words inside a substitution: the lexer must
/// end the substitution where bash does, or the quote parity flips and a
/// top-level `rm` lands in what looks like a quoted string.
#[tokio::test]
async fn test_adv_case_and_reserved_words_inside_substitution() {
    for cmd in [
        "echo \"$(case x in esac)\"; rm -rf /; echo \";esac)\"",
        "echo \"$(case x in (esac) ;; x) echo \")\"; rm -rf /;; esac)\"",
        "echo \"$(case x in a|esac) ;; x) echo \")\"; rm -rf /;; esac)\"",
        "echo \"$(case x in\ncase) ;; esac)\"; rm -rf /; echo \";esac)\"",
        "echo \"${ case } in a) ;; }) echo \"}\"; rm -rf /;; esac; }\"",
        "echo \"${ case x in\n}|x) echo \"}\"; rm -rf /;; esac; }\"",
        "echo \"${ case x in a) ;; {) ;; esac; }\"; rm -rf /; echo \"; }\"",
        "echo \"$(time -p case x in x) echo \")\"; rm -rf /;; esac)\"",
        "echo \"$(coproc case x in x) echo \")\"; rm -rf /;; esac)\"",
        "echo \"$(f() case x in x) echo \")\"; rm -rf /;; esac; f)\"",
        "echo \"$(case\r x in x)\"; rm -rf /; echo \";esac)\"",
        "echo \"$(case x in x) echo \")\"; esac; rm -rf /)\"",
        "echo \"$(case x\nin x) echo \")\";;\nesac; rm -rf /)\"",
        "echo \"$(case x in x) case y in y) echo \")\";; esac;; esac; rm -rf /)\"",
        "echo \"$(case x in #c)\nx) echo \")\";; esac; rm -rf /)\"",
        "echo \"$(case x in x) echo \")\" ;;& y) ;& z) ;; esac; rm -rf /)\"",
        "echo \"$(if true; then case x in x) echo \")\";; esac; fi; rm -rf /)\"",
    ] {
        assert_denied_by_rm_rule(cmd).await;
    }
}

/// The counter-check: ordinary escapes, quotes, `#` in words and parameter
/// expansions stay `AutoExec` under `Allow "*"`.
#[tokio::test]
async fn test_adv_ordinary_quoting_stays_autoexec() {
    for cmd in [
        "echo a\\ b",
        "ls my\\ dir",
        "echo 'it'\\''s'",
        "echo \\#x",
        "git log --format='%h #%s'",
        "grep -E 'a|b'",
        "ps aux | grep java | sort",
        "echo \"${HOME}\"",
        "echo \"${var:-x}\"",
        "echo \"${x:-\"a b\"}\" ${#x} ${x#a}",
        "echo \"a (b) c\" 'd)'",
        "echo \"it's\"",
    ] {
        assert_benign_autoexec(cmd).await;
    }
}

/// Differential check against a real bash (skipped where none is
/// installed): every simple command bash would run for an input, also inside
/// a substitution, must be a command the parser sees (a segment or a command
/// in one of its substitutions), judged by its command word. Bash runs with
/// a `DEBUG` trap under `extdebug` that prints each command and skips it
/// unless it is `echo`, `printf`, `true`, `case` or an assignment (so their
/// substitutions are expanded), with stdout on `/dev/null`, an empty
/// environment and a `PATH` that does not exist. The inputs only name those
/// and made-up `probe_*` programs, so nothing else is executed.
#[cfg(unix)]
#[test]
fn test_adv_segments_match_bash() {
    use std::process::Command;

    const TRACE: &str = "exec 3>&1 >/dev/null; set -T; shopt -s extdebug; \
                         trap 'printf \"%s\\0\" \"$BASH_COMMAND\" >&3; \
                         case $BASH_COMMAND in echo*|printf*|true*|case*|x=*) true;; \
                         *) false;; esac' DEBUG";
    // Resolved here, since the child's own `PATH` points nowhere.
    let Some(bash) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("bash"))
            .find(|path| path.is_file())
    }) else {
        eprintln!("bash not found, differential test skipped");
        return;
    };
    // Bash 3 misreads comments and `case` inside `"$(...)"`.
    let major = Command::new(&bash)
        .args(["--norc", "--noprofile", "-c", "echo ${BASH_VERSINFO[0]}"])
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        });
    if major.is_none_or(|major| major < 4) {
        eprintln!("bash older than 4, differential test skipped");
        return;
    }
    let bash_commands = |cmd: &str| -> Option<Vec<String>> {
        let output = Command::new(&bash)
            .args(["--norc", "--noprofile", "-c", &format!("{TRACE}\n{cmd}")])
            .env_clear()
            .env("PATH", "/nonexistent-filter-probe")
            .current_dir(std::env::temp_dir())
            .output()
            .ok()?;
        Some(
            String::from_utf8_lossy(&output.stdout)
                .split('\0')
                .filter(|c| !c.is_empty())
                .map(str::to_string)
                .collect(),
        )
    };
    let command_word = |text: &str| -> String {
        // `shell_words` does not know `$(...)`; its first word is plain.
        let word = match shell_words::split(text) {
            Ok(words) => words.into_iter().next().unwrap_or_default(),
            Err(_) => text
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string(),
        };
        word.trim_matches(['(', ')']).to_string()
    };
    let mut cases = vec![
        "echo \\' ; probe_hidden -rf / ; echo \\'",
        "echo a #'\nprobe_hidden -rf /\n#'",
        "echo \\\" > /dev/null; echo RAN; echo \"\\\"\"",
        "probe_hid\\\nden -rf /",
        "\\probe_hidden x",
        "probe_\\hidden x",
        "echo \\; probe_not_run",
        "echo a\\\n; probe_hidden",
        "echo \"a\\\nb\"; probe_hidden",
        "echo a # x; probe_not_run\nprobe_hidden",
        "echo a;#'\nprobe_hidden",
        "echo ${x:- #}; probe_hidden",
        "# c \\\nprobe_hidden",
        "echo a\\ #b; probe_hidden",
        "echo $'\\''; probe_hidden",
        "echo 'a\\'; probe_hidden",
        "echo \"a\\\\\"; probe_hidden",
        "find . -exec probe_f {} \\;",
        "echo 'it'\\''s'; probe_hidden",
        "probe_a 'x'\\''; probe_b 'y'\\'''",
        "echo a\\ b\\ \\;c; probe_hidden",
        "probe_ls my\\ dir\\|x | probe_hidden",
        "probe_a | probe_b && probe_c; probe_d & probe_e",
        "echo a#b; echo $#; probe_hidden",
        // A `#` after a closing `)` starts a comment.
        "(probe_a)#'\nprobe_hidden\n#'",
        "case x in x)#'\nprobe_hidden\n#'\nesac",
        "echo $(echo a)#x; probe_hidden",
        // Quotes and comments inside a substitution in double quotes.
        "echo \"$(echo \")\"; probe_hidden)\"",
        "echo \"$(echo \"a\"; probe_hidden)\"",
        "echo \"$(echo \"it's\"; probe_hidden)\"",
        "echo \"$(#'\nprobe_hidden\n)\"",
        "echo \"$(#\"\n(probe_a)#\"\nprobe_hidden\n#\"\"\n)\"",
        "x=\"$(printf '%s' \"$(probe_hidden)\")\"",
        "echo $(echo ')'; probe_hidden)",
        "echo $(true\nprobe_hidden)",
        "echo \"$(case x in x) echo \")\";; esac; probe_hidden)\"",
        "echo \"${x:-$(echo \"}\"; probe_hidden)}\"",
        "echo \"`probe_hidden`\"",
        // `case` patterns and reserved words inside a substitution.
        "echo \"$(case x in esac)\"; probe_hidden; echo \";esac)\"",
        "echo \"$(case x in (esac) ;; x) echo \")\"; probe_hidden;; esac)\"",
        "echo \"$(case x in a|esac) ;; x) echo \")\"; probe_hidden;; esac)\"",
        "echo \"$(case x in\ncase) ;; esac)\"; probe_hidden; echo \";esac)\"",
        "echo \"$(case x\nin x) echo \")\";;\nesac; probe_hidden)\"",
        "echo \"$(case x in x) case y in y) echo \")\";; esac;; esac; probe_hidden)\"",
        "echo \"$(case x in x) echo \")\" ;;& y) ;& z) ;; esac; probe_hidden)\"",
    ];
    // Function substitutions exist since bash 5.3.
    if bash_commands("echo ${ true; }").is_some_and(|run| run.len() > 1) {
        cases.extend([
            "echo \"${ probe_hidden; }\"",
            "echo ${ probe_hidden; }",
            "echo ${| probe_hidden; }",
            "echo \"${ echo \")\"; probe_hidden; }\"",
            "echo \"${ { echo a; }; probe_hidden; }x\"",
            "echo ${ echo a }; probe_hidden; }",
            "echo \"${ case } in a) ;; }) echo \"}\"; probe_hidden;; esac; }\"",
            "echo \"${ case x in a) ;; {) ;; esac; }\"; probe_hidden; echo \"; }\"",
        ]);
    } else {
        eprintln!("bash without function substitution, those cases skipped");
    }
    for cmd in cases {
        let Some(segments) = split_top_level_segments(cmd) else {
            panic!("{cmd:?}: the parser must split this input");
        };
        let commands = parser::segment_command(cmd);
        let seen: Vec<String> = commands.iter().map(|s| command_word(s)).collect();
        let run = bash_commands(cmd).expect("bash ran before");
        assert!(!run.is_empty(), "{cmd:?}: bash ran nothing");
        if cmd.contains("probe_hidden") {
            assert!(
                run.iter()
                    .any(|command| command_word(command) == "probe_hidden"),
                "{cmd:?}: bash does not run probe_hidden, the case proves nothing: {run:?}"
            );
        }
        for command in &run {
            let word = command_word(command);
            // Assignments are not matched by their word.
            if word.contains('=') {
                continue;
            }
            assert!(
                seen.contains(&word),
                "{cmd:?}: bash runs {command:?}, parser sees only {commands:?} \
                 (segments {segments:?})"
            );
        }
    }
}
