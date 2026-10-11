//! Issue #327 (Spec 0106): heuristic that decides whether the last output
//! line of a running command looks like a question the command waits on
//! (`Do you want to continue? [Y/n]`, `[sudo] password for x:`).
//!
//! Display and cancel-note only: nothing here ever writes to the remote
//! stdin. A wrong guess costs a visible hint, never an action.

/// Only this many trailing bytes of the output are looked at.
pub const PROMPT_TAIL_BYTES: usize = 512;

const PROMPT_END_CHARS: [char; 5] = ['?', ':', ']', ')', '>'];

const PROMPT_PATTERNS: [&str; 17] = [
    "[y/n]",
    "[y/n/",
    "(y/n)",
    "(yes/no",
    "[yes/no]",
    "continue?",
    "continue [",
    "password",
    "passwort",
    "passphrase",
    "[sudo]",
    "press any key",
    "press enter",
    "are you sure",
    "overwrite",
    "rm: remove",
    "proceed",
];

/// `tail` is the most recent raw output (any length; only the last line is
/// examined). `true` when that line, trimmed, ends in `?`, `:`, `]`, `)` or
/// `>` and contains a known prompt pattern (case-insensitive).
pub fn looks_like_input_prompt(tail: &[u8]) -> bool {
    let text = String::from_utf8_lossy(tail);
    let trimmed = text.trim_end();
    let last_line = trimmed.rsplit(['\n', '\r']).next().unwrap_or("").trim();
    let Some(last_char) = last_line.chars().last() else {
        return false;
    };
    if !PROMPT_END_CHARS.contains(&last_char) {
        return false;
    }
    let lower = last_line.to_lowercase();
    PROMPT_PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompts_are_detected() {
        let positives = [
            "Do you want to continue? [Y/n] ",
            "Reading package lists... Done\nDo you want to continue? [Y/n]",
            "Proceed (y/n)? ",
            "Do you want to continue [Y/n]? ",
            "Is this ok [y/N]: ",
            ":: Proceed with installation? [Y/n] ",
            "rm: remove regular file 'x'? ",
            "/root/.ssh/id_ed25519 already exists.\nOverwrite (y/n)? ",
            "[sudo] password for alice: ",
            "Passwort: ",
            "Password:",
            "Enter passphrase for key '/home/a/.ssh/id':",
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            "Type yes or no (yes/no)",
            "Press ENTER to continue?",
            "Continue? [Y/n]\r",
        ];
        for p in positives {
            assert!(looks_like_input_prompt(p.as_bytes()), "{p:?}");
        }
    }

    #[test]
    fn test_non_prompts_are_not_detected() {
        let negatives = [
            "",
            "   \n",
            "working",
            "Reading package lists... Done",
            "Progress: [=====>    ] 50%",
            "[ 42%] Building CXX object foo.o",
            "2026-01-01 12:00:00 INFO started",
            "Setting up packages:",
            "Results:",
            "Installed:",
            "Download (1/3)",
            "Continue? [Y/n]\nInstalling foo",
            "prompt$ ",
            "Done.",
            "total 12\ndrwxr-xr-x 2 a a 4096 .",
            "usage: tool [options] <file>",
        ];
        for n in negatives {
            assert!(!looks_like_input_prompt(n.as_bytes()), "{n:?}");
        }
    }

    #[test]
    fn test_invalid_utf8_does_not_panic() {
        assert!(!looks_like_input_prompt(&[0xff, 0xfe, b'x']));
        assert!(looks_like_input_prompt(b"\xff Continue? [Y/n] "));
    }
}
