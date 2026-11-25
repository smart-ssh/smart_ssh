use super::*;

fn canonical(cmd: &str) -> String {
    lex(cmd, CommentMode::Strip).expect("lexes").canonical
}

fn substitutions(cmd: &str) -> Vec<String> {
    let lexed = lex(cmd, CommentMode::Strip).expect("lexes");
    let chars: Vec<char> = lexed.canonical.chars().collect();
    lexed
        .substitutions
        .iter()
        .map(|found| chars[found.inner.clone()].iter().collect())
        .collect()
}

#[test]
fn function_substitutions_are_command_lists() {
    assert_eq!(substitutions("echo \"${ rm -rf /; }\""), ["rm -rf /; "]);
    assert_eq!(substitutions("echo ${| rm x; }"), [" rm x; "]);
    assert_eq!(substitutions("echo ${\nls\n}x"), ["ls\n"]);
    // `}` ends it only in command position, a `{ ...; }` group inside
    // does not.
    assert_eq!(substitutions("echo ${ echo a }; ls; }"), ["echo a }; ls; "]);
    assert_eq!(substitutions("echo ${ { a; }; b; }"), ["{ a; }; b; "]);
    assert_eq!(
        lexed_segments("echo \"${ echo \")\"; rm x; }\"").unwrap(),
        ["echo \"${ echo \")\"; rm x; }\""]
    );
    for text in [
        "echo ${x} ${#x} ${x:-a b}",
        "echo \"${HOME}\" \"${x:-\"}\"}\"",
    ] {
        assert!(substitutions(text).is_empty(), "{text:?}");
    }
    assert!(lex("echo ${ ls", CommentMode::Strip).is_none());
}

#[test]
fn comment_after_closing_paren() {
    assert_eq!(canonical("(true)#'\nls\n#'"), "(true)\nls\n");
    assert_eq!(
        canonical("case x in x)#'\nls\nesac"),
        "case x in x)\nls\nesac"
    );
    assert_eq!(canonical("echo a >#x\nls"), "echo a >\nls");
    // The `)` of a substitution ends no word.
    assert!(
        !lex("echo $(ls)#x <(ls)#y", CommentMode::Strip)
            .unwrap()
            .comment
    );
}

#[test]
fn case_patterns_are_no_command_position() {
    // `esac` ends the statement only at the start of a pattern list or in
    // command position; `case`, `{`, `}` and `esac` elsewhere in a
    // pattern are words.
    for (text, inner) in [
        ("echo \"$(case x in esac)\"; rm x", "case x in esac"),
        (
            "echo \"$(case x in (esac) ;; y) echo \")\";; esac)\"",
            "case x in (esac) ;; y) echo \")\";; esac",
        ),
        (
            "echo \"$(case x in a|esac) echo \")\";; esac)\"",
            "case x in a|esac) echo \")\";; esac",
        ),
        (
            "echo \"$(case x in\ncase) ;; esac)\"; rm x",
            "case x in\ncase) ;; esac",
        ),
        ("echo \"$(case x in a) ls; esac)\"", "case x in a) ls; esac"),
        (
            "echo \"$(time -p case x in a) echo \")\";; esac)\"",
            "time -p case x in a) echo \")\";; esac",
        ),
        (
            "echo \"$(f() case x in a) echo \")\";; esac)\"",
            "f() case x in a) echo \")\";; esac",
        ),
        ("echo \"$(echo case in esac)\"", "echo case in esac"),
    ] {
        assert_eq!(substitutions(text), [inner], "{text:?}");
    }
    for text in [
        "echo \"${ coproc foo { a; }; echo \"}\"; rm x; }\"",
        "echo \"${ function f { a; }; echo \"}\"; rm x; }\"",
        "echo \"${ f ( ) { a; }; echo \"}\"; rm x; }\"",
        "echo \"${ for ((i=0;i<1;i++)) { a; }; echo \"}\"; rm x; }\"",
        "echo \"${ time -p -- { a; }; echo \"}\"; rm x; }\"",
    ] {
        let found = substitutions(text);
        assert!(
            found.len() == 1 && found[0].ends_with("rm x; "),
            "{text:?}: {found:?}"
        );
    }
    assert_eq!(
        substitutions("echo \"${ case x in a) ;; {) ;; esac; }\"; rm x"),
        ["case x in a) ;; {) ;; esac; "]
    );
    // A carriage return is no blank: `case\r` is an ordinary word.
    assert_eq!(
        substitutions("echo \"$(case\r x in x)\"; rm x"),
        ["case\r x in x"]
    );
    // The clauses of a top-level `case` stay at the top level.
    assert_eq!(
        lexed_segments("case x in a) ls;; esac; rm x").unwrap(),
        ["case x in a) ls", "esac", "rm x"]
    );
}

