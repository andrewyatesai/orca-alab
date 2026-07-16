//! Background-session keep-tail sizing — the second flow-control decision core.
//!
//! Ported from `src/main/daemon/daemon-stream-keep-tail-drop.ts`. A hidden pane's
//! stream copy is only a monitoring feed (the daemon holds the full model; reveal
//! restores from its snapshot), so once a backgrounded session's undelivered
//! output exceeds the drop cap its oldest bytes are dropped down to the keep-tail.
//! The keep-tail shrinks as more sessions compete for the shared global budget,
//! bounding the AGGREGATE a reveal must drain while never starving one session
//! below the floor.
//!
//! Pure functions of the droppable-session count — the same E1 pair as the
//! producer controller: proven equivalent to the TS by `keep-tail-parity-corpus.txt`
//! and proven correct by `proofs/ay/kt_*.smt2`.

/// Upper bound on a single session's keep-tail (also its value when few sessions
/// compete). Mirrors `BACKGROUND_SESSION_KEEP_TAIL_CHARS`.
pub const BACKGROUND_SESSION_KEEP_TAIL_CHARS: u64 = 512 * 1024;
/// Floor: every droppable session keeps at least this much tail even under heavy
/// competition. Mirrors `BACKGROUND_SESSION_MIN_KEEP_TAIL_CHARS`.
pub const BACKGROUND_SESSION_MIN_KEEP_TAIL_CHARS: u64 = 64 * 1024;
/// Shared budget divided across competing sessions before clamping. Mirrors
/// `BACKGROUND_GLOBAL_KEEP_BUDGET_CHARS`.
pub const BACKGROUND_GLOBAL_KEEP_BUDGET_CHARS: u64 = 2 * 1024 * 1024;

/// Keep-tail chars for a session, given how many droppable sessions currently
/// have queued data. `clamp(budget / max(1, n), [MIN, MAX])`. Matches the TS
/// `Math.min(MAX, Math.max(MIN, Math.floor(BUDGET / Math.max(1, n))))` — u64
/// division is already floored, and `max(1, n)` guards n = 0 exactly as the TS does.
#[must_use]
pub fn background_session_keep_tail_chars(droppable_sessions_with_queued_data: u64) -> u64 {
    BACKGROUND_SESSION_KEEP_TAIL_CHARS.min(
        BACKGROUND_SESSION_MIN_KEEP_TAIL_CHARS
            .max(BACKGROUND_GLOBAL_KEEP_BUDGET_CHARS / droppable_sessions_with_queued_data.max(1)),
    )
}

/// Drop cap for a session: twice its keep-tail (the queue may grow to the cap
/// before it is thinned back down to the tail). Mirrors
/// `backgroundSessionDropCapChars = keepTail * 2`.
#[must_use]
pub fn background_session_drop_cap_chars(droppable_sessions_with_queued_data: u64) -> u64 {
    background_session_keep_tail_chars(droppable_sessions_with_queued_data) * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_tail_stays_in_the_clamp_band() {
        // The floor and cap hold for every session count, including 0.
        for n in [0u64, 1, 2, 3, 4, 8, 16, 31, 32, 33, 64, 100, 1000, u64::MAX] {
            let kt = background_session_keep_tail_chars(n);
            assert!(
                (BACKGROUND_SESSION_MIN_KEEP_TAIL_CHARS..=BACKGROUND_SESSION_KEEP_TAIL_CHARS)
                    .contains(&kt),
                "n={n} kt={kt} out of [64K, 512K]"
            );
        }
    }

    #[test]
    fn keep_tail_known_points() {
        // n=0 and n<=4 → the full 512K (budget/n >= 512K, capped).
        assert_eq!(background_session_keep_tail_chars(0), 512 * 1024);
        assert_eq!(background_session_keep_tail_chars(1), 512 * 1024);
        assert_eq!(background_session_keep_tail_chars(4), 512 * 1024); // 2M/4 = 512K
        // mid-range: keep-tail = floor(2M / n).
        assert_eq!(background_session_keep_tail_chars(8), 256 * 1024); // 2M/8
        assert_eq!(background_session_keep_tail_chars(16), 128 * 1024);
        assert_eq!(background_session_keep_tail_chars(32), 64 * 1024); // 2M/32 = 64K = floor
        // beyond the budget: pinned to the 64K floor.
        assert_eq!(background_session_keep_tail_chars(33), 64 * 1024);
        assert_eq!(background_session_keep_tail_chars(1000), 64 * 1024);
    }

    #[test]
    fn keep_tail_is_non_increasing_in_session_count() {
        let mut prev = background_session_keep_tail_chars(1);
        for n in 2..=200u64 {
            let kt = background_session_keep_tail_chars(n);
            assert!(kt <= prev, "keep-tail rose from {prev} to {kt} at n={n}");
            prev = kt;
        }
    }

    #[test]
    fn drop_cap_is_twice_keep_tail() {
        for n in [0u64, 1, 5, 16, 32, 33, 128] {
            assert_eq!(
                background_session_drop_cap_chars(n),
                background_session_keep_tail_chars(n) * 2
            );
        }
    }

    #[test]
    fn mid_range_aggregate_respects_the_budget() {
        // Where the divide is the active term (4 <= n <= 32), n sessions each
        // keeping floor(2M/n) never exceed the 2M global budget.
        for n in 4..=32u64 {
            let agg = n * background_session_keep_tail_chars(n);
            assert!(
                agg <= BACKGROUND_GLOBAL_KEEP_BUDGET_CHARS,
                "n={n} aggregate {agg} exceeds budget"
            );
        }
    }

    /// Shared corpus (`keep-tail-parity-corpus.txt`) — the same oracle the TS
    /// `backgroundSessionKeepTailChars`/`DropCapChars` run in their own test.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../keep-tail-parity-corpus.txt");
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: `<n> => <keepTail> <dropCap>`
            let (lhs, rhs) = line
                .split_once("=>")
                .unwrap_or_else(|| panic!("line {}: missing =>", idx + 1));
            let n: u64 = lhs.trim().parse().unwrap();
            let want: Vec<u64> = rhs.split_whitespace().map(|s| s.parse().unwrap()).collect();
            assert_eq!(want.len(), 2, "line {}: want `keepTail dropCap`", idx + 1);
            assert_eq!(
                background_session_keep_tail_chars(n),
                want[0],
                "keep-tail mismatch at n={n}"
            );
            assert_eq!(
                background_session_drop_cap_chars(n),
                want[1],
                "drop-cap mismatch at n={n}"
            );
        }
    }
}
