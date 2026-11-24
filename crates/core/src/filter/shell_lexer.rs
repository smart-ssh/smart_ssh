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
//!
//! The lexer also follows the nesting the shell reads: a command list inside
//! `$(...)`, `<(...)`, `>(...)` and bash 5.3's `${ ...; }` / `${| ...; }`,
//! also within double quotes, with its own quotes, comments, subshells and
//! `case` patterns; parameter expansions, in which quotes nest. It reports
//! where the top level is and where each substitution starts and ends, so the
//! segment scanner ([`scan_top_level_segments`]) and the substitution
//! stripping (`parser::strip_substitutions`) read the same structure as the
//! lexer instead of counting quotes and parentheses themselves.

use std::ops::Range;

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

/// Result of [`lex`]. Positions are char indices into `canonical`.
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
    /// Per char: in the top-level command list, outside any quote,
    /// expansion or substitution. Only there do operators split segments.
    pub(super) top_level: Vec<bool>,
    /// Command substitutions not nested in another one, in order, also
    /// those inside double quotes or a parameter expansion.
    pub(super) substitutions: Vec<Substitution>,
    /// Contents of single-quoted and ANSI-C strings outside substitutions.
    pub(super) single_quoted: Vec<Range<usize>>,
}

/// A command substitution: `$(...)`, `<(...)`, `>(...)`, `` `...` ``, or
/// bash 5.3's `${ ...; }` and `${| ...; }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Substitution {
    pub(super) whole: Range<usize>,
    /// The command list inside.
    pub(super) inner: Range<usize>,
}

/// What ends a command list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closer {
    /// The end of the input: the top level.
    End,
    /// `)`: `$(...)`, `<(...)`, `>(...)`.
    Paren,
    /// `}` in command position: `${ ...; }`, `${| ...; }`.
    Brace,
}

/// A command list being read.
#[derive(Debug, Clone, Copy)]
struct List {
    closer: Closer,
    /// Start of the substitution and of its inner list, if it is not nested
    /// in another substitution.
    start: Option<(usize, usize)>,
    /// Open `(` subshells inside the list.
    parens: usize,
    /// Open `{` groups inside the list.
    braces: usize,
    /// Open `case` statements; there a `)` ends a pattern, not the list.
    cases: usize,
    /// A new word begins here; only there does `#` start a comment.
    word_start: bool,
    /// A command begins here; only there are `case`, `esac`, `{` and `}`
    /// reserved words.
    command_start: bool,
}

impl List {
    fn new(closer: Closer) -> Self {
        Self {
            closer,
            start: None,
            parens: 0,
            braces: 0,
            cases: 0,
            word_start: true,
            command_start: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Context {
    Commands(List),
    DoubleQuoted,
    /// `${...}`, `quoted` inside double quotes.
    Parameter {
        quoted: bool,
    },
}

/// Reserved words after which a command follows.
const COMMAND_PREFIX_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "time", "{",
];

/// Lexes `cmd` as a shell would read it. Returns `None` if a quote, an
/// ANSI-C string, a backtick, an expansion or a substitution is not closed,
/// or the input ends with a lone backslash: the caller then treats the
/// command as not safely analysable.
pub(super) fn lex(cmd: &str, mode: CommentMode) -> Option<Lexed> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut lexer = Lexer {
        chars: &chars,
        mode,
        stack: vec![Context::Commands(List::new(Closer::End))],
        len: 0,
        out: Lexed {
            canonical: String::with_capacity(cmd.len()),
            escape: false,
            continuation: false,
            comment: false,
            escape_in_backticks: false,
            top_level: Vec::with_capacity(chars.len()),
            substitutions: Vec::new(),
            single_quoted: Vec::new(),
        },
    };
    let mut i = 0usize;
    while i < chars.len() {
        i = match *lexer.stack.last()? {
            Context::Commands(list) => lexer.in_commands(i, list)?,
            Context::DoubleQuoted => lexer.in_double_quotes(i)?,
            Context::Parameter { quoted } => lexer.in_parameter(i, quoted)?,
        };
    }
    (lexer.stack.len() == 1).then_some(lexer.out)
}

struct Lexer<'a> {
    chars: &'a [char],
    mode: CommentMode,
    /// Innermost context last; the top-level command list stays at the
    /// bottom.
    stack: Vec<Context>,
    /// Length of `out.canonical` in chars.
    len: usize,
    out: Lexed,
}

