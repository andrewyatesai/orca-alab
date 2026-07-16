//! Provider rate-limit refetch backoff — a pure capped-exponential-backoff core.
//!
//! Ported from the inline throttle in `src/main/rate-limits/service.ts`
//! (`getActiveWindowRefreshPlan`): after a provider's active-window fetch fails,
//! its per-provider *failure streak* drives how long we wait before retrying, so
//! a persistently failing provider backs off toward a ceiling instead of
//! hammering the network. The decision is a pure function of the streak count.
//!
//! Same E1 pair as `orca-flow-control`: proven equivalent to the TS by the shared
//! `parity-corpus.txt`, and proven correct by `proofs/ay/bo_*.smt2`. This unit
//! adds a proof shape the flow-control units did not exercise — a *saturating*
//! exponential, whose Rust `1u64 << exp` must never overflow for ANY streak.

/// Base throttle: the wait after the first failure. Mirrors the TS
/// `ACTIVE_FAILURE_REFETCH_MS` (= `MIN_POLL_MS`, 30s).
pub const ACTIVE_FAILURE_REFETCH_MS: u64 = 30 * 1000;
/// Ceiling the backoff saturates to. Mirrors the TS
/// `MAX_ACTIVE_FAILURE_REFETCH_MS` (= `DEFAULT_POLL_MS`, 15min).
pub const MAX_ACTIVE_FAILURE_REFETCH_MS: u64 = 15 * 60 * 1000;
/// The streak counter the caller clamps to (informational — the sizing itself is
/// total and saturating for every streak, so it never relies on this bound).
/// Mirrors the TS `MAX_ACTIVE_FAILURE_STREAK`.
pub const MAX_ACTIVE_FAILURE_STREAK: u32 = 8;

/// Refetch throttle (ms) for a provider with the given consecutive-failure streak.
///
/// Mirrors the TS `Math.min(ACTIVE_FAILURE_REFETCH_MS * 2 ** Math.max(0, streak - 1),
/// MAX_ACTIVE_FAILURE_REFETCH_MS)`:
/// - `streak.saturating_sub(1)` is exactly `max(0, streak - 1)` for a `u32` count
///   (streak 0 and 1 both give exponent 0);
/// - the shift is made **overflow-safe for every streak**: `checked_shl` past the
///   `u64` width yields the saturating `u64::MAX`, and `saturating_mul`/`min` then
///   clamp to the ceiling — so a huge streak can never panic and always returns the
///   ceiling, matching the TS (where the analogous `2 ** big` overflows to a value
///   still `>= MAX`, so the `min` also yields `MAX`).
#[must_use]
pub fn active_failure_refetch_throttle_ms(streak: u32) -> u64 {
    let exp = streak.saturating_sub(1);
    let multiplier = 1u64.checked_shl(exp).unwrap_or(u64::MAX);
    ACTIVE_FAILURE_REFETCH_MS
        .saturating_mul(multiplier)
        .min(MAX_ACTIVE_FAILURE_REFETCH_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_in_the_backoff_band() {
        // Floor and ceiling hold for every streak, including 0 and the extremes
        // (the saturating shift must not panic or wrap).
        for streak in [0u32, 1, 2, 5, 6, 8, 20, 63, 64, 65, 1000, u32::MAX] {
            let ms = active_failure_refetch_throttle_ms(streak);
            assert!(
                (ACTIVE_FAILURE_REFETCH_MS..=MAX_ACTIVE_FAILURE_REFETCH_MS).contains(&ms),
                "streak={streak} ms={ms} out of [30_000, 900_000]"
            );
        }
    }

    #[test]
    fn known_points_match_the_ts_doubling() {
        // 30_000 * 2^max(0, streak-1), capped at 900_000.
        assert_eq!(active_failure_refetch_throttle_ms(0), 30_000); // 2^0
        assert_eq!(active_failure_refetch_throttle_ms(1), 30_000); // 2^0
        assert_eq!(active_failure_refetch_throttle_ms(2), 60_000); // 2^1
        assert_eq!(active_failure_refetch_throttle_ms(3), 120_000); // 2^2
        assert_eq!(active_failure_refetch_throttle_ms(4), 240_000); // 2^3
        assert_eq!(active_failure_refetch_throttle_ms(5), 480_000); // 2^4
        // streak 6: 30_000 * 32 = 960_000 -> capped to 900_000.
        assert_eq!(active_failure_refetch_throttle_ms(6), 900_000);
        assert_eq!(active_failure_refetch_throttle_ms(8), 900_000); // caller's cap
    }

    #[test]
    fn is_non_decreasing_in_streak() {
        let mut prev = active_failure_refetch_throttle_ms(0);
        for streak in 1..=64u32 {
            let ms = active_failure_refetch_throttle_ms(streak);
            assert!(ms >= prev, "throttle fell from {prev} to {ms} at streak={streak}");
            prev = ms;
        }
    }

    #[test]
    fn saturates_and_stays_saturated() {
        // Once at the ceiling (streak >= 6 for these constants) it never leaves it,
        // and the largest possible streak is overflow-safe.
        for streak in 6..=40u32 {
            assert_eq!(active_failure_refetch_throttle_ms(streak), MAX_ACTIVE_FAILURE_REFETCH_MS);
        }
        assert_eq!(
            active_failure_refetch_throttle_ms(u32::MAX),
            MAX_ACTIVE_FAILURE_REFETCH_MS
        );
    }

    /// Shared corpus (`parity-corpus.txt`) — the same oracle the TS
    /// `activeFailureRefetchThrottleMs` runs in its own test.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../parity-corpus.txt");
        let mut checked = 0;
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: `<streak> => <throttleMs>`
            let (lhs, rhs) = line
                .split_once("=>")
                .unwrap_or_else(|| panic!("line {}: missing =>", idx + 1));
            let streak: u32 = lhs.trim().parse().unwrap();
            let want: u64 = rhs.trim().parse().unwrap();
            assert_eq!(
                active_failure_refetch_throttle_ms(streak),
                want,
                "throttle mismatch at streak={streak}"
            );
            checked += 1;
        }
        assert!(checked >= 8, "corpus too small ({checked} rows)");
    }
}
