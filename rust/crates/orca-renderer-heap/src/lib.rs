//! Renderer V8 old-space heap-ceiling sizing — a pure RAM-tier clamp-band core.
//!
//! Ported from `src/main/startup/renderer-heap-headroom.ts`. Chromium sizes the
//! renderer heap from a ~RAM/4 heuristic, so a big machine still caps the renderer
//! well below V8's ~4 GB pointer-compression cage and heavy Orca sessions OOM. On
//! machines with the RAM we reclaim that headroom; small machines keep Chromium's
//! default (raising it would trade a clean OOM for OS memory-pressure kills).
//!
//! This is the numeric decision: total RAM → ceiling MB, or `None` to keep the
//! default. The resolved env override is passed in as [`HeapOverride`] — the
//! JS-`Number` string parsing (hex/exponential/whitespace/…) stays in the TS
//! `parseRendererHeapOverrideMb`, out of this core's scope. Same E1 pair as the
//! other decision cores: proven equivalent to the TS by `parity-corpus.txt`, proven
//! correct by `proofs/ay/rh_*.smt2`.

#![forbid(unsafe_code)]

/// Bytes per GiB — `os.totalmem()` reports bytes; the TS divides by this.
pub const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// Below this reported total, keep Chromium's default (see the module doc). 7.5 not
/// 8 because Linux `MemTotal` excludes reserved RAM, so an 8 GB box reports ~7.7.
pub const RENDERER_HEAP_MIN_TOTAL_GIB: f64 = 7.5;
/// Fraction of total RAM to target for the ceiling before clamping.
pub const RENDERER_HEAP_RAM_FRACTION: f64 = 0.4;
/// Floor of the RAM-tier band (MB).
pub const RENDERER_HEAP_FLOOR_MB: u32 = 3072;
/// Cap of the RAM-tier band (MB) — V8's pointer-compression cage hard limit.
pub const RENDERER_HEAP_CAP_MB: u32 = 4096;

/// The env override AFTER the TS `parseRendererHeapOverrideMb` has resolved the raw
/// string: an explicit opt-out, an explicit positive MB value, or nothing (fall
/// through to the RAM tiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapOverride {
    /// `--max-old-space-size` disabled (the TS `'disable'`): keep Chromium's default.
    Disable,
    /// An explicit MB value, returned as-is (the TS returns it WITHOUT clamping).
    Fixed(u32),
    /// No usable override — fall through to the RAM tiers.
    None,
}

/// Renderer V8 old-space ceiling (MB), or `None` to keep Chromium's default.
///
/// Mirrors `computeRendererHeapCeilingMb` post-parse: an explicit override wins
/// (disable → none, a number → that number verbatim, no clamp); otherwise a
/// non-finite/non-positive total or a total below the gate keeps the default, and
/// above the gate the ceiling is `clamp(floor(totalGiB * 0.4) * 1024, [3072, 4096])`.
/// JS `Number` and Rust `f64` are both IEEE-754 doubles, so the division, `* 0.4`,
/// `floor`, and clamp are bit-identical (the parity corpus checks this end to end).
#[must_use]
pub fn renderer_heap_ceiling_mb(total_memory_bytes: f64, override_value: HeapOverride) -> Option<u32> {
    match override_value {
        HeapOverride::Disable => None,
        HeapOverride::Fixed(mb) => Some(mb),
        HeapOverride::None => {
            if !total_memory_bytes.is_finite() || total_memory_bytes <= 0.0 {
                return None;
            }
            let total_gib = total_memory_bytes / BYTES_PER_GIB;
            if total_gib < RENDERER_HEAP_MIN_TOTAL_GIB {
                return None;
            }
            // Whole-number f64 (floor(..) * 1024), clamped, then an exact cast.
            let target_mb = (total_gib * RENDERER_HEAP_RAM_FRACTION).floor() * 1024.0;
            let clamped = target_mb
                .max(f64::from(RENDERER_HEAP_FLOOR_MB))
                .min(f64::from(RENDERER_HEAP_CAP_MB));
            Some(clamped as u32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: f64 = BYTES_PER_GIB;

    #[test]
    fn ram_tier_stays_in_the_band_or_none() {
        for gib in [7.5_f64, 7.7, 8.0, 9.0, 10.0, 12.0, 16.0, 32.0, 128.0] {
            let ceiling = renderer_heap_ceiling_mb(gib * GIB, HeapOverride::None).unwrap();
            assert!(
                (RENDERER_HEAP_FLOOR_MB..=RENDERER_HEAP_CAP_MB).contains(&ceiling),
                "{gib} GiB -> {ceiling} out of [3072, 4096]"
            );
        }
    }

    #[test]
    fn gate_keeps_default_below_min() {
        assert_eq!(renderer_heap_ceiling_mb(7.0 * GIB, HeapOverride::None), None);
        assert_eq!(renderer_heap_ceiling_mb(6.0 * GIB, HeapOverride::None), None);
        assert_eq!(renderer_heap_ceiling_mb(0.0, HeapOverride::None), None);
        assert_eq!(renderer_heap_ceiling_mb(-1.0, HeapOverride::None), None);
        assert_eq!(renderer_heap_ceiling_mb(f64::NAN, HeapOverride::None), None);
        assert_eq!(renderer_heap_ceiling_mb(f64::INFINITY, HeapOverride::None), None);
    }

    #[test]
    fn override_precedence() {
        // Disable always wins, even on a big machine.
        assert_eq!(renderer_heap_ceiling_mb(32.0 * GIB, HeapOverride::Disable), None);
        // A fixed value is returned verbatim — NOT clamped to the RAM-tier band.
        assert_eq!(renderer_heap_ceiling_mb(8.0 * GIB, HeapOverride::Fixed(5000)), Some(5000));
        assert_eq!(renderer_heap_ceiling_mb(8.0 * GIB, HeapOverride::Fixed(2000)), Some(2000));
        assert_eq!(renderer_heap_ceiling_mb(0.0, HeapOverride::Fixed(3500)), Some(3500));
    }

    #[test]
    fn known_points() {
        assert_eq!(renderer_heap_ceiling_mb(8.0 * GIB, HeapOverride::None), Some(3072));
        assert_eq!(renderer_heap_ceiling_mb(9.0 * GIB, HeapOverride::None), Some(3072));
        assert_eq!(renderer_heap_ceiling_mb(10.0 * GIB, HeapOverride::None), Some(4096));
        assert_eq!(renderer_heap_ceiling_mb(16.0 * GIB, HeapOverride::None), Some(4096));
    }

    /// Shared corpus (`parity-corpus.txt`) — the same oracle the TS
    /// `computeRendererHeapCeilingMb` runs in its own test.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../parity-corpus.txt");
        let mut checked = 0;
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: `<bytes> <override> => <ceiling|null>`
            let (lhs, rhs) = line
                .split_once("=>")
                .unwrap_or_else(|| panic!("line {}: missing =>", idx + 1));
            let mut lt = lhs.split_whitespace();
            let bytes: f64 = lt.next().unwrap().parse().unwrap();
            let override_value = match lt.next().unwrap() {
                "none" => HeapOverride::None,
                "disable" => HeapOverride::Disable,
                n => HeapOverride::Fixed(n.parse().unwrap()),
            };
            let want = rhs.trim();
            let got = renderer_heap_ceiling_mb(bytes, override_value);
            let got_s = got.map_or_else(|| "null".to_string(), |v| v.to_string());
            assert_eq!(got_s, want, "line {}: {bytes} {override_value:?}", idx + 1);
            checked += 1;
        }
        assert!(checked >= 12, "corpus too small ({checked} rows)");
    }
}
