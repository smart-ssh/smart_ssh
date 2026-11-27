//! Removal of terminal escape sequences before redaction (issue #1605,
//! Spec 0068 Part 1).
//!
//! Colour and cursor codes between a key and its value, inside a PEM header
//! or in front of a token break every redaction pattern. [`strip`] removes
//! them first, in one place, so every caller of the redactor sees stripped
//! text. Removed: CSI (`ESC [` … final byte `0x40–0x7E`, also the 8-bit
//! `0x9B` form), string sequences (OSC `ESC ]`, DCS, SOS, PM, APC up to
//! `BEL` or `ESC \`), other two- and three-byte `ESC` sequences and a lone
//! `ESC`. SGR (`m`) and erase (`K`) leave no trace; every other sequence
//! (and a lone `ESC`/`0x9B`) becomes one space. A sequence never swallows a line break: it ends in front of `\n`
//! or `\r`, so lines stay line-based for the live redactor. An unterminated
//! sequence is removed up to the end of the text.

/// Decodes `data` as UTF-8 (invalid bytes become U+FFFD, as before) and
/// removes terminal escape sequences. Text without any escape introducer is
/// returned unchanged.
#[cfg(test)]
pub(crate) fn strip(data: &[u8]) -> String {
    strip_text(decode(data))
}

/// Lossy UTF-8 decoding without removing any escape sequence.
pub(crate) fn decode(data: &[u8]) -> String {
    let mut text = String::with_capacity(data.len());
    for chunk in data.utf8_chunks() {
        text.push_str(chunk.valid());
        for b in chunk.invalid() {
            // A raw 0x9B is the 8-bit CSI introducer; it cannot be part of
            // valid UTF-8 here. Keep it distinguishable from U+FFFD.
            text.push(if *b == 0x9B { '\u{9B}' } else { '\u{FFFD}' });
        }
    }
    text
}

/// Removes terminal escape sequences from already decoded text.
#[cfg(test)]
pub(crate) fn strip_text(text: String) -> String {
    if !text.contains(['\u{1B}', '\u{9B}']) {
        return text;
    }
    strip_str(&text, ' ')
}

/// Placeholder for a removed non-SGR sequence while the patterns run: not a
/// word character and not whitespace, so a secret value is not cut short by
/// an escape inside it (private-use code point).
pub(crate) const SEPARATOR: char = '\u{E000}';

/// Like [`strip_text`], but removed non-SGR sequences leave [`SEPARATOR`];
/// the caller turns leftovers into spaces after redaction.
pub(crate) fn strip_text_for_matching(text: String) -> String {
    if !text.contains(['\u{1B}', '\u{9B}']) {
        return text;
    }
    strip_str(&text, SEPARATOR)
}

fn strip_str(text: &str, sep: char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            // `0x9B` followed by a parameter byte is a CSI; a bare one is
            // only the introducer, so a secret's first character is never
            // taken for the final byte.
            '\u{9B}' => {
                if matches!(it.peek(), Some(n) if ('\u{20}'..='\u{3F}').contains(n)) {
                    push_unless_silent(&mut out, sep, skip_csi(&mut it));
                } else {
                    out.push(sep);
                }
            }
            '\u{1B}' => match it.peek().copied() {
                Some('[') => {
                    it.next();
                    push_unless_silent(&mut out, sep, skip_csi(&mut it));
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    it.next();
                    skip_string(&mut it);
                    out.push(sep);
                }
                // `-` and `.` stay out (a PEM header starts with dashes).
                Some(' ' | '#' | '%' | '(' | ')' | '*' | '+') => {
                    // Intermediates (`ESC ( B`), then one final byte.
                    while matches!(it.peek(), Some(' ' | '#' | '%' | '(' | ')' | '*' | '+')) {
                        it.next();
                    }
                    if matches!(it.peek(), Some(n) if ('\u{30}'..='\u{7E}').contains(n)) {
                        it.next();
                    }
                    out.push(sep);
                }
                // Lone ESC (end of text, line break, printable character):
                // becomes a space and never eats the character after it,
                // which may be the first character of a secret.
                _ => out.push(sep),
            },
            _ => out.push(c),
        }
    }
    out
}

/// Only SGR (`m`) and erase (`K`) leave no trace; every other sequence
/// becomes one space so that it still separates a key from its value.
fn push_unless_silent(out: &mut String, sep: char, silent: bool) {
    if !silent {
        out.push(sep);
    }
}

