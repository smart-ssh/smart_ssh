//! Issue #1145: keeps test processes from crashing at exit.
//!
//! A pool that is dropped unclosed is closed by its worker thread in the
//! background while the process runs its exit handlers. SQLCipher
//! (`atexit(sqlcipher_atexit)`) and OpenSSL (`OPENSSL_cleanup`) free state
//! that the worker still uses. Two measures, both test builds only:
//!
//! * [`close_before_exit`] makes sure every pool is closed and awaited before
//!   `sqlcipher_atexit` runs. The worker has dropped the connection before it
//!   confirms the shutdown. A drop usually happens while the test's runtime is
//!   still alive and a spawned "return connection to pool" task has not run
//!   yet, so the pool cannot be closed right there without deadlocking that
//!   runtime. Such pools are parked and closed by an exit handler that is
//!   registered after SQLCipher's own (handlers run in reverse order), when
//!   the runtime is long gone and the pending tasks have been dropped.
//! * [`init_openssl_without_atexit`] initialises OpenSSL with
//!   `OPENSSL_INIT_NO_ATEXIT` (what `openssl_sys::init` does), so the worker
//!   thread's own TSD cleanup after the close cannot race `OPENSSL_cleanup`.

use std::sync::{Mutex, Once, PoisonError};

use sqlx::SqlitePool;

static PARKED: Mutex<Vec<SqlitePool>> = Mutex::new(Vec::new());
static REGISTER: Once = Once::new();

pub(crate) fn init_openssl_without_atexit() {
    openssl_sys::init();
}

/// Called when a store is dropped. Takes over a clone of its pool; the pool
/// is closed and awaited at the latest at process exit.
pub(crate) fn close_before_exit(pool: &SqlitePool) {
    if pool.is_closed() {
        return;
    }
    // Registered after the first connection (and so after SQLCipher's own
    // `atexit`), therefore it runs first.
    REGISTER.call_once(|| {
        // SAFETY: `close_parked` is a plain `extern "C" fn()`.
        #[allow(unsafe_code)]
        unsafe {
            libc::atexit(close_parked)
        };
    });
    PARKED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(pool.clone());
}

extern "C" fn close_parked() {
    let pools = std::mem::take(&mut *PARKED.lock().unwrap_or_else(PoisonError::into_inner));
    // Own thread and runtime: no runtime exists on the exiting thread.
    let _ = std::thread::Builder::new()
        .name("pool-close-at-exit".into())
        .spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(async {
                    for pool in &pools {
                        pool.close().await;
                    }
                });
            }
        })
        .map(|handle| handle.join());
}
