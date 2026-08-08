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
//! other decision cores: proven equivalent to the TS by `parity-corpus.txt`.
//!
//! The float layer is where the trust-* stack has nothing to say: measured, this
//! crate raises exactly one Level-0 obligation and it is runtime-checked, and the
//! `f64` pipeline that computes the clamp target raises none at all — see
//! `docs/trust/capability-gaps-from-the-smt-purge.md` gap F1. It is covered here by
//! the differential oracle plus the exhaustive band/monotonicity tests, NOT by a
//! hand-written model that declared the target an unbounded `Int` and so could not
//! see the float layer at all.

#![forbid(unsafe_code)]

/// Bytes per GiB — `os.totalmem()` reports bytes; the TS divides by this.
pub const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// Below this reported total, keep Chromium's default (see the module doc). 7.5 not
/// 8 because Linux `MemTotal` excludes reserved RAM, so an 8 GB box reports ~7.7.
pub const RENDERER_HEAP_MIN_TOTAL_GIB: f64 = 7.5;
/// At or above this reported total the band's cap always wins (0.4 PiB is ~10^5×
/// the 4 GB cage), so the sizing short-circuits there. No machine reports this, and
/// bounding the input is what makes the two f64 scalings provably overflow-free.
pub const RENDERER_HEAP_MAX_TOTAL_BYTES: f64 = 1024.0 * 1024.0 * BYTES_PER_GIB;
/// Fraction of total RAM to target for the ceiling before clamping.
pub const RENDERER_HEAP_RAM_FRACTION: f64 = 0.4;
/// Floor of the RAM-tier band (MB).
pub const RENDERER_HEAP_FLOOR_MB: u32 = 3072;
/// Cap of the RAM-tier band (MB) — V8's pointer-compression cage hard limit.
pub const RENDERER_HEAP_CAP_MB: u32 = 4096;
/// `RENDERER_HEAP_CAP_MB / 1024` — the scaled-GiB value at which the band's cap
/// takes over, i.e. the point past which `floor(scaled) * 1024` is clamped away.
const RENDERER_HEAP_CAP_SCALED_GIB: f64 = 4.0;

/// The env override AFTER the TS `parseRendererHeapOverrideMb` has resolved the raw
/// string: an explicit opt-out, an explicit positive MB value, or nothing (fall
/// through to the RAM tiers).
#[derive(Debug, Clone, Copy, Eq)]
pub enum HeapOverride {
    /// `--max-old-space-size` disabled (the TS `'disable'`): keep Chromium's default.
    Disable,
    /// An explicit MB value, returned as-is (the TS returns it WITHOUT clamping).
    Fixed(u32),
    /// No usable override — fall through to the RAM tiers.
    None,
}

