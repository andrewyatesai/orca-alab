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
//! `parity-corpus.txt`, proven correct by `proofs/ay/{cs,ns}_*.smt2`.

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
    let prev = units[end - 1];
    let next = units[end];
    if is_high_surrogate(prev) && is_low_surrogate(next) {
        end - 1
    } else {
        end
    }
}

/// The next split index at least one past `start`, advanced past a surrogate pair
/// that begins exactly at `start` so a single code point is never left straddling
/// the boundary. Mirrors the TS `nextSafeSplitIndex(value, start)` — guarantees
/// forward progress even when a single astral code point exceeds the byte budget.
#[must_use]
pub fn next_safe_split_index(units: &[u16], start: usize) -> usize {
    let next = units.len().min(start + 1);
    if next < units.len()
        && is_high_surrogate(units[start])
        && is_low_surrogate(units[next])
    {
        return next + 1;
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
