//! One-shot GPU software-rendering fallback latch.
//!
//! Ported from `src/main/crash-reporting/gpu-crash-fallback-decision.ts` (the
//! numeric tracker only — the string/platform crash-candidate predicates stay in
//! TS). On old/flaky GPU drivers the GPU child crashes within seconds of launch,
//! repeatedly; a burst inside the post-launch window is the signal that hardware
//! acceleration is unusable. This counts in-window crashes and engages software
//! rendering exactly once when the count first reaches the threshold.

/// Outcome of recording a GPU crash: whether this crash just tripped the fallback,
/// plus the in-window crash count after the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuCrashDecision {
    pub should_engage_fallback: bool,
    pub crashes_in_window: u32,
}

/// Tracks GPU child crashes relative to launch and latches software-rendering
/// fallback. `ms_since_launch` is passed in so the decision is timer-free. The
/// window is inclusive at both ends: `0 <= ms_since_launch <= window_ms` counts.
#[derive(Debug, Clone)]
pub struct GpuCrashFallbackTracker {
    window_ms: i64,
    threshold: u32,
    crashes_in_window: u32,
    engaged: bool,
}

impl GpuCrashFallbackTracker {
    #[must_use]
    pub fn new(window_ms: i64, threshold: u32) -> Self {
        Self { window_ms, threshold, crashes_in_window: 0, engaged: false }
    }

    /// Records a GPU child crash at `ms_since_launch` and reports whether it just
    /// pushed the count to the threshold (fallback should engage now). Crashes
    /// outside `[0, window_ms]`, or any crash after fallback already engaged, are
    /// no-ops — so the caller relaunches at most once. Mirrors the TS
    /// `recordGpuCrash`. (The TS `Number.isFinite` guard has no integer analogue;
    /// callers pass integer ms, and the corpus exercises the range gate directly.)
    pub fn record_gpu_crash(&mut self, ms_since_launch: i64) -> GpuCrashDecision {
        if self.engaged || ms_since_launch < 0 || ms_since_launch > self.window_ms {
            return GpuCrashDecision {
                should_engage_fallback: false,
                crashes_in_window: self.crashes_in_window,
            };
        }
        // Saturating, not `+= 1`: `threshold` is a caller value, so nothing in this
        // type bounds the counter, and Trust refutes the plain add.
        self.crashes_in_window = self.crashes_in_window.saturating_add(1);
        if self.crashes_in_window >= self.threshold {
            self.engaged = true;
            return GpuCrashDecision {
                should_engage_fallback: true,
                crashes_in_window: self.crashes_in_window,
            };
        }
        GpuCrashDecision {
            should_engage_fallback: false,
            crashes_in_window: self.crashes_in_window,
        }
    }

