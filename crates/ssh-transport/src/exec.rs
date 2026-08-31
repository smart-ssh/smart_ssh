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
/// Maximale Puffergröße pro Stream (stdout / stderr) vor dem Abschneiden (Spec 0013, SEC-09).
pub const MAX_STREAM_OUTPUT_BYTES: usize = 2 * 1024 * 1024; // 2 MB
pub const TRUNCATION_NOTICE: &[u8] = b"\n[Output truncated: exceeded 2 MB limit]";

pub(crate) fn accumulate_exec_output(
    messages: impl IntoIterator<Item = ChannelMsg>,
) -> CommandOutput {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;
    let mut exit_code = None;

    for msg in messages {
        match msg {
            ChannelMsg::Data { data } => {
                if !stdout_truncated {
                    if stdout.len() + data.len() <= MAX_STREAM_OUTPUT_BYTES {
                        stdout.extend_from_slice(&data);
                    } else {
                        let remaining = MAX_STREAM_OUTPUT_BYTES.saturating_sub(stdout.len());
                        stdout.extend_from_slice(&data[..remaining]);
                        stdout.extend_from_slice(TRUNCATION_NOTICE);
                        stdout_truncated = true;
                    }
                }
            }
            // Extended-Data-Code 1 = stderr (RFC4254 5.2); andere Codes sind
            // nicht spezifiziert und werden ignoriert.
            ChannelMsg::ExtendedData { data, ext: 1 } => {
                if !stderr_truncated {
                    if stderr.len() + data.len() <= MAX_STREAM_OUTPUT_BYTES {
                        stderr.extend_from_slice(&data);
                    } else {
                        let remaining = MAX_STREAM_OUTPUT_BYTES.saturating_sub(stderr.len());
                        stderr.extend_from_slice(&data[..remaining]);
                        stderr.extend_from_slice(TRUNCATION_NOTICE);
                        stderr_truncated = true;
                    }
                }
            }
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
