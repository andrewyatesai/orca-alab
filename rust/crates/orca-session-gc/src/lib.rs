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
        // `match`, not `is_some_and`: identical, but the closure is a separate
        // verification unit that inherits the same unlowerable `HashSet::contains`,
        // so it doubles the diagnostics for no gain.
        let is_live = match live_dir_names {
            Some(live) => live.contains(&dir.name),
            None => false,
        };
        // Saturating throughout this loop: `now`, the mtimes and the byte counts all
        // arrive from the scanner, so every plain `+`/`-` here is a reachable
        // overflow (Trust refutes all three). Saturation preserves the ordering the
        // decisions read, so no plan changes for a well-formed scan.
        let age_ms = now.saturating_sub(dir.last_activity_ms);
        let exempt = is_live || age_ms < thresholds.min_dir_age_ms;
        if should_expire_session_dir(is_live, age_ms, dir.is_ended, liveness_unknown, thresholds) {
            expire.push(dir.name.clone());
            continue;
        }
        survivor_bytes = survivor_bytes.saturating_add(dir.total_bytes);
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
            remaining_bytes = remaining_bytes.saturating_sub(dir.total_bytes);
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

    /// A scan whose numbers span the whole i64/u64 range must still produce a plan
    /// instead of panicking — the age subtraction, the survivor-byte sum and the
    /// eviction decrement all overflowed here before they were made saturating.
    #[test]
    fn extreme_scan_values_do_not_overflow() {
        let dirs = [
            d("huge1", u64::MAX, i64::MIN, true),
            d("huge2", u64::MAX, i64::MIN + 1, true),
        ];
        let plan = plan_session_history_gc(&dirs, i64::MAX, 0, false, None, TH);
        // Both are ancient and ended, so both age-expire and nothing survives.
        assert_eq!(plan.expire, vec!["huge1".to_string(), "huge2".to_string()]);
        assert!(plan.evict_for_size.is_empty());
        assert_eq!(plan.remaining_bytes, 0);

        // Same magnitudes, but inside the ended retention so they survive expiry and
        // become eviction candidates: the survivor sum saturates instead of wrapping.
        let dirs = [d("a", u64::MAX, 950, true), d("b", u64::MAX, 940, true)];
        let plan = plan_session_history_gc(&dirs, 1_000, 0, false, None, TH);
        assert!(plan.expire.is_empty());
        // Oldest-first, and the first eviction already reaches the budget.
        assert_eq!(plan.evict_for_size, vec!["b".to_string()]);
        assert_eq!(plan.remaining_bytes, 0);
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

    // -------------------------------------------------------------------
    // Exhaustive retention/eviction safety.
    //
    // Beside the ay bundle, which models bytes and clocks as unbounded `Int`
    // and covers only the expiry PREDICATE plus an abstract one-step eviction:
    // these enumerate whole stores and check the real planner over real
    // `i64`/`u64`, including the sort, the loop and the saturation.
    // -------------------------------------------------------------------

    /// Independent restatement of the TS retention rule, written from the spec
    /// rather than from the implementation, so a change to either side shows up.
    fn expire_oracle(
        is_live: bool,
        age_ms: i64,
        is_ended: bool,
        liveness_unknown: bool,
        th: SessionGcThresholds,
    ) -> bool {
        if is_live {
            return false; // a live session is never touched
        }
        if age_ms < th.min_dir_age_ms {
            return false; // TOCTOU floor
        }
        match (is_ended, liveness_unknown) {
            (true, _) => age_ms > th.ended_retention_ms,
            // Not ended and we cannot tell whether it is live: retention is ∞.
            (false, true) => false,
            (false, false) => age_ms > th.unrestored_retention_ms,
        }
    }

    /// Every branch of the expiry decision over the full boolean cross product and
    /// every retention boundary, including the i64 extremes the unbounded-`Int`
    /// model of the ex1/ex2/ex3 theorems cannot state.
    #[test]
    fn expire_decision_matches_the_spec_on_every_branch_and_boundary() {
        let ages = [
            i64::MIN,
            -1,
            0,
            TH.min_dir_age_ms - 1,
            TH.min_dir_age_ms,
            TH.min_dir_age_ms + 1,
            TH.ended_retention_ms - 1,
            TH.ended_retention_ms,
            TH.ended_retention_ms + 1,
            TH.unrestored_retention_ms - 1,
            TH.unrestored_retention_ms,
            TH.unrestored_retention_ms + 1,
            i64::MAX,
        ];
        let mut expired = 0;
        let mut kept = 0;
        for &age in &ages {
            for is_live in [false, true] {
                for is_ended in [false, true] {
                    for lu in [false, true] {
                        let got = should_expire_session_dir(is_live, age, is_ended, lu, TH);
                        assert_eq!(
                            got,
                            expire_oracle(is_live, age, is_ended, lu, TH),
                            "age={age} live={is_live} ended={is_ended} unknown={lu}"
                        );
                        // The privacy bound itself, stated directly.
                        if got {
                            assert!(!is_live, "expired a LIVE session (age={age})");
                            assert!(age >= TH.min_dir_age_ms, "expired below the TOCTOU floor");
                            assert!(is_ended || !lu, "expired an unknown-liveness live-capable dir");
                            expired += 1;
                        } else {
                            kept += 1;
                        }
                    }
                }
            }
        }
        // Non-vacuity: the enumeration really reaches both verdicts. This is the
        // in-source form of the `ex_c1_*_sat` control.
        assert!(expired > 0 && kept > 0, "vacuous: {expired} expired, {kept} kept");
    }

    /// Every store over 3 dirs drawn from a class-complete state space, crossed
    /// with both liveness modes and several budgets. Checks the planner's whole
    /// safety contract at once.
    #[test]
    fn planner_never_deletes_a_protected_dir_and_reaches_the_budget() {
        const NOW: i64 = 1_000;
        // Ages 1500 / 100 / 5 select the three retention regimes: past both
        // retentions, inside the ended retention, and below the TOCTOU floor.
        const LASTS: [i64; 3] = [-500, 900, 995];
        const BYTES: [u64; 2] = [0, 100];

        let mut states = Vec::new();
        for &b in &BYTES {
            for &l in &LASTS {
                for ended in [false, true] {
                    for live in [false, true] {
                        states.push((b, l, ended, live));
                    }
                }
            }
        }

        let mut saw_expire = 0;
        let mut saw_evict = 0;
        let mut saw_spared_live = 0;
        let mut saw_spared_unknown = 0;
        let mut stores = 0;

        for a in &states {
            for b in &states {
                for c in &states {
                    let triple = [*a, *b, *c];
                    for lu in [false, true] {
                        let live: HashSet<String> = triple
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| s.3)
                            .map(|(i, _)| i.to_string())
                            .collect();
                        // `liveness_unknown` means the live set is unavailable.
                        let live_arg = if lu { None } else { Some(&live) };
                        let dirs: Vec<SessionGcPlannerDir> = triple
                            .iter()
                            .enumerate()
                            .map(|(i, s)| d(&i.to_string(), s.0, s.1, s.2))
                            .collect();
                        for budget in [0u64, 100, 150, 10_000] {
                            stores += 1;
                            let plan =
                                plan_session_history_gc(&dirs, NOW, budget, lu, live_arg, TH);
                            let deleted: Vec<&String> =
                                plan.expire.iter().chain(plan.evict_for_size.iter()).collect();

                            // (1) No name is deleted twice, and the two lists are disjoint.
                            let unique: HashSet<&String> = deleted.iter().copied().collect();
                            assert_eq!(unique.len(), deleted.len(), "double delete: {plan:?}");

                            for (i, s) in triple.iter().enumerate() {
                                let name = i.to_string();
                                let (bytes, last, ended, is_live_flag) = *s;
                                let is_live = !lu && is_live_flag;
                                let age = NOW - last;
                                let deleted_here = deleted.iter().any(|n| **n == name);

                                // (2) THE PRIVACY BOUND: a live dir is never deleted.
                                if is_live {
                                    assert!(!deleted_here, "deleted LIVE dir {name}: {plan:?}");
                                    if bytes > 0 {
                                        saw_spared_live += 1;
                                    }
                                }
                                // (3) Nothing below the TOCTOU floor is ever deleted.
                                if age < TH.min_dir_age_ms {
                                    assert!(!deleted_here, "deleted dir {name} below the floor");
                                }
                                // (4) An unknown-liveness, not-ended dir is never
                                // deleted by EITHER path — it may be a live session
                                // that has not reattached yet.
                                if lu && !ended {
                                    assert!(
                                        !deleted_here,
                                        "deleted unknown-liveness unrestored dir {name}"
                                    );
                                    saw_spared_unknown += 1;
                                }
                                // (5) Size eviction never touches an exempt dir.
                                if plan.evict_for_size.contains(&name) {
                                    assert!(!is_live && age >= TH.min_dir_age_ms);
                                    assert!(ended || !lu, "evicted a live-capable dir {name}");
                                }
                            }

                            // (6) Byte accounting is exact (all values here are far
                            // from the saturation edges).
                            let survivors: u64 = triple
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| !plan.expire.contains(&i.to_string()))
                                .map(|(_, s)| s.0)
                                .sum();
                            let evicted: u64 = triple
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| plan.evict_for_size.contains(&i.to_string()))
                                .map(|(_, s)| s.0)
                                .sum();
                            assert_eq!(
                                plan.remaining_bytes,
                                survivors - evicted,
                                "byte accounting: {plan:?}"
                            );

                            // (7) EVICTION IS MINIMAL: nothing is evicted while
                            // already under budget, and every eviction but the last
                            // was taken while still over budget.
                            if survivors <= budget {
                                assert!(plan.evict_for_size.is_empty(), "over-evicted: {plan:?}");
                            }
                            // Tightness: the loop stops at the FIRST prefix that
                            // reaches the budget, so undoing the last eviction must
                            // put the store back over it. This is what makes
                            // "reaches the budget" a bound rather than a licence
                            // to delete everything.
                            if let Some(last) = plan.evict_for_size.last() {
                                let last_bytes = triple[last.parse::<usize>().unwrap()].0;
                                assert!(
                                    plan.remaining_bytes + last_bytes > budget,
                                    "evicted {last} unnecessarily (budget {budget}): {plan:?}"
                                );
                            }

                            // (8) EVICTION REACHES THE BUDGET when the evictable
                            // bytes suffice — the liveness property
                            // `ev2_reaches_budget_when_enough` asserts abstractly.
                            let evictable: u64 = triple
                                .iter()
                                .enumerate()
                                .filter(|(i, s)| {
                                    let name = i.to_string();
                                    let is_live = !lu && s.3;
                                    let age = NOW - s.1;
                                    !plan.expire.contains(&name)
                                        && !is_live
                                        && age >= TH.min_dir_age_ms
                                        && (s.2 || !lu)
                                })
                                .map(|(_, s)| s.0)
                                .sum();
                            if survivors.saturating_sub(evictable) <= budget {
                                assert!(
                                    plan.remaining_bytes <= budget,
                                    "did not reach budget {budget}: {plan:?}"
                                );
                            }

                            // (9) OLDEST-FIRST: the eviction order is non-decreasing
                            // in last-activity.
                            let order: Vec<i64> = plan
                                .evict_for_size
                                .iter()
                                .map(|n| triple[n.parse::<usize>().unwrap()].1)
                                .collect();
                            assert!(
                                order.windows(2).all(|w| w[0] <= w[1]),
                                "eviction not oldest-first: {order:?}"
                            );

                            if !plan.expire.is_empty() {
                                saw_expire += 1;
                            }
                            if !plan.evict_for_size.is_empty() {
                                saw_evict += 1;
                            }
                        }
                    }
                }
            }
        }

        // Non-vacuity (the in-source form of the `ev_c1_*_sat` control):
        // the enumeration really reaches expiry, eviction, and both spare paths.
        assert!(stores > 10_000, "domain too thin ({stores} stores)");
        assert!(saw_expire > 0, "no store ever expired");
        assert!(saw_evict > 0, "no store ever evicted for size");
        assert!(saw_spared_live > 0, "no live dir was ever spared under pressure");
        assert!(saw_spared_unknown > 0, "no unknown-liveness dir was ever spared");
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