    #[must_use]
    pub fn has_engaged(&self) -> bool {
        self.engaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engages_once_at_the_threshold() {
        let mut t = GpuCrashFallbackTracker::new(30, 3);
        assert_eq!(t.record_gpu_crash(5), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 1 });
        assert_eq!(t.record_gpu_crash(10), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 2 });
        assert_eq!(t.record_gpu_crash(15), GpuCrashDecision { should_engage_fallback: true, crashes_in_window: 3 });
        // Already engaged: every later crash is a no-op (relaunch at most once).
        assert_eq!(t.record_gpu_crash(20), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 3 });
        assert!(t.has_engaged());
    }

    #[test]
    fn ignores_out_of_window_crashes() {
        let mut t = GpuCrashFallbackTracker::new(30, 3);
        assert_eq!(t.record_gpu_crash(-1), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 0 });
        assert_eq!(t.record_gpu_crash(31), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 0 });
        // Boundaries 0 and window_ms are INCLUSIVE.
        assert_eq!(t.record_gpu_crash(0), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 1 });
        assert_eq!(t.record_gpu_crash(30), GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 2 });
    }

    // -------------------------------------------------------------------
    // Whole-trace latch invariants.
    //
    // These replace the retired hand-encoded SMT bundle. Its `gf1`/`gf3` modelled
    // the post-count as `ite(active, k+1, k)` over unbounded `Int`, so they could
    // not see the `saturating_add` at `u32::MAX` and they simply assumed
    // `threshold >= 1` — a state the constructor never enforces.
    // -------------------------------------------------------------------

    /// Crash-time alphabet around both inclusive window edges (window = 30).
    const TIMES: [i64; 5] = [-1, 0, 15, 30, 31];

    /// Over every 5-crash trace and several thresholds: the fallback engages AT
    /// MOST ONCE, never below the threshold, never on an out-of-window crash, and
    /// an ignored crash leaves the count untouched. Replaces gf1 + gf2 + gf3.
    #[test]
    fn fallback_engages_at_most_once_and_only_inside_the_window() {
        let mut traces = 0;
        let mut saw_engage = 0;
        let mut saw_ignored = 0;
        for threshold in [1u32, 2, 3] {
            for a in TIMES {
                for b in TIMES {
                    for c in TIMES {
                        for e in TIMES {
                            for f in TIMES {
                                let mut t = GpuCrashFallbackTracker::new(30, threshold);
                                traces += 1;
                                let mut engagements = 0;
                                let mut prev_count = 0u32;
                                for ms in [a, b, c, e, f] {
                                    let already = t.has_engaged();
                                    let d = t.record_gpu_crash(ms);
                                    let in_window = (0..=30).contains(&ms);

                                    // gf2: a crash outside [0, window] is a no-op.
                                    if !in_window {
                                        assert_eq!(
                                            d.crashes_in_window, prev_count,
                                            "out-of-window crash at {ms} changed the count"
                                        );
                                        assert!(!d.should_engage_fallback);
                                        saw_ignored += 1;
                                    }
                                    // Once latched, every later crash is a no-op.
                                    if already {
                                        assert_eq!(d.crashes_in_window, prev_count);
                                        assert!(!d.should_engage_fallback);
                                    }
                                    // gf3: never engage below the threshold.
                                    if d.should_engage_fallback {
                                        engagements += 1;
                                        assert!(
                                            d.crashes_in_window >= threshold,
                                            "engaged at {} below threshold {threshold}",
                                            d.crashes_in_window
                                        );
                                        assert!(in_window, "engaged on an out-of-window crash");
                                        assert!(t.has_engaged(), "engaged without latching");
                                    }
                                    // The count only ever moves up by one, never down.
                                    assert!(d.crashes_in_window >= prev_count);
                                    assert!(d.crashes_in_window <= prev_count + 1);
                                    prev_count = d.crashes_in_window;
                                }
                                // gf1: the latch is one-shot, so the caller relaunches
                                // into software rendering at most once.
                                assert!(engagements <= 1, "engaged {engagements} times");
                                if engagements == 1 {
                                    saw_engage += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(traces > 3_000, "domain too thin ({traces})");
        // Non-vacuity (the retired `gf_c1_engage_reachable_sat` control).
        assert!(saw_engage > 0, "no trace ever engaged the fallback");
        assert!(saw_ignored > 0, "no trace ever ignored an out-of-window crash");
    }

    /// A zero threshold is a degenerate configuration the constructor does not
    /// reject; pin what it actually does. The retired proofs asserted `t >= 1` as a
    /// precondition and left this state undefined.
    #[test]
    fn a_zero_threshold_engages_on_the_first_in_window_crash() {
        let mut t = GpuCrashFallbackTracker::new(30, 0);
        assert_eq!(
            t.record_gpu_crash(0),
            GpuCrashDecision { should_engage_fallback: true, crashes_in_window: 1 }
        );
        assert!(t.has_engaged());
        // Still one-shot.
        assert_eq!(
            t.record_gpu_crash(1),
            GpuCrashDecision { should_engage_fallback: false, crashes_in_window: 1 }
        );
    }

    /// Shared trace corpus — the same crashes the TS tracker replays. Config fixed
    /// at window=30, threshold=3.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../gpu-fallback-parity-corpus.txt");
        let mut t = GpuCrashFallbackTracker::new(30, 3);
        let mut checked = 0;
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: `crash <msSinceLaunch> => <shouldEngage> <crashesInWindow>`
            let rest = line
                .strip_prefix("crash")
                .unwrap_or_else(|| panic!("line {}: expected `crash`", idx + 1));
            let (lhs, rhs) = rest
                .split_once("=>")
                .unwrap_or_else(|| panic!("line {}: missing =>", idx + 1));
            let ms: i64 = lhs.trim().parse().unwrap();
            let d = t.record_gpu_crash(ms);
            let got = format!("{} {}", u8::from(d.should_engage_fallback), d.crashes_in_window);
            assert_eq!(got, rhs.trim(), "line {}: crash {ms}", idx + 1);
            checked += 1;
        }
        assert!(checked >= 6, "corpus too small ({checked} ops)");
    }
}
