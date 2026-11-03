//! Issue #898: the one bridge from synchronous code to asynchronous code.
//!
//! [`ssh_manager_core::profiles::CredentialStore`] is synchronous, `sqlx` is
//! asynchronous only. [`block_on_io`] is the single place where a synchronous
//! caller waits for a future; extensions with their own synchronous trait
//! methods use it instead of rebuilding the case table.
//!
//! | Caller | `Handle::try_current()` | Way |
//! |---|---|---|
//! | outside any runtime (start-up, plain threads) | `Err` | `handle.block_on(..)` directly |
//! | worker of a multi-thread runtime (async task, `spawn_blocking`) | `Ok`, multi-thread | `block_in_place(\|\| handle.block_on(..))`: the runtime moves its other tasks to other threads |
//! | current-thread runtime | `Ok`, current-thread | `block_in_place` would panic ("can call blocking only when running on the multi-threaded runtime"), and `handle.block_on` inside a runtime context panics too. The future runs on a short-lived helper thread instead; the caller waits for it. Never a panic. |
//!
//! The *entered* runtime decides, not the one `handle` belongs to:
//! `block_in_place` panics at the runtime whose context the call is in.
//!
//! Limits: in the current-thread case the calling thread is blocked, so a
//! future that needs that very runtime to make progress (a task spawned on
//! it) cannot complete; futures that only need their own I/O threads (the
//! `sqlx` pool) are fine.
//!
//! **Pool of size 1.** The SQLite pool has `max_connections(1)`. A bridge call
//! made while the same caller holds the only connection does not hang
//! silently: `sqlx` fails after its `acquire_timeout` (`pool_timed_out`).
//! Every store method releases its connection within the same `await`, so
//! this cannot occur today.
//!
//! The bridge does not look at what the future returns and never logs it, so
//! no secret passes through its messages or panics. A panic of the future
//! on the helper thread is re-raised unchanged on the caller.

use std::future::Future;

use tokio::runtime::{Handle, RuntimeFlavor};

/// Runs `fut` to completion on `handle` and returns its output, blocking the
/// calling thread. See the module documentation for the three cases.
pub fn block_on_io<F>(handle: &Handle, fut: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    match Handle::try_current() {
        Err(_) => handle.block_on(fut),
        Ok(entered) if entered.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(fut))
        }
        Ok(_) => std::thread::scope(|scope| match scope.spawn(|| handle.block_on(fut)).join() {
            Ok(out) => out,
            Err(panic) => std::panic::resume_unwind(panic),
        }),
    }
}

/// Runs a blocking closure (typically one that calls a synchronous store) on
/// the blocking pool so the async worker stays free. Hand `SecretString`s
/// across as they are; they stay wrapped.
pub async fn spawn_blocking_io<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(out) => out,
        Err(join) if join.is_panic() => std::panic::resume_unwind(join.into_panic()),
        // The runtime shut down while the work was queued.
        Err(_) => panic!("blocking task cancelled by runtime shutdown"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn multi() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    async fn work() -> u32 {
        tokio::time::sleep(Duration::from_millis(5)).await;
        7
    }

    #[test]
    fn no_runtime_blocks_directly() {
        let rt = multi();
        assert!(Handle::try_current().is_err());
        assert_eq!(block_on_io(rt.handle(), work()), 7);
    }

    #[test]
    fn multi_thread_caller_does_not_panic() {
        let rt = multi();
        let handle = rt.handle().clone();
        let out = rt.block_on(async move {
            let h = handle.clone();
            tokio::spawn(async move { block_on_io(&h, work()) })
                .await
                .unwrap()
        });
        assert_eq!(out, 7);
    }

    #[test]
    fn current_thread_caller_does_not_panic() {
        let rt = multi();
        let current = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        let out = current.block_on(async move {
            assert_eq!(
                Handle::current().runtime_flavor(),
                RuntimeFlavor::CurrentThread
            );
            block_on_io(&handle, work())
        });
        assert_eq!(out, 7);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spawn_blocking_io_returns_the_value() {
        assert_eq!(spawn_blocking_io(|| 3).await, 3);
    }
}
