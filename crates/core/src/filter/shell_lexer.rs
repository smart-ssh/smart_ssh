//! Shell lexing shared by the filter parser (Spec 0002, Abschnitt 4).
//!
//! The segment scanner and the helpers behind it (redirection detection,
//! substitution stripping, first-word unquoting) only understand single and
//! double quotes. A real shell also reads unquoted backslash escapes, `#`
//! comments and backslash-newline line continuations, so an escaped quote
//! (`echo \' ; rm -rf / ; echo \'`), a quote inside a comment or a
//! continuation inside a program name (`r\<LF>m`) used to make the scanner
//! see other commands than the shell runs.
//!
//! [`lex`] therefore rewrites a command into a canonical text the shell
//! reads the same way, but which holds none of these constructs: an
//! unquoted `\x` becomes `'x'` (`\'` becomes `"'"`), a line continuation is
//! removed, and a comment is removed up to (not including) its newline. Every
//! later step works on that text. Whether any such construct was present is
//! reported; continuations, comments and backslashes inside backticks need a
//! confirmation (fail closed, see [`opaque_syntax_reason`]), plain escapes
//! are evaluated against the rules like quoted text.

use super::parser::non_writing_redirection_len;

/// What to do with a `#` the lexer reads as the start of a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommentMode {
    /// Drop the comment, as the shell does.
    Strip,
    /// Keep it as quoted text (`'#'`), so its content is read as code. Used
    /// as a second view: if the lexer took something for a comment that the
    /// shell runs, the commands in it are still evaluated.
    AsCode,
}

/// Result of [`lex`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Lexed {
    /// Text the shell reads like the input, without unquoted backslashes,
    /// line continuations and (in [`CommentMode::Strip`]) comments.
    pub(super) canonical: String,
    /// An unquoted backslash escape was found (`\x`, not a continuation).
    pub(super) escape: bool,
    /// A backslash-newline line continuation was found (unquoted or inside
    /// double quotes).
    pub(super) continuation: bool,
    /// A `#` comment was found.
    pub(super) comment: bool,
    /// A backslash inside backticks, where it can nest a substitution the
    /// scanner does not follow.
    pub(super) escape_in_backticks: bool,
}

/// Context the lexer is in: a command list (top level, `$(`, `<(`, `>(`,
/// `(`) or a parameter expansion `${...}`, where `#` never starts a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    Commands,
    Parameter,
}

/// Lexes `cmd` as a shell would read it. Returns `None` if a quote, an
/// ANSI-C string or a backtick is not closed, or the input ends with a lone
/// backslash: the caller then treats the command as not safely analysable.
pub(super) fn lex(cmd: &str, mode: CommentMode) -> Option<Lexed> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut out = Lexed {
        canonical: String::with_capacity(cmd.len()),
        escape: false,
        continuation: false,
        comment: false,
        escape_in_backticks: false,
    };
    let mut stack: Vec<Context> = Vec::new();
    // True where a new word begins; only there does `#` start a comment.
    // Conservative on purpose: after `)`, `<`, `>` or `\r` it stays false,
    // so in doubt the text is read as code (more segments, never fewer).
    let mut word_start = true;
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let in_parameter = stack.last() == Some(&Context::Parameter);
        match c {
            '\\' => match next {
                Some('\n') => {
                    out.continuation = true;
                    i += 2;
                    continue;
                }
                Some('\'') => {
                    out.escape = true;
                    out.canonical.push_str("\"'\"");
                    i += 2;
                }
                Some(escaped) => {
                    out.escape = true;
                    out.canonical.push('\'');
                    out.canonical.push(escaped);
                    out.canonical.push('\'');
                    i += 2;
                }
                None => return None,
            },
            '\'' => {
                let end = find_char(&chars, i + 1, '\'')?;
                out.canonical.extend(&chars[i..=end]);
                i = end + 1;
            }
            '$' if next == Some('\'') => i = lex_ansi_c(&chars, i, &mut out)?,
            '"' => i = lex_double_quoted(&chars, i, &mut out)?,
            '`' => i = lex_backticks(&chars, i, &mut out)?,
            '#' if word_start && !in_parameter => {
                out.comment = true;
                match mode {
                    CommentMode::Strip => {
                        i = find_char(&chars, i, '\n').unwrap_or(chars.len());
                        continue;
                    }
                    CommentMode::AsCode => {
                        out.canonical.push_str("'#'");
                        i += 1;
                    }
                }
            }
            '$' | '<' | '>' if next == Some('(') => {
                stack.push(Context::Commands);
                out.canonical.push(c);
                out.canonical.push('(');
                i += 2;
                word_start = true;
                continue;
            }
            '$' if next == Some('{') => {
                stack.push(Context::Parameter);
                out.canonical.push_str("${");
                i += 2;
            }
            '(' if !in_parameter => {
                stack.push(Context::Commands);
                out.canonical.push(c);
                i += 1;
                word_start = true;
                continue;
            }
            ')' if stack.last() == Some(&Context::Commands) => {
                stack.pop();
                out.canonical.push(c);
                i += 1;
            }
            '}' if in_parameter => {
                stack.pop();
                out.canonical.push(c);
                i += 1;
            }
            ' ' | '\t' | '\n' | ';' | '&' | '|' => {
                out.canonical.push(c);
                i += 1;
                word_start = true;
                continue;
            }
            _ => {
                out.canonical.push(c);
                i += 1;
            }
        }
        word_start = false;
    }
    Some(out)
}

