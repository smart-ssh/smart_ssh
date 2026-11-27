//! Removal of terminal escape sequences before redaction (issue #1605,
//! Spec 0068 Part 1).
//!
//! Colour and cursor codes between a key and its value, inside a PEM header
//! or in front of a token break every redaction pattern. The redactor masks
//! the union of the patterns on the raw text and on the text with the
//! sequences removed ([`redact_escaped`]); the output never carries an
//! escape sequence. Removed: CSI (`ESC [` … final byte `0x40–0x7E`, also the 8-bit
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
    let text = decode(data);
    if !has_introducer(&text) {
        return text;
    }
    strip_tracked(&text).text
}

/// Lossy UTF-8 decoding without removing any escape sequence.
pub(crate) fn decode(data: &[u8]) -> String {
    let mut text = String::with_capacity(data.len());
    for chunk in data.utf8_chunks() {
        text.push_str(chunk.valid());
        let invalid = chunk.invalid();
        if invalid.is_empty() {
            continue;
        }
        // One U+FFFD per invalid sequence, as `from_utf8_lossy`. A raw 0x9B
        // is the 8-bit CSI introducer (a lone continuation byte, so its own
        // chunk); keep it distinguishable from U+FFFD.
        text.push(if invalid == [0x9B] {
            '\u{9B}'
        } else {
            '\u{FFFD}'
        });
    }
    text
}

/// True if `text` contains an escape introducer (`ESC` or `0x9B`). Text
/// without one takes the plain redaction path, byte for byte as before.
pub(crate) fn has_introducer(text: &str) -> bool {
    text.contains(['\u{1B}', '\u{9B}'])
}

/// Redaction rules: pattern and replacement (as for `Regex::replace_all`).
pub(crate) type Rules<'a> = [(&'a regex::Regex, &'a str)];

/// Decodes `data` and applies the rules in order. Without an escape
/// introducer that is exactly the plain patterns, byte for byte; otherwise
/// see [`redact_escaped`].
pub(crate) fn redact_decoded<'a>(
    data: &[u8],
    rules: impl Iterator<Item = (&'a regex::Regex, &'a str)>,
    placeholder: &str,
) -> String {
    let mut text = decode(data);
    if has_introducer(&text) {
        let rules: Vec<_> = rules.collect();
        return redact_escaped(&text, &rules, placeholder);
    }
    for (re, replacement) in rules {
        if re.is_match(&text) {
            text = re.replace_all(&text, replacement).into_owned();
        }
    }
    text
}

/// Text after the rules ran, with the origin of every byte.
struct Tracked {
    text: String,
    /// Per byte of `text`: the byte range of the input it stems from (a
    /// replacement carries the range of the whole match).
    prov: Vec<(usize, usize)>,
    /// Per byte of `text`: produced by a replacement.
    replaced: Vec<bool>,
}

/// Applies the rules in order, exactly like repeated `replace_all`, and
/// records where every output byte comes from.
fn track(input: &str, rules: &Rules<'_>) -> Tracked {
    let mut t = Tracked {
        text: input.to_string(),
        prov: (0..input.len()).map(|i| (i, i + 1)).collect(),
        replaced: vec![false; input.len()],
    };
    for (re, replacement) in rules {
        if !re.is_match(&t.text) {
            continue;
        }
        let mut text = String::with_capacity(t.text.len());
        let mut prov = Vec::with_capacity(t.prov.len());
        let mut replaced = Vec::with_capacity(t.replaced.len());
        let mut last = 0;
        for caps in re.captures_iter(&t.text) {
            let Some(m) = caps.get(0) else { continue };
            text.push_str(&t.text[last..m.start()]);
            prov.extend_from_slice(&t.prov[last..m.start()]);
            replaced.extend_from_slice(&t.replaced[last..m.start()]);
            let range = t.prov[m.start()..m.end()]
                .iter()
                .fold((usize::MAX, 0), |(lo, hi), &(l, h)| (lo.min(l), hi.max(h)));
            let range = if m.is_empty() { (0, 0) } else { range };
            let mut dst = String::new();
            caps.expand(replacement, &mut dst);
            prov.extend(std::iter::repeat_n(range, dst.len()));
            replaced.extend(std::iter::repeat_n(true, dst.len()));
            text.push_str(&dst);
            last = m.end();
        }
        text.push_str(&t.text[last..]);
        prov.extend_from_slice(&t.prov[last..]);
        replaced.extend_from_slice(&t.replaced[last..]);
        t = Tracked {
            text,
            prov,
            replaced,
        };
    }
    t
}

