//! Issue #1191: the sudo password path of the elevated file browser. The
//! exec channel runs `sudo -S -k -p '<marker>' …`; the password is written
//! only after the marker appeared on stderr, and a second marker (sudo asks
//! again) means the password was wrong.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use ssh_manager_core::ssh::{SshError, SudoPromptError, TaskSpawner};
use ssh_manager_core::sync::lock_tolerating_poison;
use tokio::io::{duplex, split, AsyncReadExt, AsyncWriteExt, DuplexStream};

/// stderr of the prompt phase is only kept for the marker search and the
/// error message, never longer than this.
const MAX_STDERR: usize = 8 * 1024;
/// The SSH extended-data type of stderr.
const STDERR_EXT: u32 = 1;

/// Accumulates stderr and looks for the prompt marker (it can be split
/// across chunks).
pub(crate) struct StderrWatch {
    marker: String,
    buf: String,
}

impl StderrWatch {
    pub(crate) fn new(marker: &str) -> Self {
        Self {
            marker: marker.to_string(),
            buf: String::new(),
        }
    }

    /// Adds `data`; true once the marker is in the buffer.
    pub(crate) fn push(&mut self, data: &[u8]) -> bool {
        self.buf.push_str(&String::from_utf8_lossy(data));
        if self.buf.len() > MAX_STDERR {
            let mut cut = self.buf.len() - MAX_STDERR;
            while !self.buf.is_char_boundary(cut) {
                cut += 1;
            }
            self.buf.drain(..cut);
        }
        self.buf.contains(&self.marker)
    }

    /// Forgets what was seen so far (after the password was sent).
    pub(crate) fn reset(&mut self) {
        self.buf.clear();
    }

    /// First meaningful stderr line without the marker, shortened.
    fn first_line(&self) -> String {
        self.buf
            .replace(&self.marker, "")
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(200)
            .collect()
    }

    fn denial(&self) -> Option<String> {
        let lower = self.buf.to_lowercase();
        (lower.contains("not allowed") || lower.contains("may not run")).then(|| self.first_line())
    }
}

/// Reads channel messages until the marker is on stderr. sudo exiting first
/// (or the channel ending, or the timeout) is an error; nothing was written.
pub(crate) async fn wait_for_marker(
    channel: &mut Channel<Msg>,
    watch: &mut StderrWatch,
    timeout: Duration,
) -> Result<(), SudoPromptError> {
    let wait = async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::ExtendedData { data, ext }) if ext == STDERR_EXT => {
                    if watch.push(&data) {
                        return Ok(());
                    }
                }
                Some(
                    ChannelMsg::ExitStatus { .. }
                    | ChannelMsg::Eof
                    | ChannelMsg::Close
                    | ChannelMsg::ExitSignal { .. },
                )
                | None => {
                    return Err(match watch.denial() {
                        Some(line) => SudoPromptError::NotAllowed(line),
                        None => SudoPromptError::Failed(SshError::ChannelError(format!(
                            "sudo ended before asking for the password: {}",
                            watch.first_line()
                        ))),
                    });
                }
                Some(_) => {}
            }
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .map_err(|_| SudoPromptError::NoPrompt)?
}

#[derive(Default)]
struct BridgeState {
    wrong_password: bool,
    not_allowed: Option<String>,
    last_line: String,
}

/// What the bridge saw on stderr after the password was written.
pub(crate) struct BridgeHandle(Arc<StdMutex<BridgeState>>);

impl BridgeHandle {
    /// The reason sudo refused, if stderr showed one.
    pub(crate) fn failure(&self) -> Option<SudoPromptError> {
        let st = lock_tolerating_poison(&self.0);
        if st.wrong_password {
            Some(SudoPromptError::WrongPassword)
        } else {
            st.not_allowed.clone().map(SudoPromptError::NotAllowed)
        }
    }
}

/// After the password was sent: pumps stdout/stdin between the channel and
/// the returned stream (the SFTP protocol) and keeps watching stderr. A
/// second marker means a wrong password: the channel is closed at once.
pub(crate) fn bridge(
    mut channel: Channel<Msg>,
    mut watch: StderrWatch,
    spawner: Option<&TaskSpawner>,
) -> (DuplexStream, BridgeHandle) {
    let (ours, theirs) = duplex(256 * 1024);
    let state = Arc::new(StdMutex::new(BridgeState::default()));
    let task_state = state.clone();
    let (mut rd, mut wr) = split(theirs);
    let fut = async move {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            tokio::select! {
                msg = channel.wait() => match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if wr.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, ext }) if ext == STDERR_EXT => {
                        let again = watch.push(&data);
                        let mut st = lock_tolerating_poison(&task_state);
                        if again {
                            st.wrong_password = true;
                        }
                        if let Some(line) = watch.denial() {
                            st.not_allowed = Some(line);
                        }
                        st.last_line = watch.first_line();
                        drop(st);
                        if again {
                            break;
                        }
                    }
                    Some(ChannelMsg::Eof | ChannelMsg::Close | ChannelMsg::ExitStatus { .. }
                        | ChannelMsg::ExitSignal { .. }) | None => break,
                    Some(_) => {}
                },
                n = rd.read(&mut buf) => match n {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if channel.data_bytes(buf[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                },
            }
        }
        let _ = wr.shutdown().await;
        let _ = channel.close().await;
    };
    crate::sftp_tap::spawn_pump(spawner, "sudo-sftp-bridge", fut);
    (ours, BridgeHandle(state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_split_across_chunks_is_found() {
        let mut w = StderrWatch::new("smart-ssh-sudo-abc:");
        assert!(!w.push(b"smart-ssh-su"));
        assert!(w.push(b"do-abc:"));
    }

    #[test]
    fn denial_is_recognised_and_marker_stripped() {
        let mut w = StderrWatch::new("m-1:");
        w.push(b"sudo: alice is not allowed to run sudo on h.\n");
        assert!(w.denial().unwrap().contains("not allowed"));
    }
}