/// Index of the first `target` at or after `from`.
fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    chars
        .get(from..)?
        .iter()
        .position(|&c| c == target)
        .map(|offset| from + offset)
}

/// `$'...'` at `start`: inside, `\` escapes the next character, so `\'` does
/// not end the string. It is copied as is, except that `\'` becomes the
/// equivalent `\x27`, so a scanner that ends single quotes at the next `'`
/// finds the same end. Returns the index after the closing quote.
fn lex_ansi_c(chars: &[char], start: usize, out: &mut Lexed) -> Option<usize> {
    out.canonical.push_str("$'");
    let mut i = start + 2;
    loop {
        match *chars.get(i)? {
            '\'' => {
                out.canonical.push('\'');
                return Some(i + 1);
            }
            '\\' => {
                let escaped = *chars.get(i + 1)?;
                if escaped == '\'' {
                    out.canonical.push_str("\\x27");
                } else {
                    out.canonical.push('\\');
                    out.canonical.push(escaped);
                }
                i += 2;
            }
            other => {
                out.canonical.push(other);
                i += 1;
            }
        }
    }
}

/// `"..."` at `start`: a backslash-newline is removed (line continuation),
/// any other backslash pair is copied unchanged. Returns the index after the
/// closing quote.
fn lex_double_quoted(chars: &[char], start: usize, out: &mut Lexed) -> Option<usize> {
    out.canonical.push('"');
    let mut i = start + 1;
    loop {
        match *chars.get(i)? {
            '"' => {
                out.canonical.push('"');
                return Some(i + 1);
            }
            '\\' => {
                let escaped = *chars.get(i + 1)?;
                if escaped == '\n' {
                    out.continuation = true;
                } else {
                    out.canonical.push('\\');
                    out.canonical.push(escaped);
                }
                i += 2;
            }
            other => {
                out.canonical.push(other);
                i += 1;
            }
        }
    }
}

/// `` `...` `` at `start`: copied unchanged; a backslash inside is reported,
/// since `` \` `` nests a substitution. Returns the index after the closing
/// backtick.
fn lex_backticks(chars: &[char], start: usize, out: &mut Lexed) -> Option<usize> {
    out.canonical.push('`');
    let mut i = start + 1;
    loop {
        match *chars.get(i)? {
            '`' => {
                out.canonical.push('`');
                return Some(i + 1);
            }
            '\\' => {
                out.escape_in_backticks = true;
                out.canonical.push('\\');
                out.canonical.push(*chars.get(i + 1)?);
                i += 2;
            }
            other => {
                out.canonical.push(other);
                i += 1;
            }
        }
    }
}

/// Fail closed (Spec 0002, Abschnitt 4.4): a line continuation, a comment
/// or a backslash inside backticks makes the text differ from what the
/// engine evaluates (or, in backticks, leaves a nesting unresolved), so such
/// a command needs at least a confirmation. A plain unquoted backslash
/// escape does not: [`lex`] resolves it exactly into quotes, and the
/// canonical text is evaluated against the rules like any quoted input. The
/// regular evaluation always runs, so a `Deny` behind any of these keeps
/// winning. Returns the reason, or `None`. Input that does not lex at all is
/// already rejected as not safely analysable by the parser.
pub(super) fn opaque_syntax_reason(cmd: &str) -> Option<&'static str> {
    let lexed = lex(cmd, CommentMode::Strip)?;
    if lexed.continuation {
        return Some(
            "Kommando enthält eine Zeilenfortsetzung (Backslash vor Zeilenumbruch) \
             und konnte nicht sicher analysiert werden",
        );
    }
    if lexed.escape_in_backticks {
        return Some(
            "Kommando enthält einen Backslash in Backticks und konnte nicht sicher \
             analysiert werden",
        );
    }
    if lexed.comment {
        return Some(
            "Kommando enthält einen Shell-Kommentar (#) und konnte nicht sicher \
             analysiert werden",
        );
    }
    None
}

