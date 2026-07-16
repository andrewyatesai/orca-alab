//! Rolling-window renderer-reload rate limiter.
//!
//! Ported from `src/main/crash-reporting/renderer-recovery-circuit-breaker.ts`.
//! A deterministic per-load renderer fault (bad GPU driver, corrupt chunk, AV
//! interference) crashes on every load; Orca auto-reloads recoverable deaths, so
//! without a breaker it reloads forever. This counts auto-recoveries in a rolling
//! window and opens the breaker once `max_recoveries` occur within it — stale
//! attempts age out naturally as the renderer survives longer than the window.

/// Outcome of a recovery attempt: whether the auto-reload is allowed, plus the
/// in-window attempt count after the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryDecision {
    pub allowed: bool,
    pub recent_recovery_count: u32,
}

/// Tracks recent auto-recovery attempts in a rolling time window. `now` is passed
/// in (ms) so the decision is deterministic and timer-free — mirrors the TS class.
#[derive(Debug, Clone)]
pub struct RendererRecoveryCircuitBreaker {
    window_ms: i64,
    max_recoveries: u32,
    attempts: Vec<i64>,
}

impl RendererRecoveryCircuitBreaker {
    #[must_use]
    pub fn new(window_ms: i64, max_recoveries: u32) -> Self {
        Self { window_ms, max_recoveries, attempts: Vec::new() }
    }

    /// Attempts still inside the rolling window at `now`.
    #[must_use]
    pub fn recent_recovery_count(&mut self, now: i64) -> u32 {
        self.prune_expired(now);
        self.attempts.len() as u32
    }

    /// Records a recovery attempt at `now` and reports whether it is allowed.
    /// Returns `allowed = false` once `max_recoveries` already occurred in the
    /// window — the caller must then stop auto-reloading. Mirrors the TS
    /// `registerRecoveryAttempt`: prune first, reject at/above the cap, else push.
    pub fn register_recovery_attempt(&mut self, now: i64) -> RecoveryDecision {
        self.prune_expired(now);
        if self.attempts.len() as u32 >= self.max_recoveries {
            return RecoveryDecision {
                allowed: false,
                recent_recovery_count: self.attempts.len() as u32,
            };
        }
        self.attempts.push(now);
        RecoveryDecision { allowed: true, recent_recovery_count: self.attempts.len() as u32 }
    }

    /// Clears history, e.g. after a manual reload resolves the loop.
    pub fn reset(&mut self) {
        self.attempts.clear();
    }

    /// Keeps only attempts strictly newer than `now - window_ms`. The strict `>`
    /// means a timestamp exactly at the cutoff has aged out — the edge the ay
    /// `rr3_prune_boundary` proof pins.
    fn prune_expired(&mut self, now: i64) {
        let cutoff = now - self.window_ms;
        self.attempts.retain(|&timestamp| timestamp > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_then_opens() {
        let mut b = RendererRecoveryCircuitBreaker::new(100, 3);
        assert_eq!(b.register_recovery_attempt(0), RecoveryDecision { allowed: true, recent_recovery_count: 1 });
        assert_eq!(b.register_recovery_attempt(10), RecoveryDecision { allowed: true, recent_recovery_count: 2 });
        assert_eq!(b.register_recovery_attempt(20), RecoveryDecision { allowed: true, recent_recovery_count: 3 });
        // 4th within the window is rejected; the rejected attempt is NOT recorded.
        assert_eq!(b.register_recovery_attempt(25), RecoveryDecision { allowed: false, recent_recovery_count: 3 });
        assert_eq!(b.register_recovery_attempt(30), RecoveryDecision { allowed: false, recent_recovery_count: 3 });
    }

    #[test]
    fn ages_out_of_the_window() {
        let mut b = RendererRecoveryCircuitBreaker::new(100, 3);
        b.register_recovery_attempt(0);
        b.register_recovery_attempt(10);
        b.register_recovery_attempt(20);
        // At now=120 the cutoff is 20; 0,10,20 are all <= 20, so all prune out.
        assert_eq!(b.register_recovery_attempt(120), RecoveryDecision { allowed: true, recent_recovery_count: 1 });
    }

    #[test]
    fn prune_boundary_is_strict() {
        let mut b = RendererRecoveryCircuitBreaker::new(100, 3);
        b.register_recovery_attempt(124);
        b.register_recovery_attempt(200);
        // now=224 -> cutoff=124; timestamp 124 is NOT > 124 so it prunes, 200 stays.
        assert_eq!(b.recent_recovery_count(224), 1);
    }

    #[test]
    fn reset_clears_history() {
        let mut b = RendererRecoveryCircuitBreaker::new(100, 3);
        b.register_recovery_attempt(0);
        b.register_recovery_attempt(10);
        b.reset();
        assert_eq!(b.recent_recovery_count(11), 0);
        assert_eq!(b.register_recovery_attempt(11), RecoveryDecision { allowed: true, recent_recovery_count: 1 });
    }

    /// Shared trace corpus — the same operations the TS class replays in its own
    /// test. Config is fixed at window=100, max=3 (encoded in the header).
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../renderer-recovery-parity-corpus.txt");
        let mut b = RendererRecoveryCircuitBreaker::new(100, 3);
        let mut checked = 0;
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (op, rest) = split_op(line, idx);
            match op {
                "attempt" => {
                    let (now, want) = parse_lhs_rhs(rest, idx);
                    let d = b.register_recovery_attempt(now);
                    let got = format!("{} {}", u8::from(d.allowed), d.recent_recovery_count);
                    assert_eq!(got, want, "line {}: attempt {now}", idx + 1);
                    checked += 1;
                }
                "count" => {
                    let (now, want) = parse_lhs_rhs(rest, idx);
                    assert_eq!(b.recent_recovery_count(now).to_string(), want, "line {}", idx + 1);
                    checked += 1;
                }
                "reset" => {
                    b.reset();
                    checked += 1;
                }
                other => panic!("line {}: unknown op {other}", idx + 1),
            }
        }
        assert!(checked >= 10, "corpus too small ({checked} ops)");
    }

    fn split_op(line: &str, idx: usize) -> (&str, &str) {
        let mut it = line.splitn(2, char::is_whitespace);
        let op = it.next().unwrap_or_else(|| panic!("line {}: empty", idx + 1));
        (op, it.next().unwrap_or("").trim())
    }

    /// `rest` is `<now> => <want...>`. Returns (now, want-string).
    fn parse_lhs_rhs(rest: &str, idx: usize) -> (i64, String) {
        let (lhs, rhs) = rest
            .split_once("=>")
            .unwrap_or_else(|| panic!("line {}: missing =>", idx + 1));
        (lhs.trim().parse().unwrap(), rhs.trim().to_string())
    }
}