/// CSI body: parameter/intermediate bytes, then a final byte. Any other
/// character (a line break, say) ends the sequence without being consumed.
/// Returns true for an SGR/erase sequence (final byte `m` or `K`).
fn skip_csi(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    while let Some(&c) = it.peek() {
        match c {
            '\u{20}'..='\u{3F}' => {
                it.next();
            }
            '\u{40}'..='\u{7E}' => {
                it.next();
                return matches!(c, 'm' | 'K');
            }
            _ => return false,
        }
    }
    false
}

/// String sequence body: up to `BEL` or `ESC \` (both consumed). A line
/// break ends it unconsumed; so does an `ESC` not followed by `\`.
fn skip_string(it: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&c) = it.peek() {
        match c {
            '\u{7}' => {
                it.next();
                return;
            }
            '\u{1B}' => {
                it.next();
                if it.peek() == Some(&'\\') {
                    it.next();
                }
                return;
            }
            '\n' | '\r' => return,
            _ => {
                it.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::strip;

    #[test]
    fn test_sgr_and_erase_are_removed() {
        assert_eq!(
            strip(b"\x1b[01;31m\x1b[Kpassword \x1b[m\x1b[K= hunter2"),
            "password = hunter2"
        );
    }

    #[test]
    fn test_osc_with_bel_and_st_is_removed() {
        assert_eq!(strip(b"a\x1b]0;title\x07b"), "a b");
        assert_eq!(strip(b"a\x1b]8;;http://x\x1b\\b"), "a b");
    }

    #[test]
    fn test_lone_esc_and_two_byte_sequences_are_removed() {
        assert_eq!(strip(b"a\x1bbcd"), "a bcd");
        assert_eq!(strip(b"a\x1b"), "a ");
        assert_eq!(strip(b"a\x1b\nb"), "a \nb");
        assert_eq!(strip(b"a\x1b(Bb"), "a b");
        assert_eq!(strip("x\u{9b}gsk".as_bytes()), "x gsk");
        assert_eq!(strip(b"x\x1b[2~gsk"), "x gsk");
        assert_eq!(strip(b"x\x1b[@eyJ"), "x eyJ");
    }

    #[test]
    fn test_unterminated_sequences_are_removed_to_the_end() {
        assert_eq!(strip(b"abc\x1b["), "abc ");
        assert_eq!(strip(b"abc\x1b[31"), "abc ");
        assert_eq!(strip(b"abc\x1b]0;title"), "abc ");
    }

    #[test]
    fn test_unterminated_sequence_keeps_the_line_break() {
        assert_eq!(strip(b"a\x1b[3\nsecret"), "a \nsecret");
        assert_eq!(strip(b"a\x1b]0;t\r\nb"), "a \r\nb");
    }

    #[test]
    fn test_c1_csi_is_removed_raw_and_encoded() {
        assert_eq!(strip(b"a\x9b31mb"), "ab");
        assert_eq!(strip("a\u{9b}31mb".as_bytes()), "ab");
    }

    #[test]
    fn test_plain_text_is_unchanged() {
        let s = "grüße ě (0x9B continuation) [31m 日本\n\r\t";
        assert_eq!(strip(s.as_bytes()), s);
        assert_eq!(strip(b"a\xffb"), "a\u{FFFD}b");
    }

    // ---- Redaction of coloured output (issue #1605) ----

    use crate::ai::redactor_generators::secret_snippet;
    use crate::ai::redactor_reference::OutputRedactor as ReferenceTrait;
    use crate::ai::{DefaultOutputRedactor, OutputRedactor};
    use proptest::prelude::*;

    static REDACTOR: std::sync::LazyLock<DefaultOutputRedactor> =
        std::sync::LazyLock::new(DefaultOutputRedactor::new);

    fn red(text: &str) -> String {
        REDACTOR.redact_text(text)
    }

    static REFERENCE: std::sync::LazyLock<crate::ai::redactor_reference::DefaultOutputRedactor> =
        std::sync::LazyLock::new(crate::ai::redactor_reference::DefaultOutputRedactor::new);

    #[test]
    fn test_non_sgr_escapes_before_secrets_do_not_leak() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnopqrstuvwxyz";
        let gsk = "gsk_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
        let inputs = [
            format!("x\x1b{gsk}"),
            format!("x\u{9b}{gsk}"),
            format!("x\x1b[2~{gsk}"),
            format!("x\x1b]0;t\x07{gsk}"),
            format!("x\x1b]8;;http://e\x1b\\{gsk}"),
            format!("x\x1b[@{jwt}"),
            "my\x1b[2~api-key: supersecret123".to_string(),
        ];
        let expected = [
            "x [REDACTED]",
            "x [REDACTED]",
            "x [REDACTED]",
            "x [REDACTED]",
            "x [REDACTED]",
            "x [REDACTED]",
            "my [REDACTED]",
        ];
        for (input, want) in inputs.iter().zip(expected) {
            assert_eq!(red(input), want, "{input:?}");
        }
    }

    #[test]
    fn test_non_sgr_escape_inside_secret_value_does_not_cut_it() {
        for (input, want) in [
            ("password=hun\x1b[2~ter2secret", "[REDACTED]"),
            ("token: abcd\x1b]0;t\x07efghsecretvalue", "[REDACTED]"),
            (
                "https://user:\x1b[2~hunter2pass@host/x",
                "https://user:[REDACTED]@host/x",
            ),
        ] {
            assert_eq!(red(input), want, "{input:?}");
        }
    }

    /// Any escape sequence (not only SGR) around or in front of a secret.
    fn any_escape() -> impl Strategy<Value = &'static str> {
        proptest::sample::select(vec![
            "\x1b",
            "\u{9b}",
            "\x1b[2~",
            "\x1b[@",
            "\x1b[1;5H",
            "\x1b]0;t\x07",
            "\x1b]8;;http://e\x1b\\",
            "\x1bP1$r\x1b\\",
            "\x1b(B",
            "\x1b[0m",
            "\x1b[K",
        ])
    }

    #[test]
    fn test_introducer_before_secret_keeps_placeholder_and_absorbs_tail() {
        assert_eq!(
            red("id \x1bAKIAIOSFODNN7EXAMPLEtailsecret99 y"),
            "id  [REDACTED] y"
        );
        assert_eq!(
            red("id \u{9b}AKIAIOSFODNN7EXAMPLEtailsecret99 y"),
            "id  [REDACTED] y"
        );
    }

    #[test]
    fn test_sgr_around_secret_leaves_exact_placeholder() {
        assert_eq!(
            red("x \x1b[31mAKIAIOSFODNN7EXAMPLE\x1b[0m y"),
            "x [REDACTED] y"
        );
    }

    #[test]
    fn test_sgr_coloured_tail_is_absorbed() {
        assert_eq!(
            red("k AKIAIOSFODNN7EXAMPLE\x1b[1mtailsecret99"),
            "k [REDACTED]"
        );
    }

    #[test]
    fn test_grep_color_password_value_is_masked() {
        let out = red("\x1b[01;31m\x1b[Kpassword \x1b[m\x1b[K= hunter2\n");
        assert!(!out.contains("hunter2"), "{out:?}");
        assert!(!out.contains('\u{1b}'), "{out:?}");
    }

    #[test]
    fn test_pem_header_with_sgr_masks_the_body() {
        let raw = "\x1b[31m-----BEGIN\x1b[0m \x1b[1mRSA PRIVATE KEY\x1b[0m-----\n\
                   MIIEowIBAAKCAQEAsecretbodyline1\nsecretbodyline2AAAA\n\
                   -----END RSA PRIVATE KEY-----\n";
        let out = red(raw);
        assert!(!out.contains("secretbodyline"), "{out:?}");
        assert!(!out.contains("MIIEowIBAAKCAQEA"), "{out:?}");
    }

    #[test]
    fn test_token_split_by_sgr_is_masked() {
        let out = red("ghp_abcdefghijkl\x1b[0mmnopqrstuvwxyz0123456789");
        assert!(!out.contains("abcdefghijkl"), "{out:?}");
        assert!(!out.contains("mnopqrstuvwxyz"), "{out:?}");
    }

    #[test]
    fn test_secret_right_after_lone_esc_or_c1_csi_is_masked() {
        let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        for lead in ["\x1b", "\u{9b}"] {
            let out = red(&format!("{lead}{token}"));
            assert!(!out.contains("abcdefghijkl"), "{lead:?}: {out:?}");
        }
        let out = red("\x1bAKIAIOSFODNN7EXAMPLE");
        assert!(!out.contains("IOSFODNN7EXAMPLE"), "{out:?}");
        let pem = "\x1b-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAsecretbody\n-----END RSA PRIVATE KEY-----";
        let out = red(pem);
        assert!(!out.contains("secretbody"), "{out:?}");
    }

    #[test]
    fn test_key_and_value_coloured_separately_is_masked() {
        let out = red("\x1b[36mpassword\x1b[0m=\x1b[33mhunter2secret\x1b[0m");
        assert!(!out.contains("hunter2secret"), "{out:?}");
    }

    #[test]
    fn test_redact_output_strips_stdout_and_stderr() {
        let o = crate::ssh::CommandOutput {
            stdout: b"\x1b[31mpassword\x1b[0m=hunter2".to_vec(),
            stderr: b"\x1b[31mpassword\x1b[0m=hunter3".to_vec(),
            exit_code: Some(0),
            truncated: false,
        };
        let r = DefaultOutputRedactor::new().redact(&o);
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&r.stdout),
            String::from_utf8_lossy(&r.stderr)
        );
        assert!(!all.contains("hunter"), "{all:?}");
        assert!(!all.contains('\u{1b}'), "{all:?}");
    }

    fn sgr() -> impl Strategy<Value = &'static str> {
        proptest::sample::select(vec![
            "\x1b[0m",
            "\x1b[01;31m",
            "\x1b[K",
            "\x1b[38;5;196m",
            "\x1b[m",
        ])
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        /// Arbitrary escapes in front of and behind a snippet never leave a
        /// secret the reference masks in the same input. (Inside a
        /// space-free token only SGR/erase is transparent; any other
        /// sequence becomes a space by design, see `sgr_wrapped_equals_plain`.)
        #[test]
        fn any_escape_never_leaks_what_reference_masks(
            snip in secret_snippet(), a in any_escape(), c in any_escape(),
        ) {
            // `ESC` followed by one of `] P X ^ _ [` is by definition the
            // start of a string/CSI sequence and swallows what follows.
            let starts_seq = |t: &str| t.starts_with([']', 'P', 'X', '^', '_', '[']);
            prop_assume!(!starts_seq(&snip.text));
            let mut text = snip.text.clone();
            for secret in &snip.secrets {
                text = text.replace(secret.as_str(), &format!("{a}{secret}{c}"));
            }
            let text = format!("{a}{text}{c}");
            let reference = ReferenceTrait::redact_text(&*REFERENCE, &text);
            let out = red(&text);
            for secret in snip.secrets.iter().filter(|s| !reference.contains(s.as_str())) {
                prop_assert!(!out.contains(secret.as_str()), "{secret:?} in {out:?} (input {text:?})");
            }
        }

        /// SGR-wrapped secret redacts exactly like the plain text.
        #[test]
        fn sgr_wrapped_equals_plain(snip in secret_snippet(), a in sgr(), c in sgr()) {
            let mut wrapped = snip.text.clone();
            for secret in &snip.secrets {
                wrapped = wrapped.replace(secret.as_str(), &format!("{a}{secret}{c}"));
            }
            prop_assert_eq!(red(&wrapped), red(&snip.text), "input {:?}", wrapped);
        }

        /// Random SGR around key, separator and value never lets a secret
        /// through.
        #[test]
        fn coloured_secret_is_still_redacted(
            snip in secret_snippet(),
            a in sgr(), b in sgr(), c in sgr(),
        ) {
            let mut text = String::new();
            for ch in snip.text.chars() {
                if matches!(ch, ' ' | '=' | ':' | '"' | '\'') {
                    text.push_str(a);
                    text.push(ch);
                    text.push_str(b);
                } else {
                    text.push(ch);
                }
            }
            for secret in &snip.secrets {
                text = text.replace(secret.as_str(), &format!("{a}{secret}{c}"));
            }
            // Only secrets the redactor masks in plain text count (some
            // low-entropy values are not masked there either).
            let plain = red(&snip.text);
            let out = red(&text);
            for secret in snip.secrets.iter().filter(|s| !plain.contains(s.as_str())) {
                prop_assert!(!out.contains(secret.as_str()), "{secret:?} in {out:?} (input {text:?})");
            }
        }
    }
}
