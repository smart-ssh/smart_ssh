//! Issue #1184: the authentication time limit of a hop stands still while
//! the user is asked for an answer (`keyboard-interactive`, multi-step
//! login), like the host key decision which lies outside every limit.

use std::future::Future;
use std::time::Duration;

use ssh_manager_core::ssh::SshError;
use tokio::sync::watch;
use tokio::time::Instant;

/// Shared between the authentication code (which pauses) and the timeout
/// around it (which stops counting while paused).
pub(crate) struct AuthPause {
    tx: watch::Sender<bool>,
}

impl AuthPause {
    pub(crate) fn new() -> Self {
        Self {
            tx: watch::channel(false).0,
        }
    }

    /// Pauses the limit until the returned guard is dropped (also when the
    /// waiting future is cancelled).
    pub(crate) fn pause(&self) -> PauseGuard<'_> {
        self.tx.send_replace(true);
        PauseGuard(self)
    }
}

pub(crate) struct PauseGuard<'a>(&'a AuthPause);

impl Drop for PauseGuard<'_> {
    fn drop(&mut self) {
        self.0.tx.send_replace(false);
    }
}

/// Like `connect_with_timeout`, but the clock only runs while `pause` is not
/// held. Without a pause it behaves exactly like a plain timeout.
pub(crate) async fn timeout_unless_paused<F, T>(
    fut: F,
    limit: Duration,
    pause: &AuthPause,
) -> Result<T, SshError>
where
    F: Future<Output = Result<T, SshError>>,
{
    tokio::pin!(fut);
    let mut rx = pause.tx.subscribe();
    let mut remaining = limit;
    loop {
        if *rx.borrow_and_update() {
            tokio::select! {
                result = &mut fut => return result,
                changed = rx.changed() => {
                    if changed.is_err() {
                        return fut.await;
                    }
                }
            }
            continue;
        }
        let started = Instant::now();
        tokio::select! {
            result = &mut fut => return result,
            () = tokio::time::sleep(remaining) => return Err(SshError::Timeout),
            changed = rx.changed() => {
                remaining = remaining.saturating_sub(started.elapsed());
                if changed.is_err() {
                    return match tokio::time::timeout(remaining, &mut fut).await {
                        Ok(result) => result,
                        Err(_elapsed) => Err(SshError::Timeout),
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn test_clock_stands_still_while_paused() {
        let pause = AuthPause::new();
        let limit = Duration::from_secs(60);
        let work = async {
            tokio::time::sleep(Duration::from_secs(50)).await;
            let guard = pause.pause();
            tokio::time::sleep(Duration::from_secs(3600)).await;
            drop(guard);
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok::<_, SshError>(7)
        };
        assert_eq!(timeout_unless_paused(work, limit, &pause).await, Ok(7));
    }

    #[tokio::test(start_paused = true)]
    async fn test_limit_still_applies_around_the_pause() {
        let pause = AuthPause::new();
        let work = async {
            tokio::time::sleep(Duration::from_secs(50)).await;
            drop(pause.pause());
            tokio::time::sleep(Duration::from_secs(11)).await;
            Ok::<_, SshError>(())
        };
        assert_eq!(
            timeout_unless_paused(work, Duration::from_secs(60), &pause).await,
            Err(SshError::Timeout)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_unpaused_behaves_like_plain_timeout() {
        let pause = AuthPause::new();
        let work = async {
            tokio::time::sleep(Duration::from_secs(61)).await;
            Ok::<_, SshError>(())
        };
        assert_eq!(
            timeout_unless_paused(work, Duration::from_secs(60), &pause).await,
            Err(SshError::Timeout)
        );
    }
}
