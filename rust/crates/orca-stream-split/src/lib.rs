//! Surrogate-safe split-index primitives for daemon stream chunking.
//!
//! Ported from `src/main/daemon/daemon-stream-data-split.ts`. When a stream data
//! event is too big for the receiver's NDJSON line limit it is sliced into chunks;
//! these two functions choose split indices that never cut a UTF-16 surrogate pair
//! in half (which would corrupt an astral code point — emoji, CJK-ext, …). They
//! operate on UTF-16 code units, exactly as the TS `charCodeAt` does, so parity is
//! bit-exact. The NDJSON-byte-budget binary search that drives them
//! (`splitStreamDataForNdjson`) stays in TS; this is the boundary-safety core.
//!
//! Same E1 pair as the other decision cores: proven equivalent to the TS by
//! `parity-corpus.txt`. Panic-freedom and the index bounds are discharged by the
//! compiler's own verifier (`-Ztrust-verify=on`); the boundary-safety properties
//! are pinned by the exhaustive class tests below.

#![forbid(unsafe_code)]

/// A UTF-16 high surrogate (leading half of an astral pair). Mirrors the TS
/// `isHighSurrogate`.
#[must_use]
pub fn is_high_surrogate(value: u16) -> bool {
    (0xd800..=0xdbff).contains(&value)
}

/// A UTF-16 low surrogate (trailing half of an astral pair). Mirrors the TS
/// `isLowSurrogate`.
#[must_use]
pub fn is_low_surrogate(value: u16) -> bool {
    (0xdc00..=0xdfff).contains(&value)
}

/// Clamp a proposed split at `end` back by one if it would fall between a high
/// surrogate and its following low surrogate. Mirrors the TS
/// `clampToSafeSplitIndex(value, start, end)`: the guard returns `end` unchanged at
/// the string edges (`end <= start` or `end >= len`), otherwise it moves a
/// pair-splitting `end` to `end - 1`.
#[must_use]
pub fn clamp_to_safe_split_index(units: &[u16], start: usize, end: usize) -> usize {
    if end <= start || end >= units.len() {
        return end;
    }
    // The guard above gives `end > start >= 0`, so `end >= 1` and `wrapping_sub` is
    // exact — but it carries no overflow obligation, and `get` states the in-bounds
    // fact the guard already establishes.
    let before = end.wrapping_sub(1);
    if let (Some(&prev), Some(&next)) = (units.get(before), units.get(end)) {
        if is_high_surrogate(prev) && is_low_surrogate(next) {
            return before;
        }
    }
    end
}

