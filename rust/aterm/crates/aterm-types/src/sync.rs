// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Non-poisoning synchronization primitives.
//!
//! Drop-in replacements for `parking_lot::Mutex`, `MutexGuard`, and `Condvar`
//! built on top of `std::sync`. These wrappers recover from poison automatically
//! so callers get the same ergonomic API as `parking_lot` (`.lock()` returns a
//! guard directly, no `Result`) without pulling in an external crate.
//!
//! # Why not `std::sync::Mutex` directly?
//!
//! Terminal state must remain accessible even if a thread panicked while holding
//! a lock. `std::sync::Mutex` returns `Result<MutexGuard, PoisonError>` from
//! `.lock()`, forcing every call site to handle poison — and the correct
//! terminal-engine answer is always "recover". These wrappers centralize that
//! decision.
//!
//! # Differences from `parking_lot`
//!
//! - Backed by `std::sync` (no external dependency, works under Miri).
//! - `Condvar::wait_for` matches `parking_lot`'s signature:
//!   `wait_for(&self, &mut MutexGuard, Duration) -> WaitTimeoutResult`.

use std::ops::{Deref, DerefMut};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Mutex
// ---------------------------------------------------------------------------

/// A non-poisoning mutual-exclusion lock.
///
/// Wraps [`std::sync::Mutex`] and auto-recovers from poison on every lock
/// acquisition, so callers never need to handle `PoisonError`.
pub struct Mutex<T>(std::sync::Mutex<T>);

impl<T> Mutex<T> {
    /// Create a new mutex wrapping `val`.
    #[must_use]
    pub const fn new(val: T) -> Self {
        Self(std::sync::Mutex::new(val))
    }

    /// Acquire the lock, recovering from poison if necessary.
    #[track_caller]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        let guard = match self.0.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                log_poison_recovery("Mutex::lock");
                poisoned.into_inner()
            }
        };
        MutexGuard(Some(guard))
    }

    /// Try to acquire the lock without blocking.
    ///
    /// Returns `None` if the lock is currently held by another thread.
    #[track_caller]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.0.try_lock() {
            Ok(guard) => Some(MutexGuard(Some(guard))),
            Err(std::sync::TryLockError::WouldBlock) => None,
            Err(std::sync::TryLockError::Poisoned(e)) => {
                log_poison_recovery("Mutex::try_lock");
                Some(MutexGuard(Some(e.into_inner())))
            }
        }
    }

    /// Get a mutable reference to the underlying data.
    ///
    /// Since this requires `&mut self`, no locking is needed.
    #[track_caller]
    pub fn get_mut(&mut self) -> &mut T {
        match self.0.get_mut() {
            Ok(r) => r,
            Err(poisoned) => {
                log_poison_recovery("Mutex::get_mut");
                poisoned.into_inner()
            }
        }
    }

    /// Consume the mutex and return the underlying data.
    #[track_caller]
    pub fn into_inner(self) -> T {
        match self.0.into_inner() {
            Ok(v) => v,
            Err(poisoned) => {
                log_poison_recovery("Mutex::into_inner");
                poisoned.into_inner()
            }
        }
    }
}

