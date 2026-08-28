use russh::ChannelMsg;
use ssh_manager_core::ssh::CommandOutput;

/// Baut aus einer Sequenz von `ChannelMsg`s (wie sie `Channel::wait()` im
/// Exec-Modus liefert) das fertige [`CommandOutput`] zusammen.
///
/// Bewusst als reine Funktion von der eigentlichen `Channel`-I/O entkoppelt
/// — dadurch ohne echtes Netzwerk testbar (`cargo test -p ssh-transport
/// --lib`). Die echte `execute()`-Implementierung (s.
/// `crate::transport::RusshTransport`) treibt `channel.wait()` einfach so
/// lange, bis `None` kommt, sammelt die Nachrichten und übergibt sie hier
/// hinein.
pub(crate) fn accumulate_exec_output(
    messages: impl IntoIterator<Item = ChannelMsg>,
) -> CommandOutput {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = None;

    for msg in messages {
        match msg {
            ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
            // Extended-Data-Code 1 = stderr (RFC4254 5.2); andere Codes sind
            // nicht spezifiziert und werden ignoriert.
            ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status as i32),
            _ => {}
        }
    }

    CommandOutput {
        stdout,
        stderr,
        exit_code,
    }
}