impl Lexer<'_> {
    fn in_commands(&mut self, i: usize, list: List) -> Option<usize> {
        let c = self.chars[i];
        let next = self.chars.get(i + 1).copied();
        match c {
            '\\' => return self.unquoted_escape(i),
            '\'' => return self.single_quoted(i),
            '$' if next == Some('\'') => return self.ansi_c(i),
            '"' => self.open(Context::DoubleQuoted, &['"']),
            '`' => return self.backticks(i),
            '#' if list.word_start => return Some(self.comment(i)),
            '$' | '<' | '>' if next == Some('(') => {
                self.open_list(&[c, '('], Closer::Paren);
                return Some(i + 2);
            }
            '$' if next == Some('{') => return Some(self.dollar_brace(i, false)),
            '(' => {
                self.push(c);
                self.update(|list| list.parens += 1);
                self.mark(true, true);
            }
            // After a subshell, a case pattern or `f()` a new word begins,
            // so a `#` there starts a comment, as after `<` and `>`. In doubt
            // `#` is better read as a comment: the `AsCode` view still
            // evaluates its text and a comment needs confirmation, while a
            // comment read as code can let a quote in it hide the next line.
            ')' if list.parens > 0 => {
                self.push(c);
                self.update(|list| list.parens -= 1);
                self.mark(true, false);
            }
            ')' if list.cases > 0 => {
                self.push(c);
                self.mark(true, true);
            }
            ')' if list.closer == Closer::Paren => self.close(c),
            ')' | '<' | '>' => {
                self.push(c);
                self.mark(true, false);
            }
            ' ' | '\t' => {
                self.push(c);
                self.mark(true, list.command_start);
            }
            '\n' | ';' | '&' | '|' => {
                self.push(c);
                self.mark(true, true);
            }
            _ if list.word_start && list.command_start => {
                if let Some(word) = self.reserved_word(i) {
                    return Some(self.reserved(i, word, list));
                }
                self.push(c);
                self.mark(false, false);
            }
            _ => {
                self.push(c);
                self.mark(false, false);
            }
        }
        Some(i + 1)
    }

    fn in_double_quotes(&mut self, i: usize) -> Option<usize> {
        let c = self.chars[i];
        match (c, self.chars.get(i + 1).copied()) {
            ('"', _) => self.close(c),
            ('\\', next) => return self.quoted_escape(i, next?),
            ('$', Some('(')) => {
                self.open_list(&['$', '('], Closer::Paren);
                return Some(i + 2);
            }
            ('$', Some('{')) => return Some(self.dollar_brace(i, true)),
            ('`', _) => return self.backticks(i),
            _ => self.push(c),
        }
        Some(i + 1)
    }

    /// Inside `${...}` quotes nest, also within double quotes, and `#` never
    /// starts a comment.
    fn in_parameter(&mut self, i: usize, quoted: bool) -> Option<usize> {
        let c = self.chars[i];
        match (c, self.chars.get(i + 1).copied()) {
            ('}', _) => self.close(c),
            ('\\', next) if quoted => return self.quoted_escape(i, next?),
            ('\\', _) => return self.unquoted_escape(i),
            ('\'', _) => return self.single_quoted(i),
            // bash reads `$'...'` here also within double quotes (`extquote`).
            ('$', Some('\'')) => return self.ansi_c(i),
            ('"', _) => self.open(Context::DoubleQuoted, &['"']),
            ('`', _) => return self.backticks(i),
            ('$', Some('(')) => {
                self.open_list(&['$', '('], Closer::Paren);
                return Some(i + 2);
            }
            ('<' | '>', Some('(')) if !quoted => {
                self.open_list(&[c, '('], Closer::Paren);
                return Some(i + 2);
            }
            ('$', Some('{')) => return Some(self.dollar_brace(i, quoted)),
            _ => self.push(c),
        }
        Some(i + 1)
    }

    /// The reserved word starting at `i`, if it is a whole word.
    fn reserved_word(&self, i: usize) -> Option<&'static str> {
        ["case", "esac", "}"]
            .iter()
            .chain(COMMAND_PREFIX_WORDS)
            .copied()
            .find(|word| {
                let end = i + word.chars().count();
                // `}` needs no delimiter after it: `${ echo a; }x`.
                self.chars
                    .get(i..end)
                    .is_some_and(|text| text.iter().copied().eq(word.chars()))
                    && (*word == "}"
                        || self.chars.get(end).is_none_or(|&c| {
                            c.is_whitespace()
                                || matches!(c, ';' | '&' | '|' | '(' | ')' | '<' | '>')
                        }))
            })
    }

    /// Reads the reserved `word` at `i` in command position. Returns the
    /// index after it.
    fn reserved(&mut self, i: usize, word: &str, list: List) -> usize {
        let end = i + word.chars().count();
        match word {
            "}" if list.braces == 0 && list.closer == Closer::Brace => {
                self.close('}');
                return end;
            }
            "}" => self.update(|list| list.braces = list.braces.saturating_sub(1)),
            "{" => self.update(|list| list.braces += 1),
            "case" => self.update(|list| list.cases += 1),
            "esac" => self.update(|list| list.cases = list.cases.saturating_sub(1)),
            _ => {}
        }
        for &c in &self.chars[i..end] {
            self.push(c);
        }
        self.mark(false, COMMAND_PREFIX_WORDS.contains(&word));
        end
    }

    /// `${` at `i`: a parameter expansion, or (followed by a blank or `|`)
    /// a bash 5.3 command substitution. Returns the index after the opener.
    fn dollar_brace(&mut self, i: usize, quoted: bool) -> usize {
        match self.chars.get(i + 2).copied() {
            Some(c @ (' ' | '\t' | '\n' | '|')) => {
                self.open_list(&['$', '{', c], Closer::Brace);
                i + 3
            }
            _ => {
                self.open(Context::Parameter { quoted }, &['$', '{']);
                i + 2
            }
        }
    }

    /// An unquoted `\x` becomes `'x'` (`\'` becomes `"'"`), a line
    /// continuation is removed.
    fn unquoted_escape(&mut self, i: usize) -> Option<usize> {
        match *self.chars.get(i + 1)? {
            '\n' => self.out.continuation = true,
            '\'' => {
                self.out.escape = true;
                self.push_quoted("\"'\"".chars());
                self.mark(false, false);
            }
            escaped => {
                self.out.escape = true;
                self.push_quoted(['\'', escaped, '\'']);
                self.mark(false, false);
            }
        }
        Some(i + 2)
    }

    /// Inside double quotes a backslash-newline is removed (line
    /// continuation), any other backslash pair is copied unchanged.
    fn quoted_escape(&mut self, i: usize, escaped: char) -> Option<usize> {
        if escaped == '\n' {
            self.out.continuation = true;
        } else {
            self.push('\\');
            self.push(escaped);
        }
        Some(i + 2)
    }

    fn single_quoted(&mut self, i: usize) -> Option<usize> {
        let end = find_char(self.chars, i + 1, '\'')?;
        self.push_quoted(['\'']);
        let content = self.len;
        self.push_quoted(self.chars[i + 1..end].iter().copied());
        self.note_single_quoted(content);
        self.push_quoted(['\'']);
        self.mark(false, false);
        Some(end + 1)
    }

    /// `$'...'` at `i`: inside, `\` escapes the next character, so `\'` does
    /// not end the string. It is copied as is, except that `\'` becomes the
    /// equivalent `\x27`, so a scanner that ends single quotes at the next
    /// `'` finds the same end. Returns the index after the closing quote.
    fn ansi_c(&mut self, start: usize) -> Option<usize> {
        self.push_quoted("$'".chars());
        let content = self.len;
        let mut i = start + 2;
        loop {
            match *self.chars.get(i)? {
                '\'' => break,
                '\\' => {
                    match *self.chars.get(i + 1)? {
                        '\'' => self.push_quoted("\\x27".chars()),
                        escaped => self.push_quoted(['\\', escaped]),
                    }
                    i += 2;
                }
                other => {
                    self.push_quoted([other]);
                    i += 1;
                }
            }
        }
        self.note_single_quoted(content);
        self.push_quoted(['\'']);
        self.mark(false, false);
        Some(i + 1)
    }

    /// `` `...` `` at `start`: copied unchanged; a backslash inside is
    /// reported, since `` \` `` nests a substitution. Returns the index after
    /// the closing backtick.
    fn backticks(&mut self, start: usize) -> Option<usize> {
        let whole = self.len;
        self.push_quoted(['`']);
        let mut i = start + 1;
        loop {
            match *self.chars.get(i)? {
                '`' => break,
                '\\' => {
                    self.out.escape_in_backticks = true;
                    self.push_quoted(['\\', *self.chars.get(i + 1)?]);
                    i += 2;
                }
                other => {
                    self.push_quoted([other]);
                    i += 1;
                }
            }
        }
        let inner = whole + 1..self.len;
        self.push_quoted(['`']);
        if !self.in_substitution() {
            self.out.substitutions.push(Substitution {
                whole: whole..self.len,
                inner,
            });
        }
        self.mark(false, false);
        Some(i + 1)
    }

    /// A comment at `i`. Returns the index after what was read.
    fn comment(&mut self, i: usize) -> usize {
        self.out.comment = true;
        match self.mode {
            CommentMode::Strip => find_char(self.chars, i, '\n').unwrap_or(self.chars.len()),
            CommentMode::AsCode => {
                self.push_quoted("'#'".chars());
                self.mark(false, false);
                i + 1
            }
        }
    }

    fn open(&mut self, context: Context, opener: &[char]) {
        self.stack.push(context);
        for &c in opener {
            self.push(c);
        }
    }

    fn open_list(&mut self, opener: &[char], closer: Closer) {
        let nested = self.in_substitution();
        let whole = self.len;
        self.stack.push(Context::Commands(List::new(closer)));
        for &c in opener {
            self.push(c);
        }
        if !nested {
            let inner = self.len;
            self.update(|list| list.start = Some((whole, inner)));
        }
    }

    /// Ends the innermost context with `closer`; the word it is part of
    /// goes on.
    fn close(&mut self, closer: char) {
        let inner_end = self.len;
        self.push(closer);
        if let Some(Context::Commands(List {
            start: Some((whole, inner)),
            ..
        })) = self.stack.pop()
        {
            self.out.substitutions.push(Substitution {
                whole: whole..self.len,
                inner: inner..inner_end,
            });
        }
        self.mark(false, false);
    }

    fn in_substitution(&self) -> bool {
        self.stack
            .iter()
            .filter(|context| matches!(context, Context::Commands(_)))
            .nth(1)
            .is_some()
    }

    fn note_single_quoted(&mut self, content: usize) {
        if !self.in_substitution() {
            self.out.single_quoted.push(content..self.len);
        }
    }

    /// Updates the command list on top of the stack, if there is one.
    fn update(&mut self, change: impl FnOnce(&mut List)) {
        if let Some(Context::Commands(list)) = self.stack.last_mut() {
            change(list);
        }
    }

    fn mark(&mut self, word_start: bool, command_start: bool) {
        self.update(|list| {
            list.word_start = word_start;
            list.command_start = command_start;
        });
    }

    fn push(&mut self, c: char) {
        let top_level = self.stack.len() == 1;
        self.emit(c, top_level);
    }

    fn push_quoted(&mut self, text: impl IntoIterator<Item = char>) {
        for c in text {
            self.emit(c, false);
        }
    }

    fn emit(&mut self, c: char, top_level: bool) {
        self.out.canonical.push(c);
        self.out.top_level.push(top_level);
        self.len += 1;
    }
}