/// F11-4 (#7941): emit a structured error log when a poisoned lock is
/// silently recovered. Silent recovery hides the fact that some other
/// thread panicked mid-critical-section — observability must not lose
/// that signal. Pulled out into a function so every recovery site ends
/// up with identical output and a stable call-site location.
#[cold]
#[inline(never)]
#[track_caller]
fn log_poison_recovery(site: &'static str) {
    let caller = ::core::panic::Location::caller();
    aterm_log::error!(
        "{}: Mutex was poisoned at {}:{}:{} — recovering inner value; another thread panicked mid-critical-section",
        site,
        caller.file(),
        caller.line(),
        caller.column(),
    );
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_lock() {
            Ok(guard) => f.debug_tuple("Mutex").field(&*guard).finish(),
            Err(std::sync::TryLockError::WouldBlock) => {
                f.debug_tuple("Mutex").field(&"<locked>").finish()
            }
            Err(std::sync::TryLockError::Poisoned(e)) => {
                f.debug_tuple("Mutex").field(&*e.into_inner()).finish()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MutexGuard
// ---------------------------------------------------------------------------

/// RAII guard returned by [`Mutex::lock`].
///
/// Wraps [`std::sync::MutexGuard`] in an `Option` so that the inner guard can
/// be temporarily extracted for APIs that consume it by value (e.g.,
/// `std::sync::Condvar::wait_timeout`).
pub struct MutexGuard<'a, T>(Option<std::sync::MutexGuard<'a, T>>);

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    #[inline]
    #[allow(
        clippy::expect_used,
        reason = "trait impl cannot return Result; \
        INVARIANT: Option is always Some while the guard is live — \
        it is only temporarily None inside Condvar::wait_for"
    )]
    fn deref(&self) -> &T {
        // INVARIANT: The Option is always Some while the guard is live.
        // It is only temporarily None inside Condvar::wait_for.
        self.0.as_ref().expect("MutexGuard inner was taken")
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    #[inline]
    #[allow(
        clippy::expect_used,
        reason = "trait impl cannot return Result; \
        INVARIANT: Option is always Some while the guard is live — \
        it is only temporarily None inside Condvar::wait_for"
    )]
    fn deref_mut(&mut self) -> &mut T {
        // INVARIANT: The Option is always Some while the guard is live.
        // It is only temporarily None inside Condvar::wait_for.
        self.0.as_mut().expect("MutexGuard inner was taken")
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

// ---------------------------------------------------------------------------
// WaitTimeoutResult
// ---------------------------------------------------------------------------

/// Result of a condvar wait with timeout.
///
/// Matches the `parking_lot::WaitTimeoutResult` API: call `.timed_out()` to
/// check whether the wait expired before being notified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitTimeoutResult(bool);

impl WaitTimeoutResult {
    /// Returns `true` if the wait timed out.
    #[must_use]
    pub fn timed_out(&self) -> bool {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Condvar
// ---------------------------------------------------------------------------

/// A non-poisoning condition variable.
///
/// Wraps [`std::sync::Condvar`] and provides a `wait_for` method with the
/// same signature as `parking_lot::Condvar::wait_for`.
pub struct Condvar(std::sync::Condvar);

impl Condvar {
    /// Create a new condition variable.
    #[must_use]
    pub const fn new() -> Self {
        Self(std::sync::Condvar::new())
    }

    /// Wait on the condvar with a timeout.
    ///
    /// Temporarily releases the lock, waits up to `timeout`, then reacquires
    /// the lock. Returns a [`WaitTimeoutResult`] indicating whether the
    /// timeout elapsed.
    ///
    /// This matches the `parking_lot::Condvar::wait_for` signature.
    pub fn wait_for<T>(
        &self,
        guard: &mut MutexGuard<'_, T>,
        timeout: Duration,
    ) -> WaitTimeoutResult {
        // Take the inner std guard out so we can pass it by value to
        // std::sync::Condvar::wait_timeout, which consumes it.
        #[allow(
            clippy::expect_used,
            reason = "INVARIANT: Option is always Some \
            while the guard is live — we are the only code that calls .take()"
        )]
        let inner = guard.0.take().expect("MutexGuard inner was already taken");
        let (new_guard, result) = match self.0.wait_timeout(inner, timeout) {
            Ok(v) => v,
            Err(poisoned) => {
                log_poison_recovery("Condvar::wait_for");
                poisoned.into_inner()
            }
        };
        guard.0 = Some(new_guard);
        WaitTimeoutResult(result.timed_out())
    }

    /// Wake one waiting thread.
    pub fn notify_one(&self) {
        self.0.notify_one();
    }

    /// Wake all waiting threads.
    pub fn notify_all(&self) {
        self.0.notify_all();
    }
}

impl Default for Condvar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn mutex_lock_and_deref() {
        let m = Mutex::new(42);
        let guard = m.lock();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn mutex_lock_mut() {
        let m = Mutex::new(0);
        {
            let mut guard = m.lock();
            *guard = 7;
        }
        assert_eq!(*m.lock(), 7);
    }

    #[test]
    fn mutex_try_lock_succeeds_when_free() {
        let m = Mutex::new(1);
        let guard = m.try_lock();
        assert!(guard.is_some());
        assert_eq!(*guard.unwrap(), 1);
    }

    #[test]
    fn mutex_try_lock_fails_when_held() {
        let m = Mutex::new(1);
        let _guard = m.lock();
        assert!(m.try_lock().is_none());
    }

    #[test]
    fn mutex_get_mut() {
        let mut m = Mutex::new(10);
        *m.get_mut() = 20;
        assert_eq!(*m.lock(), 20);
    }

    #[test]
    fn mutex_into_inner() {
        let m = Mutex::new(99);
        assert_eq!(m.into_inner(), 99);
    }

    #[test]
    fn mutex_recovers_from_poison() {
        let m = Arc::new(Mutex::new(0));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let mut g = m2.lock();
            *g = 7;
            panic!("intentional poison");
        })
        .join();
        // The inner std Mutex is now poisoned, but our wrapper recovers.
        let guard = m.lock();
        assert_eq!(*guard, 7);
    }

    #[test]
    fn condvar_wait_for_timeout() {
        let m = Mutex::new(false);
        let c = Condvar::new();
        let mut guard = m.lock();
        let result = c.wait_for(&mut guard, Duration::from_millis(1));
        assert!(result.timed_out());
    }

    #[test]
    fn condvar_notify_before_timeout() {
        let pair = Arc::new((Mutex::new(false), Condvar::new()));
        let pair2 = Arc::clone(&pair);

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            let mut guard = pair2.0.lock();
            *guard = true;
            pair2.1.notify_all();
            drop(guard);
        });

        let mut guard = pair.0.lock();
        let start = std::time::Instant::now();
        loop {
            if *guard {
                break;
            }
            let result = pair.1.wait_for(&mut guard, Duration::from_secs(1));
            if result.timed_out() {
                panic!("timed out waiting for notification");
            }
        }
        assert!(*guard);
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn mutex_default() {
        let m: Mutex<i32> = Mutex::default();
        assert_eq!(*m.lock(), 0);
    }

    #[test]
    fn mutex_debug() {
        let m = Mutex::new(42);
        let dbg = format!("{m:?}");
        assert!(dbg.contains("42"), "debug should show value: {dbg}");
    }

    #[test]
    fn guard_debug() {
        let m = Mutex::new(42);
        let guard = m.lock();
        let dbg = format!("{guard:?}");
        assert_eq!(dbg, "42");
    }
}
