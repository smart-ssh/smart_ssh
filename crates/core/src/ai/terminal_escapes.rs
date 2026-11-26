//! Removal of terminal escape sequences before redaction (issue #1605,
//! Spec 0068 Part 1).
//!
//! Colour and cursor codes between a key and its value, inside a PEM header
//! or in front of a token break every redaction pattern. [`strip`] removes
//! them first, in one place, so every caller of the redactor sees stripped
//! text. Removed: CSI (`ESC [` … final byte `0x40–0x7E`, also the 8-bit
//! `0x9B` form), string sequences (OSC `ESC ]`, DCS, SOS, PM, APC up to
//! `BEL` or `ESC \`), other two- and three-byte `ESC` sequences and a lone
//! `ESC`. A sequence never swallows a line break: it ends in front of `\n`
//! or `\r`, so lines stay line-based for the live redactor. An unterminated
//! sequence is removed up to the end of the text.

/// Decodes `data` as UTF-8 (invalid bytes become U+FFFD, as before) and
/// removes terminal escape sequences. Text without any escape introducer is
/// returned unchanged.
pub(crate) fn strip(data: &[u8]) -> String {
    let mut text = String::with_capacity(data.len());
    for chunk in data.utf8_chunks() {
        text.push_str(chunk.valid());
        for b in chunk.invalid() {
            // A raw 0x9B is the 8-bit CSI introducer; it cannot be part of
            // valid UTF-8 here. Keep it distinguishable from U+FFFD.
            text.push(if *b == 0x9B { '\u{9B}' } else { '\u{FFFD}' });
        }
    }
    if !text.contains(['\u{1B}', '\u{9B}']) {
        return text;
    }
    strip_str(&text)
}

fn strip_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\u{9B}' => skip_csi(&mut it),
            '\u{1B}' => match it.peek().copied() {
                Some('[') => {
                    it.next();
                    skip_csi(&mut it);
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    it.next();
                    skip_string(&mut it);
                }
                Some(n) if ('\u{20}'..='\u{2F}').contains(&n) => {
                    // Intermediates (`ESC ( B`), then one final byte.
                    while matches!(it.peek(), Some(n) if ('\u{20}'..='\u{2F}').contains(n)) {
                        it.next();
                    }
                    if matches!(it.peek(), Some(n) if ('\u{30}'..='\u{7E}').contains(n)) {
                        it.next();
                    }
                }
                Some(n) if ('\u{30}'..='\u{7E}').contains(&n) => {
                    it.next();
                }
                // Lone ESC (end of text, line break, other byte).
                _ => {}
            },
            _ => out.push(c),
        }
    }
    out
}

/// CSI body: parameter/intermediate bytes, then a final byte. Any other
/// character (a line break, say) ends the sequence without being consumed.
fn skip_csi(it: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&c) = it.peek() {
        match c {
            '\u{20}'..='\u{3F}' => {
                it.next();
            }
            '\u{40}'..='\u{7E}' => {
                it.next();
                return;
            }
            _ => return,
        }
    }
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
        assert_eq!(strip(b"a\x1b]0;title\x07b"), "ab");
        assert_eq!(strip(b"a\x1b]8;;http://x\x1b\\b"), "ab");
    }

    #[test]
    fn test_lone_esc_and_two_byte_sequences_are_removed() {
        assert_eq!(strip(b"a\x1bb"), "a");
        assert_eq!(strip(b"a\x1b"), "a");
        assert_eq!(strip(b"a\x1b\nb"), "a\nb");
        assert_eq!(strip(b"a\x1b(Bb"), "ab");
    }

    #[test]
    fn test_unterminated_sequences_are_removed_to_the_end() {
        assert_eq!(strip(b"abc\x1b["), "abc");
        assert_eq!(strip(b"abc\x1b[31"), "abc");
        assert_eq!(strip(b"abc\x1b]0;title"), "abc");
    }

    #[test]
    fn test_unterminated_sequence_keeps_the_line_break() {
        assert_eq!(strip(b"a\x1b[3\nsecret"), "a\nsecret");
        assert_eq!(strip(b"a\x1b]0;t\r\nb"), "a\r\nb");
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
    use crate::ai::{DefaultOutputRedactor, OutputRedactor};
    use proptest::prelude::*;

    fn red(text: &str) -> String {
        DefaultOutputRedactor::new().redact_text(text)
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
    fn test_token_right_after_sgr_is_masked() {
        let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let out = red(&format!("\x1b[32m{token}\x1b[0m"));
        assert!(!out.contains(token), "{out:?}");
        assert!(
            !out.contains("abcdefghijklmnopqrstuvwxyz0123456789"),
            "{out:?}"
        );
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