/// Redacts decoded text that contains an escape introducer (issue #1605).
///
/// The result masks the union of two views: (a) the patterns on the raw
/// text, as before escape handling existed, so nothing masked there can
/// leak; (b) the patterns on the stripped text, which finds secrets that an
/// escape sequence cut apart. Matches of (a) are mapped onto the stripped
/// text by offsets; a match that touches part of a removed sequence masks
/// the whole stripped span it touches. The output carries no escape
/// sequence.
pub(crate) fn redact_escaped(raw: &str, rules: &Rules<'_>, placeholder: &str) -> String {
    let raw_pass = track(raw, rules);
    let mut spans: Vec<(usize, usize)> = raw_pass
        .prov
        .iter()
        .zip(&raw_pass.replaced)
        .filter(|(_, r)| **r)
        .map(|(p, _)| *p)
        .filter(|(lo, hi)| lo < hi)
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (lo, hi) in spans {
        match merged.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }
    let touches = |(lo, hi): (usize, usize)| {
        let i = merged.partition_point(|s| s.1 <= lo);
        merged.get(i).is_some_and(|s| s.0 < hi)
    };

    let stripped = strip_tracked(raw);
    let masked: Vec<bool> = stripped.raw.iter().map(|r| touches(*r)).collect();
    let pass = track(&stripped.text, rules);

    let mut out = String::with_capacity(pass.text.len());
    let mut in_run = false;
    for (b, c) in pass.text.char_indices() {
        let extra = !pass.replaced[b] && masked[pass.prov[b].0];
        if extra {
            if !in_run && !out.ends_with(placeholder) {
                out.push_str(placeholder);
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// Stripped text with the raw byte range of every output byte.
struct Stripped {
    text: String,
    raw: Vec<(usize, usize)>,
}

type Cs = [(usize, char)];

fn strip_tracked(text: &str) -> Stripped {
    let cs: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |k: usize| cs.get(k).map_or(text.len(), |x| x.0);
    let mut out = Stripped {
        text: String::with_capacity(text.len()),
        raw: Vec::with_capacity(text.len()),
    };
    // A removed sequence becomes one space carrying its whole raw range.
    let sep = |out: &mut Stripped, start: usize, end: usize| {
        out.text.push(' ');
        out.raw.push((start, end));
    };
    let mut i = 0;
    while i < cs.len() {
        let (start, c) = cs[i];
        i += 1;
        match c {
            // `0x9B` followed by a parameter byte is a CSI; a bare one is
            // only the introducer, so a secret's first character is never
            // taken for the final byte.
            '\u{9B}' => {
                if matches!(cs.get(i), Some(&(_, n)) if ('\u{20}'..='\u{3F}').contains(&n)) {
                    if !skip_csi(&cs, &mut i) {
                        sep(&mut out, start, end_of(i));
                    }
                } else {
                    sep(&mut out, start, end_of(i));
                }
            }
            '\u{1B}' => match cs.get(i).map(|x| x.1) {
                Some('[') => {
                    i += 1;
                    if !skip_csi(&cs, &mut i) {
                        sep(&mut out, start, end_of(i));
                    }
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    i += 1;
                    skip_string(&cs, &mut i);
                    sep(&mut out, start, end_of(i));
                }
                // `-` and `.` stay out (a PEM header starts with dashes).
                Some(' ' | '#' | '%' | '(' | ')' | '*' | '+') => {
                    // Intermediates (`ESC ( B`), then one final byte.
                    while matches!(
                        cs.get(i),
                        Some(&(_, ' ' | '#' | '%' | '(' | ')' | '*' | '+'))
                    ) {
                        i += 1;
                    }
                    if matches!(cs.get(i), Some(&(_, n)) if ('\u{30}'..='\u{7E}').contains(&n)) {
                        i += 1;
                    }
                    sep(&mut out, start, end_of(i));
                }
                // Lone ESC (end of text, line break, printable character):
                // becomes a space and never eats the character after it,
                // which may be the first character of a secret.
                _ => sep(&mut out, start, end_of(i)),
            },
            _ => {
                out.text.push(c);
                let end = end_of(i);
                out.raw.extend((start..end).map(|b| (b, b + 1)));
            }
        }
    }
    out
}

/// CSI body: parameter/intermediate bytes, then a final byte. Any other
/// character (a line break, say) ends the sequence without being consumed.
/// Returns true for an SGR/erase sequence (final byte `m` or `K`), which
/// leaves no trace; every other sequence becomes one space so that it still
/// separates a key from its value.
fn skip_csi(cs: &Cs, i: &mut usize) -> bool {
    while let Some(&(_, c)) = cs.get(*i) {
        match c {
            '\u{20}'..='\u{3F}' => *i += 1,
            '\u{40}'..='\u{7E}' => {
                *i += 1;
                return matches!(c, 'm' | 'K');
            }
            _ => return false,
        }
    }
    false
}

/// String sequence body: up to `BEL` or `ESC \` (both consumed). A line
/// break ends it unconsumed; so does an `ESC` not followed by `\`.
fn skip_string(cs: &Cs, i: &mut usize) {
    while let Some(&(_, c)) = cs.get(*i) {
        match c {
            '\u{7}' => {
                *i += 1;
                return;
            }
            '\u{1B}' => {
                *i += 1;
                if cs.get(*i).map(|x| x.1) == Some('\\') {
                    *i += 1;
                }
                return;
            }
            '\n' | '\r' => return,
            _ => *i += 1,
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

    #[test]
    fn test_decode_matches_lossy_without_escape() {
        for data in [
            &b"abc\xE2\x82 def"[..],
            b"\xF0\x9F\x98",
            b"a\xffb\xC3",
            b"\xE2\x82\xE2\x82\xAC",
        ] {
            assert_eq!(
                super::decode(data),
                String::from_utf8_lossy(data),
                "{data:?}"
            );
        }
        assert_eq!(super::decode(b"x\x9by"), "x\u{9b}y");
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
                // The raw view masks the whole URL, so the union does too.
                "[REDACTED]",
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
            "\x1b ",
            "\x1b(",
            "\x1b[",
            "\x1b[31;",
            "\x1b[ ",
            "\u{9b} ",
        ])
    }

    #[test]
    fn test_incomplete_sequence_before_secret_leaks_nothing() {
        let tok = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let akia = "AKIAIOSFODNN7EXAMPLE";
        let cases: Vec<(String, &str)> = vec![
            (format!("x\x1b {tok}"), "[REDACTED]"),
            (format!("x\x1b({tok}"), "[REDACTED]"),
            (format!("x \x1b({akia}"), "x [REDACTED]"),
            (format!("x \x1b[{akia}"), "x [REDACTED]"),
            (format!("x\x1b[{tok}"), "[REDACTED]"),
            ("\x1b[31;password=hunter2secret".to_string(), "[REDACTED]"),
            ("\x1b[ password=hunter2secret".to_string(), "[REDACTED]"),
            ("\u{9b} password=hunter2secret".to_string(), "[REDACTED]"),
        ];
        for (input, expected) in cases {
            assert_eq!(red(&input), expected, "{input:?}");
        }
    }

    #[test]
    fn test_private_use_characters_without_escape_are_unchanged() {
        let s = "\u{E000} icon \u{F101} file \u{E0B0}";
        assert_eq!(red(s), s);
        assert_eq!(red("\u{E000}password=hunter2secret"), "\u{E000}[REDACTED]");
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

        /// Arbitrary (also incomplete) escapes in front of and behind a snippet
        /// never leave a secret the reference masks in the same input.
        #[test]
        fn any_escape_never_leaks_what_reference_masks(
            snip in secret_snippet(), a in any_escape(), c in any_escape(),
        ) {
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

        /// Escapes inserted at any char position (also inside or before a secret)
        /// never leave a literally present secret the reference masks in the
        /// same escaped input.
        #[test]
        fn escapes_at_random_positions_never_leak(
            snip in secret_snippet(),
            inserts in proptest::collection::vec((any::<usize>(), any_escape()), 1..6),
        ) {
            let mut chars: Vec<char> = snip.text.chars().collect();
            // Any insertion slot, including before secrets and in context words.
            let slots: Vec<usize> = (0..=chars.len()).collect();
            // Insert from the back so earlier positions stay valid.
            let mut picks: Vec<(usize, &str)> =
                inserts.iter().map(|(p, e)| (slots[p % slots.len()], *e)).collect();
            picks.sort_by_key(|x| std::cmp::Reverse(x.0));
            for (at, esc) in picks {
                for (i, ch) in esc.chars().enumerate() {
                    chars.insert(at + i, ch);
                }
            }
            let text: String = chars.into_iter().collect();
            let reference = ReferenceTrait::redact_text(&*REFERENCE, &text);
            let out = red(&text);
            for secret in snip
                .secrets
                .iter()
                .filter(|s| text.contains(s.as_str()) && !reference.contains(s.as_str()))
            {
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