impl PartialEq for HeapOverride {
    /// Structurally identical to `#[derive(PartialEq)]`, written by hand because the
    /// derive compares through `&self` and Trust cannot lower an address walk into a
    /// variant-bearing ADT's interior; matching the `Copy` value instead verifies.
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Disable, Self::Disable) | (Self::None, Self::None) => true,
            (Self::Fixed(a), Self::Fixed(b)) => a == b,
            _ => false,
        }
    }
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
            // ONE conjunction, not two early returns: this is the two-sided bound the
            // scalings below are proved against, and Trust only propagates it in this
            // shape. `> 0.0` also rejects NaN, so it is the TS `Number.isFinite(t) &&
            // t > 0` intersected with the short-circuit bound.
            if !(total_memory_bytes > 0.0 && total_memory_bytes < RENDERER_HEAP_MAX_TOTAL_BYTES) {
                // NaN, non-positive and +∞ keep Chromium's default — `f64::is_finite`
                // is absent from Trust's bundle, hence the explicit ∞ test. A finite
                // total at or above the bound gets the cap, which is what the full
                // formula yields there anyway.
                let past_bound = total_memory_bytes >= RENDERER_HEAP_MAX_TOTAL_BYTES
                    && total_memory_bytes < f64::INFINITY;
                return if past_bound { Some(RENDERER_HEAP_CAP_MB) } else { None };
            }
            let total_gib = total_memory_bytes / BYTES_PER_GIB;
            if total_gib < RENDERER_HEAP_MIN_TOTAL_GIB {
                return None;
            }
            // The target is `floor(totalGiB * 0.4) * 1024` clamped to the band, as the
            // TS computes it. Capping the scaled GiB at CAP/1024 first cannot change
            // the clamped result (the clamp discards everything above the cap anyway)
            // and keeps the whole number small enough to floor with an integer cast —
            // f64 `floor`/`max`/`min` are absent from Trust's lowered bundle, the cast
            // and integer `max`/`min` are not.
            let scaled_gib = total_gib * RENDERER_HEAP_RAM_FRACTION;
            let scaled_gib_capped = if scaled_gib > RENDERER_HEAP_CAP_SCALED_GIB {
                RENDERER_HEAP_CAP_SCALED_GIB
            } else {
                scaled_gib
            };
            // Truncation toward zero == floor for a non-negative value.
            let target_mb = (scaled_gib_capped as u32).saturating_mul(1024);
            Some(target_mb.max(RENDERER_HEAP_FLOOR_MB).min(RENDERER_HEAP_CAP_MB))
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

    /// The pre-rewrite float pipeline, kept verbatim as a behavioral oracle.
    fn oracle(total_memory_bytes: f64) -> Option<u32> {
        if !total_memory_bytes.is_finite() || total_memory_bytes <= 0.0 {
            return None;
        }
        let total_gib = total_memory_bytes / BYTES_PER_GIB;
        if total_gib < RENDERER_HEAP_MIN_TOTAL_GIB {
            return None;
        }
        let target_mb = (total_gib * RENDERER_HEAP_RAM_FRACTION).floor() * 1024.0;
        let clamped = target_mb
            .max(f64::from(RENDERER_HEAP_FLOOR_MB))
            .min(f64::from(RENDERER_HEAP_CAP_MB));
        Some(clamped as u32)
    }

    /// The cap-first rewrite must be bit-identical to the float pipeline it replaced
    /// on every input, including the gate edge, the cap crossover and both sides of
    /// every ULP boundary the `* 0.4` rounding can move.
    #[test]
    fn matches_the_pre_rewrite_float_pipeline() {
        let mut cases: Vec<f64> = vec![
            0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MIN, f64::MAX,
            f64::MIN_POSITIVE, 1.0, RENDERER_HEAP_MAX_TOTAL_BYTES,
        ];
        for gib in [0.0_f64, 1.0, 7.4, 7.5, 7.7, 8.0, 9.0, 9.99, 10.0, 10.01, 12.0, 16.0,
                    32.0, 64.0, 128.0, 1024.0, 65536.0, 1_048_575.0, 1_048_576.0]
        {
            let bytes = gib * BYTES_PER_GIB;
            // The exact value plus its neighbours: the crossover lives on an ULP.
            cases.extend([
                bytes,
                f64::from_bits(bytes.to_bits().wrapping_sub(1)),
                f64::from_bits(bytes.to_bits().wrapping_add(1)),
                bytes - 1.0,
                bytes + 1.0,
            ]);
        }
        for bytes in cases {
            assert_eq!(
                renderer_heap_ceiling_mb(bytes, HeapOverride::None),
                oracle(bytes),
                "drift at {bytes} bytes"
            );
        }
    }

    /// The rewrite is only equivalent because the band's endpoints are the only
    /// multiples of 1024 it can land on; pin that so a retune cannot silently break it.
    #[test]
    fn band_endpoints_justify_the_cap_first_rewrite() {
        assert_eq!(RENDERER_HEAP_FLOOR_MB % 1024, 0);
        assert_eq!(RENDERER_HEAP_CAP_MB % 1024, 0);
        assert_eq!(RENDERER_HEAP_CAP_SCALED_GIB, f64::from(RENDERER_HEAP_CAP_MB) / 1024.0);
        // The short-circuit bound really is past the cap crossover.
        assert!(
            RENDERER_HEAP_MAX_TOTAL_BYTES / BYTES_PER_GIB * RENDERER_HEAP_RAM_FRACTION
                >= RENDERER_HEAP_CAP_SCALED_GIB
        );
        assert_eq!(
            renderer_heap_ceiling_mb(RENDERER_HEAP_MAX_TOTAL_BYTES, HeapOverride::None),
            Some(RENDERER_HEAP_CAP_MB)
        );
        assert_eq!(renderer_heap_ceiling_mb(f64::MAX, HeapOverride::None), Some(RENDERER_HEAP_CAP_MB));
    }

    /// A dense sweep of the whole RAM tier: the ceiling is always inside the band,
    /// and it never falls as total RAM rises. Replaces `rh1_band_bound` (which
    /// modelled the clamp target as a free unbounded `Int`, so it could say nothing
    /// about the `f64` pipeline or the `as u32` cast that actually produce it),
    /// `rh2_clamp_monotone`, and `rh3_floor_redundant_under_gate`.
    ///
    /// Monotonicity is relational; see
    /// `docs/trust/capability-gaps-from-the-smt-purge.md` gap R1.
    #[test]
    fn ram_tier_is_banded_and_never_falls_as_ram_rises() {
        let mut prev: Option<u32> = None;
        let mut saw_floor = 0;
        let mut saw_cap = 0;
        let mut steps = 0;
        // 1/64 GiB steps across the gate, the crossover and well past both.
        for i in 0..(64 * 64) {
            let gib = i as f64 / 64.0;
            let got = renderer_heap_ceiling_mb(gib * GIB, HeapOverride::None);
            match got {
                None => {
                    // Below the gate only — the default is never re-entered later.
                    assert!(gib < RENDERER_HEAP_MIN_TOTAL_GIB, "default returned at {gib} GiB");
                    assert!(prev.is_none(), "fell back to the default after sizing at {gib} GiB");
                }
                Some(mb) => {
                    steps += 1;
                    assert!(
                        (RENDERER_HEAP_FLOOR_MB..=RENDERER_HEAP_CAP_MB).contains(&mb),
                        "{gib} GiB -> {mb} outside [3072, 4096]"
                    );
                    if let Some(p) = prev {
                        assert!(mb >= p, "ceiling fell from {p} to {mb} at {gib} GiB");
                    }
                    if mb == RENDERER_HEAP_FLOOR_MB {
                        saw_floor += 1;
                    }
                    if mb == RENDERER_HEAP_CAP_MB {
                        saw_cap += 1;
                    }
                    prev = Some(mb);
                }
            }
        }
        assert!(steps > 2_000, "sweep too thin ({steps})");
        // Non-vacuity (the retired `rh_c1`/`rh_c2` controls): both band endpoints are
        // really reached, so the clamp is load-bearing at both ends.
        assert!(saw_floor > 0, "the floor is never active — the clamp is vacuous");
        assert!(saw_cap > 0, "the cap is never active — the clamp is vacuous");
        // Monotonicity continues past the short-circuit bound.
        assert_eq!(
            renderer_heap_ceiling_mb(RENDERER_HEAP_MAX_TOTAL_BYTES * 2.0, HeapOverride::None),
            Some(RENDERER_HEAP_CAP_MB)
        );
    }

    /// Hand-written `PartialEq` must stay derive-equivalent.
    #[test]
    fn heap_override_equality_is_structural() {
        let all = [
            HeapOverride::Disable,
            HeapOverride::None,
            HeapOverride::Fixed(0),
            HeapOverride::Fixed(1),
            HeapOverride::Fixed(u32::MAX),
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(a == b, i == j, "{a:?} vs {b:?}");
            }
        }
        assert_eq!(HeapOverride::Fixed(7), HeapOverride::Fixed(7));
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
