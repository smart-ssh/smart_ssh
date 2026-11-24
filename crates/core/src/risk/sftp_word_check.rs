//! Quote-aware word check for `sftp-server` invocations (Issues #1208,
//! #1230, #1232); split out of `classifier.rs`.

use super::classifier::{shell_words, RESHELL_COMMANDS};
use super::sudo_options::{classify_sudo_option, SudoOption, WrapperState, WrapperStep};

/// Ergebnis der Wort-Prüfung auf `sftp-server`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WordCheck {
    Found,
    /// Tiefer verschachtelt als geprüft wird — fail-safe eskalieren.
    TooDeep,
    Clean,
}

/// Programme, hinter denen das nächste Nicht-Options-Wort ausgeführt wird.
const COMMAND_PREFIXES: &[&str] = &[
    "sudo", "doas", "env", "nice", "nohup", "time", "command", "exec", "timeout", "setsid",
    "stdbuf", "ionice", "chroot", "flock", "busybox", "xargs", "runuser", "su", "watch", "strace",
    "ltrace", "script", "builtin",
];

/// Quote-aware counterpart of the red pattern (Issue #1232): `true` when
/// `command` starts `sftp-server` at the command position behind wrappers,
/// env assignments and (quoted) option arguments, or inside the code string
/// of a re-shell (`su root -c "…"`). Bare mentions stay `false`.
pub(super) fn quoted_sftp_server_invocation(command: &str) -> bool {
    sftp_server_word_check_mode(command, 0, true) == WordCheck::Found
}

pub(super) fn sftp_server_word_check(command: &str, depth: usize) -> WordCheck {
    sftp_server_word_check_mode(command, depth, false)
}

/// `strict`: only a hit the red pattern would also count (command position,
/// or nested code of a re-shell), not the broad "anywhere after a shell" hit.
fn sftp_server_word_check_mode(command: &str, depth: usize, strict: bool) -> WordCheck {
    if depth > 3 {
        return WordCheck::TooDeep;
    }
    // Original case (sudo options are case-sensitive); comparisons lowercase.
    let words = shell_words(command);
    let lower_texts: Vec<String> = words.iter().map(|w| w.text.to_lowercase()).collect();
    let name = |text: &str| text.rsplit('/').next().unwrap_or("").to_string();
    let has_reshell = lower_texts
        .iter()
        .any(|text| RESHELL_COMMANDS.contains(&name(text).as_str()));
    let mut at_command_position = true;
    // `Some(program)` while the options of a `sudo`/`doas` at the command
    // position are being skipped.
    let mut in_sudo_options: Option<&'static str> = None;
    let mut skip_argument = false;
    // A re-shell seen in the current pipeline stage (Issue #1285).
    let mut stage_reshell = false;
    // Same for other wrappers (`timeout 5m`, `nice -n 10`, Issue #1230).
    let mut in_wrapper: Option<WrapperState> = None;
    for (idx, (word, text)) in words.iter().zip(&lower_texts).enumerate() {
        if word.stage_start {
            stage_reshell = false;
            at_command_position = true;
            in_sudo_options = None;
            in_wrapper = None;
            skip_argument = false;
        }
        if at_command_position {
            if let Some(state) = in_wrapper.as_mut() {
                match state.step(&word.text, &mut skip_argument) {
                    WrapperStep::Skip => continue,
                    WrapperStep::Command => in_wrapper = None,
                }
            }
            if let Some(program) = in_sudo_options {
                if skip_argument {
                    // The argument of a sudo option (`-p 'sftp-server'`) is
                    // not a command.
                    skip_argument = false;
                    continue;
                }
                if word.text.starts_with('-') {
                    match classify_sudo_option(program, &word.text) {
                        SudoOption::EndOfOptions => in_sudo_options = None,
                        SudoOption::Complete => {}
                        SudoOption::TakesNext => skip_argument = true,
                    }
                    continue;
                }
                in_sudo_options = None;
            }
        }
        let word_name = name(text);
        // Issue #1285: the single word after a re-shell's `-c` is a code
        // string even without whitespace (`su root -c "sftp-server"`).
        let after_reshell_c = strict
            && stage_reshell
            && idx > 0
            && lower_texts[idx - 1] == "-c"
            && !words[idx].stage_start;
        if word_name == "sftp-server" && after_reshell_c {
            return WordCheck::Found;
        }
        if RESHELL_COMMANDS.contains(&word_name.as_str()) {
            stage_reshell = true;
        }
        if word_name == "sftp-server" && (at_command_position || (has_reshell && !strict)) {
            return WordCheck::Found;
        }
        if text.contains(char::is_whitespace)
            && text.contains("sftp-server")
            && (!strict || has_reshell)
        {
            let nested = sftp_server_word_check_mode(text, depth + 1, strict);
            if nested != WordCheck::Clean {
                return nested;
            }
        }
        if at_command_position {
            if word_name == "sudo" || word_name == "doas" {
                in_sudo_options = Some(if word_name == "sudo" { "sudo" } else { "doas" });
            }
            let is_prefix = COMMAND_PREFIXES.contains(&word_name.as_str())
                || RESHELL_COMMANDS.contains(&word_name.as_str())
                || text.starts_with('-')
                || is_env_assignment(text)
                || text
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == 's');
            if let Some(state) = WrapperState::new(&word_name) {
                in_wrapper = Some(state);
            }
            at_command_position = is_prefix;
        }
    }
    WordCheck::Clean
}

/// Test access to the word check alone (Issue #1208).
#[cfg(test)]
pub(super) fn sftp_server_word_check_for_tests(command: &str) -> &'static str {
    match sftp_server_word_check(command, 0) {
        WordCheck::Found => "found",
        WordCheck::TooDeep => "too deep",
        WordCheck::Clean => "clean",
    }
}

/// `NAME=wert` vor einem Kommando.
fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}