/// Index of the first `target` at or after `from`.
fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    chars
        .get(from..)?
        .iter()
        .position(|&c| c == target)
        .map(|offset| from + offset)
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
/// view are added (when that view lexes), so a misread comment can only add
/// segments to evaluate, never hide one. `None` if the text cannot be lexed.
pub(super) fn lexed_segments(cmd: &str) -> Option<Vec<String>> {
    let lexed = lex(cmd, CommentMode::Strip)?;
    let mut segments = scan_top_level_segments(&lexed);
    if lexed.comment {
        let extra = lex(cmd, CommentMode::AsCode)
            .map(|view| scan_top_level_segments(&view))
            .unwrap_or_default();
        for segment in extra {
            if !segments.contains(&segment) {
                segments.push(segment);
            }
        }
    }
    Some(segments)
}

/// Zerlegt den kanonischen Text an den Top-Level-Operatoren `&&`, `||`,
/// `;`, `|`, `&` sowie Zeilenumbrüchen (`\n`, `\r`, `\r\n`), ohne in Quotes,
/// Expansionen oder Substitutionen hineinzuspalten ([`Lexed::top_level`]).
pub(super) fn scan_top_level_segments(lexed: &Lexed) -> Vec<String> {
    let chars: Vec<char> = lexed.canonical.chars().collect();
    let mut segments = Vec::new();
    let mut current_start = 0usize;
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        if !lexed.top_level[i] {
            i += 1;
            continue;
        }
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
        i += 1;
    }

    push_segment(&chars, current_start, chars.len(), &mut segments);
    segments
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
}