/// Lexes `cmd` and splits the canonical text into its top-level segments.
/// If the lexer found a comment, the segments of the [`CommentMode::AsCode`]
/// view are added (when that view can be split), so a misread comment can
/// only add segments to evaluate, never hide one. `None` if the text cannot
/// be lexed or split safely.
pub(super) fn lexed_segments(cmd: &str) -> Option<Vec<String>> {
    let lexed = lex(cmd, CommentMode::Strip)?;
    let canonical = lexed.canonical.trim();
    if shell_words::split(canonical).is_err() {
        return None;
    }
    let mut segments = scan_top_level_segments(canonical)?;
    if lexed.comment {
        let as_code = lex(cmd, CommentMode::AsCode).map(|view| view.canonical);
        let extra = as_code
            .filter(|text| shell_words::split(text.trim()).is_ok())
            .and_then(|text| scan_top_level_segments(text.trim()))
            .unwrap_or_default();
        for segment in extra {
            if !segments.contains(&segment) {
                segments.push(segment);
            }
        }
    }
    Some(segments)
}

/// Zerlegt `cmd` an den Top-Level-Operatoren `&&`, `||`, `;`, `|`, `&` sowie
/// Zeilenumbrüchen (`\n`, `\r`, `\r\n`), ohne dabei in einfache/doppelte
/// Anführungszeichen, Backticks, `$(...)`, `<(...)` oder `>(...)`
/// hineinzuspalten. Gibt `None` zurück, wenn am Ende Quotes/Klammern nicht
/// ausgeglichen sind. Expects canonical text from [`lex`] (no unquoted
/// backslashes or comments).
pub(super) fn scan_top_level_segments(cmd: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut segments = Vec::new();
    let mut current_start = 0usize;
    let mut i = 0usize;
    let mut paren_depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;

    while i < chars.len() {
        let c = chars[i];

        if in_single {
            if c == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if c == '`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }

        match c {
            '\'' => {
                in_single = true;
                i += 1;
                continue;
            }
            '"' => {
                in_double = true;
                i += 1;
                continue;
            }
            '`' => {
                in_backtick = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        if (c == '$' || c == '<' || c == '>') && chars.get(i + 1) == Some(&'(') {
            paren_depth += 1;
            i += 2;
            continue;
        }
        if c == ')' && paren_depth > 0 {
            paren_depth -= 1;
            i += 1;
            continue;
        }

        if paren_depth == 0 {
            match c {
                '\n' | '\r' => {
                    push_segment(&chars, current_start, i, &mut segments);
                    if c == '\r' && chars.get(i + 1) == Some(&'\n') {
                        i += 1;
                    }
                    i += 1;
                    current_start = i;
                    continue;
                }
                '&' if chars.get(i + 1) == Some(&'&') => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 2;
                    current_start = i;
                    continue;
                }
                // `&>/dev/null` is one redirection operator, not a background
                // `&` followed by `>`: keep it inside the segment.
                '&' if chars.get(i + 1) == Some(&'>')
                    && non_writing_redirection_len(&chars, i + 1).is_some() =>
                {
                    i += 1;
                    continue;
                }
                // Likewise `2>&1` / `>&2`: the `&` belongs to the fd operator.
                '&' if i > 0
                    && chars[i - 1] == '>'
                    && non_writing_redirection_len(&chars, i - 1).is_some() =>
                {
                    i += 1;
                    continue;
                }
                '&' => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 1;
                    current_start = i;
                    continue;
                }
                '|' if chars.get(i + 1) == Some(&'|') => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 2;
                    current_start = i;
                    continue;
                }
                '|' | ';' => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 1;
                    current_start = i;
                    continue;
                }
                _ => {}
            }
        }

        i += 1;
    }

    push_segment(&chars, current_start, chars.len(), &mut segments);

    if in_single || in_double || in_backtick || paren_depth != 0 {
        return None;
    }
    Some(segments)
}

fn push_segment(chars: &[char], start: usize, end: usize, segments: &mut Vec<String>) {
    if start >= end {
        return;
    }
    let trimmed: String = chars[start..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string();
    if !trimmed.is_empty() {
        segments.push(trimmed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(cmd: &str) -> String {
        lex(cmd, CommentMode::Strip).expect("lexes").canonical
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
            assert_eq!(scan_top_level_segments(text), None, "{text:?}");
        }
        assert_eq!(
            scan_top_level_segments("a 'x;y' \"p\\\";q\" `r;s` $(t;u) ; b").unwrap(),
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
}