/// The next split index at least one past `start`, advanced past a surrogate pair
/// that begins exactly at `start` so a single code point is never left straddling
/// the boundary. Mirrors the TS `nextSafeSplitIndex(value, start)` — guarantees
/// forward progress even when a single astral code point exceeds the byte budget.
#[must_use]
pub fn next_safe_split_index(units: &[u16], start: usize) -> usize {
    // Explicit compare, not `len().min(start + 1)`: `Ord::min` is opaque to Trust and
    // `start + 1` overflows at `usize::MAX`. The branch is the same clamp, and the
    // `start < len` guard makes both indices provably in bounds.
    let len = units.len();
    let next = if start < len { start + 1 } else { len };
    if next < len {
        // `next < len` implies `start + 1 < len`, so `start` is in bounds too; `get`
        // states that to the verifier instead of leaving it to be inferred.
        if let (Some(&prev), Some(&following)) = (units.get(start), units.get(next)) {
            if is_high_surrogate(prev) && is_low_surrogate(following) {
                return next + 1;
            }
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    // 😀 U+1F600 = surrogate pair D83D DE00.
    const PAIR_HI: u16 = 0xd83d;
    const PAIR_LO: u16 = 0xde00;
    const A: u16 = 0x0041;

    #[test]
    fn clamp_moves_a_pair_splitting_index_back() {
        // [HI, LO]: splitting at 1 cuts the pair -> clamp to 0.
        assert_eq!(clamp_to_safe_split_index(&[PAIR_HI, PAIR_LO], 0, 1), 0);
    }

    #[test]
    fn clamp_leaves_safe_indices_alone() {
        // After the whole pair (index 2) is safe.
        assert_eq!(clamp_to_safe_split_index(&[PAIR_HI, PAIR_LO, A], 0, 2), 2);
        // Between two BMP chars is safe.
        assert_eq!(clamp_to_safe_split_index(&[A, A], 0, 1), 1);
    }

    #[test]
    fn clamp_guards_return_end_unchanged() {
        // end <= start.
        assert_eq!(clamp_to_safe_split_index(&[PAIR_HI, PAIR_LO], 2, 2), 2);
        // end >= len (never clamps at the very edge, even mid-pair — the caller has
        // no further data to move to).
        assert_eq!(clamp_to_safe_split_index(&[PAIR_HI, PAIR_LO], 0, 2), 2);
    }

    #[test]
    fn next_skips_a_pair_at_start() {
        // start on the high half -> jump past the whole pair (index 2).
        assert_eq!(next_safe_split_index(&[PAIR_HI, PAIR_LO, A], 0), 2);
    }

    #[test]
    fn next_advances_by_one_otherwise() {
        assert_eq!(next_safe_split_index(&[A, PAIR_HI, PAIR_LO], 0), 1);
        assert_eq!(next_safe_split_index(&[A], 0), 1);
        // start on the low half (not a pair start) -> just +1 (= len here).
        assert_eq!(next_safe_split_index(&[PAIR_HI, PAIR_LO], 1), 2);
    }

    /// Shared corpus (`parity-corpus.txt`) — the same cases the TS clamp/next run.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../parity-corpus.txt");
        let mut checked = 0;
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut tok = line.split_whitespace();
            let op = tok.next().unwrap();
            let units = parse_units(tok.next().unwrap());
            match op {
                "clamp" => {
                    let start: usize = tok.next().unwrap().parse().unwrap();
                    let end: usize = tok.next().unwrap().parse().unwrap();
                    expect_arrow(tok.next(), idx);
                    let want: usize = tok.next().unwrap().parse().unwrap();
                    assert_eq!(
                        clamp_to_safe_split_index(&units, start, end),
                        want,
                        "line {}: clamp",
                        idx + 1
                    );
                }
                "next" => {
                    let start: usize = tok.next().unwrap().parse().unwrap();
                    expect_arrow(tok.next(), idx);
                    let want: usize = tok.next().unwrap().parse().unwrap();
                    assert_eq!(
                        next_safe_split_index(&units, start),
                        want,
                        "line {}: next",
                        idx + 1
                    );
                }
                other => panic!("line {}: unknown op {other}", idx + 1),
            }
            checked += 1;
        }
        assert!(checked >= 8, "corpus too small ({checked})");
    }

    fn expect_arrow(t: Option<&str>, idx: usize) {
        assert_eq!(t, Some("=>"), "line {}: expected =>", idx + 1);
    }

    // ---------------------------------------------------------------------
    // Exhaustive boundary-safety properties.
    //
    // `clamp_to_safe_split_index` / `next_safe_split_index` inspect a code unit
    // ONLY through `is_high_surrogate` / `is_low_surrogate`, which partition
    // `u16` into exactly three classes. So one representative per class makes a
    // sequence enumeration exhaustive up to the predicates, and
    // `surrogate_predicates_partition_all_u16` below discharges the partition
    // itself over all 65_536 values. Together these cover the same quantified
    // domain the retired hand SMT abstracted, over real `usize`/`u16` rather
    // than unbounded `Int`.
    // ---------------------------------------------------------------------

    /// Every `u16` is in exactly one of {high surrogate, low surrogate, neither},
    /// and the two predicates hit precisely their Unicode ranges.
    #[test]
    fn surrogate_predicates_partition_all_u16() {
        for v in 0..=u16::MAX {
            let hi = is_high_surrogate(v);
            let lo = is_low_surrogate(v);
            assert!(!(hi && lo), "{v:#06x} classed as both halves");
            assert_eq!(hi, (0xd800..=0xdbff).contains(&v), "high at {v:#06x}");
            assert_eq!(lo, (0xdc00..=0xdfff).contains(&v), "low at {v:#06x}");
        }
    }

    /// All sequences of length 0..=4 over one representative per surrogate class.
    fn all_class_sequences() -> Vec<Vec<u16>> {
        const CLASSES: [u16; 3] = [A, PAIR_HI, PAIR_LO];
        let mut out = vec![Vec::new()];
        let mut frontier = vec![Vec::<u16>::new()];
        for _ in 0..4 {
            let mut next = Vec::new();
            for seq in &frontier {
                for c in CLASSES {
                    let mut s = seq.clone();
                    s.push(c);
                    next.push(s);
                }
            }
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }

    fn splits_a_pair(units: &[u16], at: usize) -> bool {
        at >= 1
            && at < units.len()
            && is_high_surrogate(units[at - 1])
            && is_low_surrogate(units[at])
    }

    /// CS1 (interior): for any interior split request the clamped index never
    /// falls between a high surrogate and its following low surrogate. This is
    /// the corruption bound the chunker depends on — a split here would emit two
    /// halves of one astral code point.
    ///
    /// CS2: the result always stays in `[start, end]` and only ever moves back by
    /// one, so a caller's slice bounds are preserved and progress is not lost.
    #[test]
    fn clamp_never_returns_an_interior_pair_splitting_index() {
        let mut interior_cases = 0;
        for units in all_class_sequences() {
            let len = units.len();
            for start in 0..=len + 1 {
                for end in 0..=len + 1 {
                    let r = clamp_to_safe_split_index(&units, start, end);
                    // CS2: never leaves [min(start,end), end], never moves more than one.
                    assert!(r == end || r + 1 == end, "{units:?} {start}/{end} -> {r}");
                    assert!(r <= end, "{units:?} {start}/{end} -> {r} above end");
                    if start < end && end < len {
                        interior_cases += 1;
                        assert!(
                            !splits_a_pair(&units, r),
                            "{units:?} start={start} end={end} clamped to {r}, which splits a pair"
                        );
                        assert!(r >= start, "{units:?} {start}/{end} -> {r} below start");
                    }
                }
            }
        }
        assert!(interior_cases > 100, "domain too thin ({interior_cases})");
    }

    /// The edge case the clamp deliberately does NOT fix, pinned so it cannot
    /// drift into a silent corruption: at `end >= len` there is no following unit
    /// to inspect, and at `end <= start` there is no room to move back, so `end`
    /// is returned verbatim. The caller (the byte-budget search) owns that edge.
    #[test]
    fn clamp_returns_edge_requests_verbatim() {
        for units in all_class_sequences() {
            let len = units.len();
            for start in 0..=len + 1 {
                for end in 0..=len + 1 {
                    if end <= start || end >= len {
                        assert_eq!(
                            clamp_to_safe_split_index(&units, start, end),
                            end,
                            "{units:?} {start}/{end} was moved at an edge"
                        );
                    }
                }
            }
        }
    }

    /// NS1 (progress): from any in-range start the next index strictly advances
    /// and never runs past the end. Without strict progress the NDJSON chunker
    /// loops forever on a single over-budget astral code point.
    ///
    /// NS2: a pair beginning exactly at `start` is skipped whole.
    #[test]
    fn next_always_progresses_and_skips_a_whole_pair() {
        let mut progress_cases = 0;
        for units in all_class_sequences() {
            let len = units.len();
            for start in 0..=len + 1 {
                let r = next_safe_split_index(&units, start);
                assert!(r <= len, "{units:?} start={start} -> {r} past len {len}");
                if start < len {
                    progress_cases += 1;
                    assert!(r > start, "{units:?} start={start} -> {r} made no progress");
                }
                // NS2: the whole pair is skipped, never left straddling.
                if start + 1 < len
                    && is_high_surrogate(units[start])
                    && is_low_surrogate(units[start + 1])
                {
                    assert_eq!(r, start + 2, "{units:?} start={start} split a pair");
                }
                // A returned index in the interior is never itself pair-splitting.
                assert!(!splits_a_pair(&units, r), "{units:?} start={start} -> {r} splits a pair");
            }
        }
        assert!(progress_cases > 100, "domain too thin ({progress_cases})");
    }

    /// Comma-separated hex UTF-16 code units, e.g. `d83d,de00,0041`. `_` = empty.
    fn parse_units(s: &str) -> Vec<u16> {
        if s == "_" {
            return Vec::new();
        }
        s.split(',')
            .map(|h| u16::from_str_radix(h, 16).unwrap())
            .collect()
    }
}
