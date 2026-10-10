//! Redaction of live command output (issue #325, Spec 0106, ADR 0130).
//!
//! The chat shows the output of a running command while it arrives. That
//! live view must never show a secret that the final, redacted result
//! hides. Redacting each chunk (or each line) on its own is not enough: the
//! redactor has patterns that span lines — a private key block, a quoted
//! value with a line break, a key on one line and its value on the next
//! (`password:` / `hunter2`). [`LiveOutputRedactor`] therefore releases text
//! only in whole lines and only when the redaction of those lines is stable:
//!
//! - **Context before:** the lines are redacted together with up to
//!   [`CONTEXT_BYTES`] of already released output, so a value whose key was
//!   on an earlier line is still recognised.
//! - **Look-ahead after:** at least one further complete line must have
//!   arrived, and the redaction of the released lines must be a prefix of
//!   the redaction of everything received so far — a match that would reach
//!   into the next line keeps the lines back.
//! - **Line end kept:** the released text must end with a line break. A
//!   match that swallows the line end (an unterminated private key block is
//!   masked up to the end of the text) keeps everything from its start back
//!   until the block ends.
//! - **Never revise:** if the new lines change the redaction of the already
//!   shown context, the released block is replaced by one placeholder
//!   instead of its text.
//!
//! What is held back appears with the final result when the command ends.
//! The last line of a stream therefore only appears live once the next line
//! follows. Above [`MAX_HELD_BYTES`] held back without a stable cut, the live
//! view of that stream stops (fail-safe) — the final result is unaffected.

use super::redactor::{OutputRedactor, REDACTED_PLACEHOLDER};

/// Already released raw output kept as left context for the next release.
pub const CONTEXT_BYTES: usize = 4096;

/// Upper bound for output held back without a stable cut (e.g. an open
/// private key block that never ends). Beyond it the stream stops showing
/// live output, so the redaction work per release stays bounded.
pub const MAX_HELD_BYTES: usize = 256 * 1024;

/// How many cut positions (counted from the newest line backwards) a
/// release tries before it waits for more output.
const MAX_CANDIDATES: usize = 3;

/// Live redaction state of one output stream (stdout or stderr) of one
/// running command. Purely in memory, display only.
#[derive(Debug, Default)]
pub struct LiveOutputRedactor {
    /// Tail of the already released raw output, used as left context.
    context: Vec<u8>,
    /// Received raw output not released yet; starts at a line start.
    pending: Vec<u8>,
    stalled: bool,
}

impl LiveOutputRedactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends raw output as it arrives. Ignored once the stream stalled.
    pub fn push(&mut self, bytes: &[u8]) {
        if !self.stalled {
            self.pending.extend_from_slice(bytes);
        }
    }

    /// `true` once more than [`MAX_HELD_BYTES`] were held back without a
    /// stable cut; no further live output for this stream.
    pub fn is_stalled(&self) -> bool {
        self.stalled
    }

    /// Returns the redacted text that may be shown now (possibly empty) and
    /// removes it from the pending output. See the module documentation for
    /// the rules.
    pub fn release(&mut self, redactor: &dyn OutputRedactor) -> String {
        if self.stalled {
            return String::new();
        }
        let ends = line_ends(&self.pending);
        let n = ends.len();
        if n >= 2 {
            let context_red = redact(redactor, &self.context, &[]);
            let full = redact(redactor, &self.context, &self.pending[..ends[n - 1]]);
            for k in (1..n).rev().take(MAX_CANDIDATES) {
                let cut = ends[k - 1];
                let candidate = redact(redactor, &self.context, &self.pending[..cut]);
                if !(candidate.ends_with('\n') || candidate.ends_with('\r'))
                    || !full.starts_with(&candidate)
                {
                    continue;
                }
                let released = match candidate.strip_prefix(context_red.as_str()) {
                    Some(new_part) => new_part.to_string(),
                    // The new lines change how the already shown context
                    // is redacted — it cannot be taken back, so the new
                    // block is masked as a whole.
                    None => format!("{REDACTED_PLACEHOLDER}\n"),
                };
                let lines: Vec<u8> = self.pending.drain(..cut).collect();
                self.context.extend_from_slice(&lines);
                trim_context(&mut self.context);
                return released;
            }
        }
        if self.pending.len() > MAX_HELD_BYTES {
            self.stalled = true;
            self.pending = Vec::new();
        }
        String::new()
    }
}

