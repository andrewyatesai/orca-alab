//! Daemon session-history GC planning core.
//!
//! Ported from `src/main/daemon/history-retention.ts` (`runDaemonSessionHistoryGc`,
//! now split into a pure planner + an fs executor). Given the scanned session dirs
//! and the liveness/budget context, it decides which dirs to age-expire and which
//! to evict for the size cap — the fs scan and the `rmSync`s stay in TS. Every
//! retention bound is a privacy bound (scrollback is secret-bearing), so the safety
//! properties are "never expire/evict a live or unknown-liveness recoverable
//! session" and "keep the store under budget, oldest-first".
//!
//! Same E1 pair as the other decision cores: proven equivalent to the TS by
//! `parity-corpus.txt`, proven correct by `proofs/ay/*.smt2`.

#![forbid(unsafe_code)]

use std::collections::HashSet;

/// A scanned session dir reduced to the fields the plan depends on.
#[derive(Debug, Clone)]
pub struct SessionGcPlannerDir {
    pub name: String,
    pub total_bytes: u64,
    /// Newest mtime across the dir — "last activity" (ms).
    pub last_activity_ms: i64,
    /// `meta.endedAt` is a non-null string (the dir can no longer cold-restore).
    pub is_ended: bool,
}

/// Retention/floor thresholds (ms). Mirrors the TS constants.
#[derive(Debug, Clone, Copy)]
pub struct SessionGcThresholds {
    pub min_dir_age_ms: i64,
    pub ended_retention_ms: i64,
    pub unrestored_retention_ms: i64,
}

/// The plan: names to delete, and the store bytes remaining if all deletions
/// succeed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionGcPlan {
    /// Names to delete for age (in scan order).
    pub expire: Vec<String>,
    /// Names to delete for the size cap (oldest-activity first).
    pub evict_for_size: Vec<String>,
    pub remaining_bytes: u64,
}

/// Whether a scanned dir should be age-expired. Live dirs and dirs younger than
/// the TOCTOU floor are exempt; otherwise the retention is ended → `ended`,
/// not-ended → `unrestored`, EXCEPT unknown-liveness not-ended → never (∞), which
/// might be a live-but-unreattached session. `retention = None` models the TS `∞`.
#[must_use]
pub fn should_expire_session_dir(
    is_live: bool,
    age_ms: i64,
    is_ended: bool,
    liveness_unknown: bool,
    thresholds: SessionGcThresholds,
) -> bool {
    if is_live || age_ms < thresholds.min_dir_age_ms {
        return false;
    }
    let retention: Option<i64> = if is_ended {
        Some(thresholds.ended_retention_ms)
    } else if liveness_unknown {
        None
    } else {
        Some(thresholds.unrestored_retention_ms)
    };
    match retention {
        None => false,
        Some(r) => age_ms > r,
    }
}

