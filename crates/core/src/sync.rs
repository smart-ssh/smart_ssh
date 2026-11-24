//! One strategy for poisoned `std::sync` locks (Spec 0088, A2.1/A2.2).
//!
//! A `std::sync::Mutex`/`RwLock` is **poisoned** as soon as a thread panics
//! while holding it. Every later `lock().unwrap()` then panics as well, which
//! turns a one-off failure into a permanent one — and a panic inside `Drop`
//! while another panic unwinds aborts the whole process (A2.2).
//!
//! # Rule
//!
//! Production code locks `std::sync` primitives **only** through the helpers
//! in this module ([`lock_tolerating_poison`], [`read_tolerating_poison`],
//! [`write_tolerating_poison`]); `lock().unwrap()`, `lock().expect(..)` and the
//! `read`/`write` equivalents are not used outside tests (a guard test in this
//! crate scans the workspace). Where a poisoned value could be half-updated
//! and continuing with it would be wrong, the call site resets the value to a
//! safe default right after recovering and says so in a comment — it never
//! panics. Recovering must never skip a revocation or turn a pending decision
//! into an approval.
//!
//! `tokio::sync` locks are never poisoned and are out of scope.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Locks `mutex` and keeps working with a poisoned value instead of
/// panicking (Spec 0088, A2.1).
///
/// Deliberately no `unwrap`/`expect`: `PoisonError::into_inner` yields exactly
/// the guard a successful `lock()` would have returned.
pub fn lock_tolerating_poison<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Read-locks `lock`, tolerating poisoning like [`lock_tolerating_poison`].
pub fn read_tolerating_poison<T: ?Sized>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

/// Write-locks `lock`, tolerating poisoning like [`lock_tolerating_poison`].
pub fn write_tolerating_poison<T: ?Sized>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

/// Poisons `mutex` on purpose: a thread panics while holding the lock.
///
/// Tests only (Spec 0088, T5/T6a/T6b/T6c/T7). The helper thread's panic shows
/// up as noise in the test output — that is intended, not a failure.
#[cfg(any(test, feature = "test-support"))]
pub fn poison_for_test<T: Send>(mutex: &Mutex<T>) {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = lock_tolerating_poison(mutex);
                panic!("test poisoning (Spec 0088) — expected");
            })
            .join()
            .expect_err("the helper thread must have panicked");
    });
    assert!(
        mutex.is_poisoned(),
        "the mutex must be poisoned after the panic"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counter-proof anchor for A2.1: against a `lock().unwrap()` this test
    /// would be red (panic instead of a value).
    #[test]
    fn test_lock_tolerating_poison_still_yields_the_value_after_poisoning() {
        let mutex = Mutex::new(41_u32);
        poison_for_test(&mutex);
        *lock_tolerating_poison(&mutex) += 1;
        assert_eq!(*lock_tolerating_poison(&mutex), 42);
    }

    fn poison_rwlock(lock: &RwLock<u32>) {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = lock.write().expect("not poisoned yet");
                    panic!("test poisoning of an RwLock — expected");
                })
                .join()
                .expect_err("the helper thread must have panicked");
        });
        assert!(lock.read().is_err() && lock.write().is_err());
    }

    #[test]
    fn test_read_tolerating_poison_still_yields_the_value_after_poisoning() {
        let lock = RwLock::new(7_u32);
        poison_rwlock(&lock);
        assert_eq!(*read_tolerating_poison(&lock), 7);
    }

    #[test]
    fn test_write_tolerating_poison_still_yields_the_value_after_poisoning() {
        let lock = RwLock::new(7_u32);
        poison_rwlock(&lock);
        *write_tolerating_poison(&lock) += 1;
        assert_eq!(*read_tolerating_poison(&lock), 8);
    }
}