fn redact(redactor: &dyn OutputRedactor, context: &[u8], lines: &[u8]) -> String {
    let mut text = Vec::with_capacity(context.len() + lines.len());
    text.extend_from_slice(context);
    text.extend_from_slice(lines);
    // Lines are cut only at `\n`/`\r`, which never occur inside a UTF-8
    // sequence — lossy decoding therefore sees complete sequences only.
    redactor.redact_text(&String::from_utf8_lossy(&text))
}

/// End positions (exclusive) of all complete lines. A line ends with `\n`,
/// `\r\n` or a lone `\r` (progress output). A trailing `\r` is not yet a
/// line end: a following `\n` may still arrive in the next chunk.
fn line_ends(data: &[u8]) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'\n' => ends.push(i + 1),
            b'\r' => match data.get(i + 1) {
                Some(b'\n') => {}
                Some(_) => ends.push(i + 1),
                None => break,
            },
            _ => {}
        }
        i += 1;
    }
    ends
}

/// Keeps at most [`CONTEXT_BYTES`] of the released tail, starting at a line
/// start when the tail contains one.
fn trim_context(context: &mut Vec<u8>) {
    if context.len() <= CONTEXT_BYTES {
        return;
    }
    let mut start = context.len() - CONTEXT_BYTES;
    if let Some(pos) = context[start..context.len() - 1]
        .iter()
        .position(|b| *b == b'\n' || *b == b'\r')
    {
        start += pos + 1;
    }
    context.drain(..start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::DefaultOutputRedactor;
    use crate::ssh::CommandOutput;

    fn final_redaction(raw: &str) -> String {
        let output = CommandOutput {
            stdout: raw.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
            truncated: false,
        };
        String::from_utf8(DefaultOutputRedactor::new().redact(&output).stdout).unwrap()
    }

    /// Feeds `raw` in chunks of `chunk` bytes, releasing after every chunk,
    /// and returns everything the live view showed.
    fn live_view(raw: &str, chunk: usize) -> String {
        let redactor = DefaultOutputRedactor::new();
        let mut live = LiveOutputRedactor::new();
        let mut shown = String::new();
        for part in raw.as_bytes().chunks(chunk) {
            live.push(part);
            shown.push_str(&live.release(&redactor));
        }
        shown
    }

    /// Every chunk size, so each secret is also split at every position.
    fn assert_never_shows(raw: &str, secret: &str) {
        assert!(
            !final_redaction(raw).contains(secret),
            "precondition: the final result masks {secret:?}"
        );
        for chunk in 1..=raw.len() {
            let shown = live_view(raw, chunk);
            assert!(
                !shown.contains(secret),
                "chunk size {chunk}: live view showed {secret:?}: {shown:?}"
            );
        }
    }

    #[test]
    fn test_lines_are_released_once_the_next_line_arrives() {
        let redactor = DefaultOutputRedactor::new();
        let mut live = LiveOutputRedactor::new();
        live.push(b"1\n");
        assert_eq!(live.release(&redactor), "");
        live.push(b"2\n");
        assert_eq!(live.release(&redactor), "1\n");
        live.push(b"3\n4\n");
        assert_eq!(live.release(&redactor), "2\n3\n");
    }

    #[test]
    fn test_partial_lines_are_never_released() {
        let redactor = DefaultOutputRedactor::new();
        let mut live = LiveOutputRedactor::new();
        live.push(b"a\nb\npartial AKIA");
        assert_eq!(live.release(&redactor), "a\n");
        live.push(b"1234");
        assert_eq!(live.release(&redactor), "");
    }

    #[test]
    fn test_carriage_return_ends_a_progress_line() {
        let redactor = DefaultOutputRedactor::new();
        let mut live = LiveOutputRedactor::new();
        live.push(b"10%\r20%\r30%");
        assert_eq!(live.release(&redactor), "10%\r");
    }

    #[test]
    fn test_crlf_counts_as_one_line_end() {
        assert_eq!(line_ends(b"a\r\nb\r\n"), vec![3, 6]);
        assert_eq!(line_ends(b"a\r"), Vec::<usize>::new());
        assert_eq!(line_ends(b"a\rb"), vec![2]);
    }

    #[test]
    fn test_multibyte_characters_split_across_chunks_stay_intact() {
        let shown = live_view("Grüße ✓\nnächste Zeile\nende\n", 1);
        assert_eq!(shown, "Grüße ✓\nnächste Zeile\n");
    }

    #[test]
    fn test_single_line_secret_is_masked_at_every_split() {
        assert_never_shows(
            "start\naws key AKIAIOSFODNN7EXAMPLE here\nnext\nend\n",
            "AKIAIOSFODNN7EXAMPLE",
        );
    }

    #[test]
    fn test_value_on_the_line_after_its_key_is_masked() {
        // `password:` alone on one line, the value on the next one.
        assert_never_shows(
            "config\npassword:\nhunter2secret\nmore\nend\n",
            "hunter2secret",
        );
    }

    #[test]
    fn test_value_after_blank_lines_is_masked_through_the_context() {
        assert_never_shows(
            "config\npassword:\n\n\n\nhunter2secret\nmore\nend\n",
            "hunter2secret",
        );
    }

    #[test]
    fn test_private_key_block_is_never_shown() {
        let raw = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\nQUJDREVGR0hJSktMTU5PUA\n-----END OPENSSH PRIVATE KEY-----\nafter\nend\n";
        assert_never_shows(raw, "b3BlbnNzaC1rZXktdjEAAAAA");
        assert_never_shows(raw, "QUJDREVGR0hJSktMTU5PUA");
        let shown = live_view(raw, 7);
        assert!(shown.starts_with("before\n"), "{shown:?}");
        assert!(shown.ends_with("after\n"), "{shown:?}");
    }

    #[test]
    fn test_unterminated_private_key_block_is_held_back() {
        let redactor = DefaultOutputRedactor::new();
        let mut live = LiveOutputRedactor::new();
        live.push(b"before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEsecretbody\nmore body\n");
        let shown = live.release(&redactor);
        assert_eq!(shown, "before\n");
        live.push(b"MIIEanotherbody\n");
        assert_eq!(live.release(&redactor), "");
    }

    #[test]
    fn test_quoted_value_spanning_two_lines_is_masked() {
        // A quoted command-line password with a line break: only the
        // closing quote on the next line makes the earlier part a match.
        let raw = "$ sshpass -p 'firstHalf\nsecondHalf' ssh host\nok\nend\n";
        assert_never_shows(raw, "firstHalf");
        assert_never_shows(raw, "secondHalf");
    }

    #[test]
    fn test_stream_stalls_when_too_much_is_held_back() {
        let redactor = DefaultOutputRedactor::new();
        let mut live = LiveOutputRedactor::new();
        live.push(b"-----BEGIN PRIVATE KEY-----\n");
        let body = "QUJD\n".repeat(MAX_HELD_BYTES / 5 + 10);
        live.push(body.as_bytes());
        assert_eq!(live.release(&redactor), "");
        assert!(live.is_stalled());
        live.push(b"-----END PRIVATE KEY-----\nafter\nend\n");
        assert_eq!(live.release(&redactor), "");
    }

    #[test]
    fn test_context_is_trimmed_to_a_line_start() {
        let mut context = b"x".repeat(CONTEXT_BYTES).to_vec();
        context.extend_from_slice(b"\nlast line\n");
        trim_context(&mut context);
        assert_eq!(context, b"last line\n");
    }

    #[test]
    fn test_released_text_equals_final_result_for_plain_output() {
        let raw: String = (0..200).map(|i| format!("line {i}\n")).collect();
        let shown = live_view(&raw, 13);
        let all_but_last: String = (0..199).map(|i| format!("line {i}\n")).collect();
        assert_eq!(shown, all_but_last);
    }
}