#[test]
fn substitution_in_double_quotes_is_a_command_list() {
    assert_eq!(
        substitutions("echo \"$(echo \")\"; rm x)\""),
        ["echo \")\"; rm x"]
    );
    assert_eq!(
        substitutions("echo \"$(echo 'a)'; rm x)\""),
        ["echo 'a)'; rm x"]
    );
    assert_eq!(substitutions("echo \"$(#'\nrm x\n)\""), ["\nrm x\n"]);
    assert_eq!(substitutions("echo \"$(echo a\\ b)\""), ["echo a' 'b"]);
    assert_eq!(
        substitutions("echo \"$(case x in x) echo \")\";; esac; rm x)\""),
        ["case x in x) echo \")\";; esac; rm x"]
    );
    assert_eq!(
        substitutions("echo \"a $(b \"$(c)\") `d`\" ${x:-$(e)}"),
        ["b \"$(c)\"", "d", "e"]
    );
    assert_eq!(
        lexed_segments("echo \"$(echo \")\"; rm x)\"; ls").unwrap(),
        ["echo \"$(echo \")\"; rm x)\"", "ls"]
    );
}

#[test]
fn unquoted_escapes_become_quotes() {
    assert_eq!(canonical("echo \\' ; ls"), "echo \"'\" ; ls");
    assert_eq!(canonical("echo \\\" x"), "echo '\"' x");
    assert_eq!(canonical("echo a\\ b"), "echo a' 'b");
    assert_eq!(canonical("\\rm x"), "'r'm x");
    assert_eq!(
        canonical("find . -exec ls {} \\;"),
        "find . -exec ls {} ';'"
    );
    let lexed = lex("echo a\\ b", CommentMode::Strip).unwrap();
    assert!(lexed.escape && !lexed.continuation && !lexed.comment);
}

#[test]
fn line_continuations_are_removed() {
    assert_eq!(canonical("r\\\nm -rf /"), "rm -rf /");
    assert_eq!(canonical("echo \"a\\\nb\""), "echo \"ab\"");
    // Inside single quotes nothing is special.
    assert_eq!(canonical("echo 'a\\\nb'"), "echo 'a\\\nb'");
    let lexed = lex("r\\\nm", CommentMode::Strip).unwrap();
    assert!(lexed.continuation && !lexed.escape);
}

#[test]
fn comments_start_only_at_a_word_start() {
    assert_eq!(canonical("echo a # x\nls"), "echo a \nls");
    assert_eq!(canonical("echo a;#'\nls"), "echo a;\nls");
    assert_eq!(canonical("# only\nls"), "\nls");
    assert_eq!(canonical("echo $(#c\nls)"), "echo $(\nls)");
    // A backslash in a comment is no continuation.
    assert_eq!(canonical("# c \\\nls"), "\nls");
    for text in [
        "echo a#b",
        "echo $#",
        "echo 'a # b'",
        "echo \"a # b\"",
        "echo ${x:- #}",
        "echo ${#x}",
        "echo a\\ #b",
        "echo $(ls)#x",
    ] {
        let lexed = lex(text, CommentMode::Strip).unwrap();
        assert!(!lexed.comment, "{text:?}");
    }
}

#[test]
fn comment_as_code_keeps_the_text() {
    let lexed = lex("echo a # x; ls", CommentMode::AsCode).unwrap();
    assert!(lexed.comment);
    assert_eq!(lexed.canonical, "echo a '#' x; ls");
}

#[test]
fn ansi_c_strings_keep_their_extent() {
    assert_eq!(canonical("echo $'a\\'b' ; ls"), "echo $'a\\x27b' ; ls");
    assert_eq!(canonical("echo $'\\\\' ; ls"), "echo $'\\\\' ; ls");
    assert!(lex("echo $'a\\'", CommentMode::Strip).is_none());
}

#[test]
fn unterminated_input_does_not_lex() {
    for text in ["echo 'a", "echo \"a", "echo `a", "echo a\\", "echo \"a\\\""] {
        assert!(lex(text, CommentMode::Strip).is_none(), "{text:?}");
    }
}

