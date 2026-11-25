# ADR 0162 — Filter parser: shell lexing of escapes, comments and line continuations

## Status

Accepted

Betrifft: Spec 0002 (Abschnitt 4.4, 4.5), ADR 0107, ADR 0001

## Problem

The segment scanner of the filter parser tracked single quotes, double
quotes, backticks and `$(...)`, but not three things a shell reads before it
splits a command:

- an unquoted backslash escapes the next character (`\'` is a literal
  quote, not the start of a quoted string);
- a `#` at the start of a word, outside quotes, starts a comment up to the
  end of the line;
- a backslash-newline is removed (line continuation), also inside double
  quotes.

So the scanner could see other commands than the shell runs. Under
`Allow "*"` (or `Allow "echo *"`) plus `Deny "rm *"` these were `AutoExec`,
while bash runs the hidden command; neither the deny rule nor the hard
blacklist saw it:

- `echo \' ; rm -rf / ; echo \'` (escaped quote opens a fake quoted region)
- `echo a #'<LF>rm -rf /<LF>#'` (quote inside a comment)
- `echo \" > /tmp/o; echo RAN; echo "\""` (hidden redirection and command)
- `r\<LF>m -rf /` (line continuation splits the program name)

The helpers that run on each segment afterwards (output-redirection check,
substitution stripping, first-word unquoting, rule matching) share the same
blind spot.

Substitutions had a similar gap. A double-quoted string ended at the next
`"` even inside `"$(...)"`, the substitution stripping ended `$(` at the
next `)` without regard to quotes or comments, and bash 5.3's `${ cmd; }`
was taken for a parameter expansion. `echo "${ rm -rf /; }"` was `AutoExec`
under `Allow "*"` plus `Deny "rm *"`; `echo "$(echo ")"; rm -rf /)"` and
`echo "$(#'<LF>rm -rf /<LF>)"` were only `Confirm`, the deny rule did not
see the inner `rm`.

## Decision

1. **One lexer in front of the scanner** (`crates/core/src/filter/shell_lexer.rs`).
   `split_command` lexes the command first and splits the resulting
   canonical text; every consumer of the split (filter engine, risk
   classifier, rule suggestions, the structure check of prepared commands,
   here-string extraction) gets the same view. The canonical text is read by
   the shell like the input but contains none of the three constructs:
   - unquoted `\x` becomes `'x'` (`\'` becomes `"'"`), which the existing
     quote-aware helpers already understand;
   - a line continuation is removed, unquoted and inside double quotes
     (inside single quotes it stays literal; inside a comment it is part of
     the comment, as in bash);
   - a comment is removed up to, not including, its newline;
   - in `$'...'` a `\'` does not end the string; it is rewritten to the
     equivalent `\x27` so later steps find the same end.

   Unterminated quotes, `$'...'` strings or backticks and a trailing lone
   backslash make the command not safely analysable (`Confirm`), as
   unbalanced quotes did before.

2. **Where a comment starts.** Only at a word start: at the beginning, after
   a blank (space, tab, newline), after `;`, `&`, `|`, `<`, `>`, after an
   opening `(` / `$(` / `<(` / `>(` and after a `)` that closes a subshell,
   a `case` pattern or `f()` (bash runs the next line of
   `(true)#'<LF>rm -rf /<LF>#'`). Never inside quotes, inside `${...}` (bash
   reads `${x:- #}` as a parameter expansion), inside a word (`a#b`, `$#`,
   `a\ #b`), after the `)` of a substitution (`$(ls)#x` is one word) or
   after `\r`. Where in doubt the `#` is read as a comment: the second view
   below still evaluates its text, and a comment needs confirmation. Read
   the other way, a real comment taken for code can let a quote in it hide
   the next line.

3. **A misread comment cannot hide a segment.** When the lexer found a
   comment, the command is lexed a second time with the comment kept as
   quoted text (`'#'`), and the segments of that view are added to the
   result when it can be split. If the lexer took something for a comment
   that the shell runs, those commands are still evaluated, so a deny rule
   or the hard blacklist still applies.

   The text of a parameter expansion gets the same second view. In
   `${...}` bash runs nothing outside a substitution (`${x:-a;rm -rf /}`
   holds a literal word, `${a;rm -rf /;}` is a "bad substitution"), so the
   lexer reads it as one word; read only that way, chaining could lower the
   risk rating (`${a` + `;` + `rm;}` gave `${a;rm;}`, rated lower than
   `rm;}`), and a `${` the lexer takes for an expansion could hide a command.
   So the text of a parameter expansion at the top level is also split as
   code, and those segments are added: `echo ${a;rm -rf /;}` is `Deny`
   under `Deny "rm *"`. More segments can only raise a decision or a risk
   rating. A valid expansion with an operator in its word usually needs
   confirmation then (`echo ${x:-a;b}` adds the segment `b}`, which is no
   plain command); such words are rare in generated commands.