/// Plan the age-expiry and size-cap eviction over a scanned store. Size eviction is
/// oldest-first and restricted to evictable dirs (ended always; not-ended only when
/// liveness is KNOWN — an unknown-liveness not-ended dir is never evicted for disk).
#[must_use]
pub fn plan_session_history_gc(
    dirs: &[SessionGcPlannerDir],
    now: i64,
    max_total_bytes: u64,
    liveness_unknown: bool,
    live_dir_names: Option<&HashSet<String>>,
    thresholds: SessionGcThresholds,
) -> SessionGcPlan {
    let mut expire = Vec::new();
    let mut eviction_candidates: Vec<&SessionGcPlannerDir> = Vec::new();
    let mut survivor_bytes: u64 = 0;
    for dir in dirs {
        let is_live = live_dir_names.is_some_and(|s| s.contains(&dir.name));
        let age_ms = now - dir.last_activity_ms;
        let exempt = is_live || age_ms < thresholds.min_dir_age_ms;
        if should_expire_session_dir(is_live, age_ms, dir.is_ended, liveness_unknown, thresholds) {
            expire.push(dir.name.clone());
            continue;
        }
        survivor_bytes += dir.total_bytes;
        // Only non-exempt survivors are size-eviction candidates; live/recent dirs
        // are counted toward the total but never evicted.
        if !exempt && (dir.is_ended || !liveness_unknown) {
            eviction_candidates.push(dir);
        }
    }

    let mut evict_for_size = Vec::new();
    let mut remaining_bytes = survivor_bytes;
    if remaining_bytes > max_total_bytes {
        // Stable sort by last activity ascending — ties keep scan order, matching
        // the TS Array.sort stability.
        eviction_candidates.sort_by_key(|d| d.last_activity_ms);
        for dir in eviction_candidates {
            if remaining_bytes <= max_total_bytes {
                break;
            }
            remaining_bytes -= dir.total_bytes;
            evict_for_size.push(dir.name.clone());
        }
    }
    SessionGcPlan { expire, evict_for_size, remaining_bytes }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TH: SessionGcThresholds = SessionGcThresholds {
        min_dir_age_ms: 10,
        ended_retention_ms: 100,
        unrestored_retention_ms: 1000,
    };

    #[test]
    fn expire_decision_covers_every_branch() {
        // live -> never, even when ancient + ended.
        assert!(!should_expire_session_dir(true, 9_999, true, false, TH));
        // TOCTOU floor -> never, even when ended past retention.
        assert!(!should_expire_session_dir(false, 5, true, false, TH));
        // ended past retention -> expire.
        assert!(should_expire_session_dir(false, 200, true, false, TH));
        // ended within retention -> keep.
        assert!(!should_expire_session_dir(false, 50, true, false, TH));
        // not-ended, liveness known, past unrestored retention -> expire.
        assert!(should_expire_session_dir(false, 1_500, false, false, TH));
        // not-ended, liveness UNKNOWN -> never (might be live-unreattached).
        assert!(!should_expire_session_dir(false, 9_999_999, false, true, TH));
    }

    fn d(name: &str, bytes: u64, last: i64, ended: bool) -> SessionGcPlannerDir {
        SessionGcPlannerDir { name: name.into(), total_bytes: bytes, last_activity_ms: last, is_ended: ended }
    }

    #[test]
    fn size_cap_evicts_oldest_first_sparing_live() {
        let live: HashSet<String> = ["L".to_string()].into_iter().collect();
        let dirs = [
            d("L", 200, 100, true), // live -> exempt, counted, never evicted
            d("a", 100, 905, true),
            d("b", 100, 915, true),
        ];
        let plan = plan_session_history_gc(&dirs, 1000, 150, false, Some(&live), TH);
        assert!(plan.expire.is_empty());
        // oldest-first: a(905) then b(915); L spared.
        assert_eq!(plan.evict_for_size, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(plan.remaining_bytes, 200); // only L remains
    }

    /// Shared corpus (`parity-corpus.txt`) — the same cases the TS planner runs.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../parity-corpus.txt");
        let mut checked = 0;
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let case = parse_case(line, idx);
            let plan = plan_session_history_gc(
                &case.dirs,
                case.now,
                case.max_total_bytes,
                case.liveness_unknown,
                case.live_dir_names.as_ref(),
                TH,
            );
            assert_eq!(plan.expire, case.want_expire, "line {}: expire", idx + 1);
            assert_eq!(plan.evict_for_size, case.want_evict, "line {}: evict", idx + 1);
            assert_eq!(plan.remaining_bytes, case.want_remaining, "line {}: remaining", idx + 1);
            checked += 1;
        }
        assert!(checked >= 8, "corpus too small ({checked})");
    }

    struct Case {
        now: i64,
        max_total_bytes: u64,
        liveness_unknown: bool,
        live_dir_names: Option<HashSet<String>>,
        dirs: Vec<SessionGcPlannerDir>,
        want_expire: Vec<String>,
        want_evict: Vec<String>,
        want_remaining: u64,
    }

    fn names(tok: &str) -> Vec<String> {
        if tok == "-" {
            Vec::new()
        } else {
            tok.split(',').map(str::to_string).collect()
        }
    }

    // `<now> <maxBytes> <lu> <live> | <dirs> => <expire> <evict> <remaining>`
    fn parse_case(line: &str, idx: usize) -> Case {
        let (input, output) = line.split_once("=>").unwrap_or_else(|| panic!("line {}: no =>", idx + 1));
        let (config, dirs_s) = input.split_once('|').unwrap_or_else(|| panic!("line {}: no |", idx + 1));
        let mut c = config.split_whitespace();
        let now: i64 = c.next().unwrap().parse().unwrap();
        let max_total_bytes: u64 = c.next().unwrap().parse().unwrap();
        let liveness_unknown = c.next().unwrap() == "1";
        let live_tok = c.next().unwrap();
        let live_dir_names = if liveness_unknown || live_tok == "-" {
            if liveness_unknown { None } else { Some(HashSet::new()) }
        } else {
            Some(live_tok.split(',').map(str::to_string).collect())
        };
        let dirs = dirs_s
            .trim()
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|spec| {
                let mut p = spec.trim().split(':');
                let name = p.next().unwrap().to_string();
                let bytes: u64 = p.next().unwrap().parse().unwrap();
                let last: i64 = p.next().unwrap().parse().unwrap();
                let ended = p.next().unwrap() == "1";
                SessionGcPlannerDir { name, total_bytes: bytes, last_activity_ms: last, is_ended: ended }
            })
            .collect();
        let mut o = output.split_whitespace();
        let want_expire = names(o.next().unwrap());
        let want_evict = names(o.next().unwrap());
        let want_remaining: u64 = o.next().unwrap().parse().unwrap();
        Case { now, max_total_bytes, liveness_unknown, live_dir_names, dirs, want_expire, want_evict, want_remaining }
    }
}
