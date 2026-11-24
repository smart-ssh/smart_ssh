//! Option arguments of pass-through wrappers (Issue #1230).

/// Shared by the effective-command resolution and the `sftp-server` word
/// check (Issue #1230), so the two cannot drift apart. `env -S` is left out on
/// purpose: its argument is a command string that must stay visible.
///
/// Whether the (exact, case-sensitive) option `flag` of `program` takes its
/// argument as the next word. Attached forms (`-oL`, `--signal=KILL`) are
/// complete in themselves. Unknown options count as complete: the word after
/// them stays a candidate for the command, which only escalates more.
pub(crate) fn wrapper_option_takes_value(program: &str, flag: &str) -> bool {
    let flags: &[&str] = match program {
        "timeout" => &["-s", "--signal", "-k", "--kill-after"],
        "nice" => &["-n", "--adjustment"],
        "env" => &["-u", "--unset", "-C", "--chdir"],
        "stdbuf" => &["-i", "-o", "-e", "--input", "--output", "--error"],
        "ionice" => &[
            "-c",
            "-n",
            "-p",
            "-P",
            "-u",
            "--class",
            "--classdata",
            "--pid",
            "--pgid",
            "--uid",
        ],
        "runuser" => &[
            "-u",
            "-g",
            "-G",
            "-s",
            "--user",
            "--group",
            "--supp-group",
            "--shell",
        ],
        "chroot" => &["--userspec", "--groups"],
        "flock" => &["-w", "-E", "--timeout", "--wait", "--conflict-exit-code"],
        "xargs" => &["-n", "-I", "-L", "-P", "-s", "-d", "-E", "-a"],
        "time" => &["-f", "-o", "--format", "--output"],
        "exec" => &["-a"],
        "strace" => &["-e", "-o", "-p", "-s", "-u", "-E", "-a", "-P", "-I"],
        "watch" => &["-n", "--interval"],
        _ => &[],
    };
    flags.contains(&flag)
}
