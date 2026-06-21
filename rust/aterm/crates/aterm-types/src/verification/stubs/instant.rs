// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Counter-based `Instant` replacement for Kani proofs.

use std::time::Duration;

/// Global counter for `VerifyInstant::now()`.
///
/// # Safety
/// `static mut` is safe under Kani's single-threaded execution. Using this
/// instead of `AtomicU64` avoids state explosion in CBMC. In parallel tests,
/// this is technically UB but tests only assert monotonicity, not exact values.
static mut VERIFY_TIME_COUNTER: u64 = 0;

/// Counter-based `Instant` replacement for Kani proofs.
///
/// Avoids `clock_gettime` FFI that Kani cannot model. Each `now()` call
/// increments a global counter, producing monotonically increasing values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerifyInstant {
    /// Internal counter value representing "time".
    pub(super) ticks: u64,
}

impl VerifyInstant {
    /// Returns the current "instant" (counter-based, not real time).
    pub fn now() -> Self {
        // SAFETY: Kani proofs are single-threaded; static mut avoids AtomicU64 state explosion.
        let ticks = unsafe {
            let current = VERIFY_TIME_COUNTER;
            VERIFY_TIME_COUNTER = current.wrapping_add(1);
            current
        };
        Self { ticks }
    }

    /// Returns the elapsed duration since this instant was created.
    pub fn elapsed(&self) -> Duration {
        Self::now().duration_since(*self)
    }

    /// Returns the duration between `earlier` and `self`.
    ///
    /// # Panics
    ///
    /// Panics if `earlier` is later than `self`.
    pub fn duration_since(&self, earlier: VerifyInstant) -> Duration {
        assert!(
            self.ticks >= earlier.ticks,
            "duration_since called with later instant"
        );
        Duration::from_millis(self.ticks - earlier.ticks)
    }

    /// Returns `Some(t)` where `t` is the instant `self + duration`, or `None` on overflow.
    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        let millis = duration.as_millis();
        let millis_u64 = u64::try_from(millis).ok()?;
        self.ticks
            .checked_add(millis_u64)
            .map(|ticks| Self { ticks })
    }

    /// Returns `Some(t)` where `t` is the instant `self - duration`, or `None` on underflow.
    pub fn checked_sub(&self, duration: Duration) -> Option<Self> {
        let millis = duration.as_millis();
        let millis_u64 = u64::try_from(millis).ok()?;
        self.ticks
            .checked_sub(millis_u64)
            .map(|ticks| Self { ticks })
    }
}

// Mirrors std::time::Instant — Add/Sub panic on overflow per Rust convention.
#[allow(
    clippy::expect_used,
    reason = "mirrors std::time::Instant — Add/Sub panic on overflow per Rust convention"
)]
impl std::ops::Add<Duration> for VerifyInstant {
    type Output = Self;

    fn add(self, duration: Duration) -> Self {
        self.checked_add(duration)
            .expect("overflow when adding duration to instant")
    }
}

#[allow(
    clippy::expect_used,
    reason = "mirrors std::time::Instant — Add/Sub panic on overflow per Rust convention"
)]
impl std::ops::Sub<Duration> for VerifyInstant {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self {
        self.checked_sub(duration)
            .expect("overflow when subtracting duration from instant")
    }
}

impl std::ops::Sub<VerifyInstant> for VerifyInstant {
    type Output = Duration;

    fn sub(self, other: VerifyInstant) -> Duration {
        self.duration_since(other)
    }
}

impl Default for VerifyInstant {
    fn default() -> Self {
        Self::now()
    }
}
