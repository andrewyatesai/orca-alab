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
        self.crashes_in_window += 1;
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