4. **Fail closed only where the lexer cannot make the text exact.** A line
   continuation, a comment or a backslash inside backticks raises the
   decision to at least `Confirm` with `FILTER_PARSE_AMBIGUOUS` (via
   `opaque_encoding_reason`, the same place as the Unicode and ANSI-C checks
   of ADR 0107). As in ADR 0107 this only escalates: the regular evaluation
   of the canonical text still runs, so a `Deny` behind the construct wins.
   - A continuation or a comment changes which lines and words the user
     sees compared with what runs (`r\<LF>m`, a quote inside a comment), and
     the comment boundary rests on the word-start rule above; both are rare
     in AI-proposed commands.
   - A backslash inside backticks is not resolved (`` \` `` nests a
     substitution the scanner does not follow); it only triggers the
     confirmation.
   - A `${` that starts neither a function substitution nor a parameter
     expansion (after an optional `#` or `!`, a name, a positional or a
     special parameter, followed by `}`, a subscript or an operator), such
     as `${a;b}`, `${a|b}`, `${a b}` or `${}`, is a bad substitution in
     bash. Other shells read some of these forms differently, so it also
     needs confirmation.

   **Plain backslash escapes do not raise the decision.** An unquoted `\x`
   outside backticks is resolved exactly into quotes (`'x'`), and the
   canonical text is evaluated against the rules like any quoted input:
   deny rules and the hard blacklist see the command behind it
   (`echo \' ; rm -rf / ; echo \'` is `Deny` under `Deny "rm *"`, `\rm` and
   `r\m` are `rm`), and an Allow rule matches as before. So `echo a\ b`,
   `ls my\ dir`, `find . -exec … {} \;` and the single-quote nesting idiom
   `'it'\''s'` stay `AutoExec` under a matching Allow rule;
   `echo \; rm -rf /` is an `echo` (bash prints the text) and is decided as
   one. A blanket confirmation for every escape was considered and
   rejected: these forms are common in generated commands, and the escape
   itself no longer changes what the engine sees.

   Commands without any of these constructs are lexed to the same text and
   keep their decision. `echo a # comment` needs confirmation under an
   Allow rule.

5. **Substitutions are command lists of their own.** The lexer follows the
   nesting the shell reads instead of counting quotes and parentheses: inside
   `$(...)`, `<(...)`, `>(...)` and bash 5.3's function substitutions
   `${ ...; }` / `${| ...; }` it reads a command list with its own quotes,
   comments, escapes, subshells and `{ ...; }` groups, also when the
   substitution stands in double quotes or in a parameter expansion
   (`"${x:-"a"}"`, where quotes nest as well). A `case` statement is read as
   head, pattern lists and clauses, since its `)` must not end a
   substitution: in a pattern `(`, `|`, `{`, `}` and `case` are words, and
   `esac` ends the statement only at the start of a pattern list or in
   command position. Reserved words count only in command position (also
   after `time -p --`, `coproc NAME` before a compound command, `function
   NAME` and any `)` closing a group, as in `f ( )` or `for ((...))`), and
   only space, tab and newline delimit them (`case<CR>` is a plain word).
   Where the lexer still misreads a command position inside a substitution,
   the result can only fall from `Deny` to `Confirm`: every substitution
   needs confirmation. Extended glob patterns (`@(a|b)`) in a `case` pattern
   are not followed; bash accepts them there only with `extglob` already on
   when the line is parsed. It reports which
   chars are at the top level and where each substitution starts and ends.
   The segment scanner splits only at top-level operators, and
   `strip_substitutions` takes the substitution bounds from the same lexer,
   so `"$(echo ")"; rm -rf /)"` and `"$(#'<LF>rm -rf /<LF>)"` yield the inner
   `rm -rf /`, and `"${ rm -rf /; }"` (before: a parameter expansion, so
   `AutoExec` under `Allow "*"`) is a substitution: at least `Confirm`, and
   `Deny` under `Deny "rm *"`. The inner command is taken from the segment
   as split, before whitespace is normalised, so a newline in it still
   separates commands (`$(true<LF>rm -rf /)`). Substitutions inside single
   quotes are still extracted as before (the shell does not run them; in
   doubt more is checked). A function substitution ends at a `}` in command
   position, as in bash: `${ echo a }; ls; }` holds `echo a }; ls;`.

6. **Risk classification changes with it.** ADR 0107 kept its new checks out
   of `split_command` so that the risk segmentation stayed the same. This
   decision changes `split_command` on purpose: the risk classifier must see
   the hidden `rm -rf /` too, and for input without the three constructs the
   segmentation is unchanged.

## Testing

`crates/core/src/filter/adversarial_tests/shell_lexing.rs` holds the four inputs above,
the substitution and `)#` cases of decisions 2 and 5 and further variants;
they failed before this change and pass after it. A differential test runs
bash (skipped where none is installed) with a `DEBUG` trap under `extdebug`
that prints every simple command and skips all but `echo`, `printf`, `true`,
`case` and assignments (so substitutions in them are expanded), and checks
that each command word bash would run is a command the parser sees, at the
top level or in a substitution. Its function-substitution cases run only
with a bash that has them (5.3); the lexer's own unit tests in
`shell_lexer/tests.rs` cover them independently of the installed bash.
Cases the risk property tests found, such as `${a;rm;}`, are fixed tests
in `crates/core/src/risk/tests_chaining.rs`.
