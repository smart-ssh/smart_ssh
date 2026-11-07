//! Option parsing for `sudo`/`doas` words (Issue #1208).

/// Issue #1208: how a `sudo`/`doas` word starting with `-` continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SudoOption {
    /// `--`: end of options.
    EndOfOptions,
    /// Option without an argument, or with an attached one (`-uroot`,
    /// `--user=root`).
    Complete,
    /// Option whose argument is the next word (`-u root`, `--prompt 'x'`).
    TakesNext,
}

/// Classifies one `sudo`/`doas` option word. `word` keeps its original case
/// (`-H` takes no argument, `-h` does). Unknown options are `Complete`, so
/// the next word still counts as the command.
pub(super) fn classify_sudo_option(program: &str, word: &str) -> SudoOption {
    if word == "--" {
        return SudoOption::EndOfOptions;
    }
    if let Some(long) = word.strip_prefix("--") {
        if long.contains('=') {
            return SudoOption::Complete;
        }
        const LONG_WITH_ARG: &[&str] = &[
            "close-from",
            "chdir",
            "group",
            "host",
            "prompt",
            "chroot",
            "role",
            "command-timeout",
            "type",
            "other-user",
            "user",
        ];
        return if program == "sudo" && LONG_WITH_ARG.contains(&long.to_lowercase().as_str()) {
            SudoOption::TakesNext
        } else {
            SudoOption::Complete
        };
    }
    let with_arg: &[char] = if program == "doas" {
        &['C', 'u']
    } else {
        &['C', 'D', 'g', 'h', 'p', 'R', 'r', 'T', 't', 'U', 'u']
    };
    // Cluster such as `-Sk`: the first option letter that takes an argument
    // ends it; letters after it are the attached argument.
    let letters: Vec<char> = word.chars().skip(1).collect();
    match letters.iter().position(|c| with_arg.contains(c)) {
        Some(pos) if pos + 1 == letters.len() => SudoOption::TakesNext,
        _ => SudoOption::Complete,
    }
}
