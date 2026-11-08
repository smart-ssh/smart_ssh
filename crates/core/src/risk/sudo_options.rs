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

/// Wrapper whose options and arguments the `sftp-server` word check skips
/// (Issue #1230).
const WRAPPERS_WITH_OPTIONS: &[&str] = &[
    "timeout", "nice", "env", "stdbuf", "ionice", "runuser", "chroot", "flock", "xargs", "time",
    "exec", "strace", "watch",
];

pub(super) struct WrapperState {
    program: &'static str,
    /// `timeout DURATION`, `chroot DIR`, `flock FILE`: one positional
    /// argument before the command.
    positional_pending: bool,
    options_done: bool,
}

pub(super) enum WrapperStep {
    /// The word is an option, an option argument or the positional argument.
    Skip,
    /// The word is the wrapped command (or another wrapper).
    Command,
}

impl WrapperState {
    pub(super) fn new(program: &str) -> Option<Self> {
        let program = WRAPPERS_WITH_OPTIONS.iter().find(|p| **p == program)?;
        Some(Self {
            program,
            positional_pending: matches!(*program, "timeout" | "chroot" | "flock"),
            options_done: false,
        })
    }

    /// Consumes one word (original case) behind the wrapper.
    /// `skip_argument` is set while an option's argument is due.
    pub(super) fn step(&mut self, word: &str, skip_argument: &mut bool) -> WrapperStep {
        if std::mem::take(skip_argument) {
            // The argument of an option is not a command — but never swallow
            // the program itself (`nice -n sftp-server`).
            return if word.to_lowercase().rsplit('/').next() == Some("sftp-server") {
                WrapperStep::Command
            } else {
                WrapperStep::Skip
            };
        }
        if !self.options_done && word.starts_with('-') {
            if word == "--" {
                self.options_done = true;
            } else if crate::filter::wrapper_option_takes_value(self.program, word) {
                *skip_argument = true;
            }
            return WrapperStep::Skip;
        }
        if self.positional_pending {
            self.positional_pending = false;
            self.options_done = true;
            return WrapperStep::Skip;
        }
        WrapperStep::Command
    }
}