#[test]
fn scanner_rejects_unbalanced_text() {
    for text in [
        "echo 'a",
        "echo \"a",
        "echo `a",
        "echo $(a",
        "echo <(a",
        "a $(b $(c) ; d",
    ] {
        assert_eq!(lexed_segments(text), None, "{text:?}");
    }
    assert_eq!(
        lexed_segments("a 'x;y' \"p\\\";q\" `r;s` $(t;u) ; b").unwrap(),
        ["a 'x;y' \"p\\\";q\" `r;s` $(t;u)", "b"]
    );
}

#[test]
fn backslash_in_backticks_is_reported() {
    let lexed = lex("echo `echo \\` x \\``", CommentMode::Strip).unwrap();
    assert!(lexed.escape_in_backticks);
    assert!(
        !lex("echo `ls`", CommentMode::Strip)
            .unwrap()
            .escape_in_backticks
    );
}

#[test]
fn plain_commands_are_unchanged() {
    for text in [
        "ls -la",
        "ps aux | grep java && echo 'done' || echo \"x\\\"y\"",
        "echo $(date) ${HOME} $'abc'",
        "ls 2>&1 >/dev/null",
    ] {
        let lexed = lex(text, CommentMode::Strip).unwrap();
        assert_eq!(lexed.canonical, text);
        assert!(opaque_syntax_reason(text).is_none(), "{text:?}");
    }
}

#[test]
fn only_continuations_comments_and_backtick_escapes_are_opaque() {
    for text in [
        "echo a\\ b",
        "echo \\' x",
        "find . -exec ls {} \\;",
        "echo 'it'\\''s'",
    ] {
        assert!(opaque_syntax_reason(text).is_none(), "{text:?}");
    }
    for text in [
        "r\\\nm x",
        "echo \"a\\\nb\"",
        "echo a # c",
        "echo `echo \\` x \\``",
    ] {
        assert!(opaque_syntax_reason(text).is_some(), "{text:?}");
    }
}

#[test]
fn lexed_segments_see_the_hidden_commands() {
    assert_eq!(
        lexed_segments("echo \\' ; rm -rf / ; echo \\'").unwrap(),
        ["echo \"'\"", "rm -rf /", "echo \"'\""]
    );
    assert_eq!(
        lexed_segments("echo a #'\nrm -rf /\n#'").unwrap(),
        ["echo a", "rm -rf /", "echo a '#''\nrm -rf /\n#'"]
    );
    assert_eq!(lexed_segments("r\\\nm -rf /").unwrap(), ["rm -rf /"]);
    // The comment is also read as code; that view splits here.
    assert_eq!(
        lexed_segments("echo a # x; rm -rf /").unwrap(),
        ["echo a", "echo a '#' x", "rm -rf /"]
    );
}

#[test]
fn parameter_expansion_text_is_also_read_as_code() {
    assert_eq!(
        lexed_segments("echo ${a;rm -rf /;}; ls").unwrap(),
        ["echo ${a;rm -rf /;}", "ls", "echo ${a", "rm -rf /", "}"]
    );
    assert_eq!(
        lexed_segments("echo ${x:-a|b} \"${y;z}\"").unwrap(),
        ["echo ${x:-a|b} \"${y;z}\"", "echo ${x:-a", "b} \"${y;z}\""]
    );
    assert_eq!(
        lexed_segments("echo ${x:-a b}").unwrap(),
        ["echo ${x:-a b}"]
    );
}

#[test]
fn bad_substitutions_are_reported() {
    for text in [
        "echo ${a;b}",
        "echo ${a|b}",
        "echo ${a b}",
        "echo ${}",
        "echo ${1a}",
        "echo ${#a;b}",
        "echo \"${(x)}\"",
        "echo ${x:-${a&b}}",
    ] {
        assert!(
            lex(text, CommentMode::Strip).unwrap().bad_substitution,
            "{text:?}"
        );
        assert!(opaque_syntax_reason(text).is_some(), "{text:?}");
    }
    for text in [
        "echo ${a} ${_a1:-x} ${10} ${#} ${#a} ${!a} ${!a*} ${!a@} ${a[1]} ${a@Q}",
        "echo ${$} ${?} ${-} ${!} ${@} ${*} ${##} ${a//x/y} ${a^^} ${a,} ${a%%x}",
        "echo ${ ls; } ${| ls; }",
    ] {
        assert!(
            !lex(text, CommentMode::Strip).unwrap().bad_substitution,
            "{text:?}"
        );
    }
}
