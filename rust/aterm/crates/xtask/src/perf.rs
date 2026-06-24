// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Wall-clock THROUGHPUT baseline for `gate perf` (PERF-WALLCLOCK-BASELINE lane).
//!
//! aterm is a NO-CI, MULTI-MACHINE repo: m3 and m7 are different-speed boxes that
//! share one committed `tools/golden/perf-baseline.json`, and a gate run may land
//! on a throttled laptop. The #1 requirement is therefore NON-FLAKINESS — the gate
//! must catch a CATASTROPHIC throughput regression (an algorithmic blow-up, a
//! debug-build slip, lock contention) while NEVER spuriously failing on a normal
//! or slower-than-baseline machine.
//!
//! How it stays robust:
//!
//! MEASURE in a release subprocess. The `aterm-bench` `perf_harness` example feeds
//! a deterministic ~32 MiB representative VT workload (plain text + SGR + CSI +
//! scrolling) through `Terminal::process` and prints a single JSON line of
//! throughput. We spawn it with `cargo run --release` so timing is the shipped
//! build, never this debug-built xtask.
//!
//! MEDIAN-OF-N + WARMUP. The harness discards warmup iters, then takes the MEDIAN
//! of N>=5 timed iters — one scheduler hiccup cannot move the median.
//!
//! GENEROUS RATIO. The gate FAILS only when `median < baseline * RATIO`.
//! [`PASS_RATIO`] is 0.45: a machine running at 45% of the baseline box still
//! passes, tolerating ~2.2x slowdown from a slower core or thermal throttle. That
//! is far wider than real machine variance yet still trips on the kind of 10x+
//! collapse a debug build or O(n^2) parser would cause. See [`PASS_RATIO`].
//!
//! NEVER BLOCK A FRESH CHECKOUT. With no baseline file present the gate REPORTS the
//! measured throughput and PASSES. The strict comparison is engaged only when a
//! committed baseline exists.

use std::path::Path;
use std::process::Command;

use crate::workspace_root;

/// The pass threshold as a fraction of the recorded baseline median. The gate
/// fails iff `measured_median < baseline_median * PASS_RATIO`.
///
/// WHY 0.45: the baseline is recorded on one machine (m3) and checked on others
/// (m7, throttled laptops). A factor of 0.45 means a box can run at 45% of the
/// baseline's speed — i.e. be ~2.2x slower — and still pass. Measured m3-vs-m7
/// and throttled-vs-cool spreads sit comfortably inside ~1.5x, so 0.45 has a wide
/// margin against false positives while still catching a CATASTROPHIC regression:
/// a debug-build slip or an algorithmic blow-up costs 5x-50x, dropping the ratio
/// far below 0.45. The deterministic allocation gates (mem_budget, perf_scaling)
/// remain the precise, zero-flake guards; this is the coarse wall-clock floor.
pub(crate) const PASS_RATIO: f64 = 0.45;

/// Parsed throughput report emitted (as one JSON line on stdout) by the
/// `aterm-bench` `perf_harness` example, and the shape persisted to the golden
/// baseline. Field-for-field identical so a recorded baseline round-trips.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PerfReport {
    pub median_mbps: f64,
    pub min_mbps: f64,
    pub max_mbps: f64,
    pub workload_bytes: u64,
    pub n: u64,
    pub warmup: u64,
}

/// The verdict of comparing a fresh measurement against a baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// No baseline on disk — report the number, never block (fresh checkout).
    NoBaseline,
    /// `measured >= baseline * PASS_RATIO`.
    Pass,
    /// `measured < baseline * PASS_RATIO` — a catastrophic regression.
    Fail,
}

/// Pure threshold comparison (unit-tested). `baseline` is the recorded median MB/s;
/// `measured` the fresh median MB/s. Returns the minimum MB/s that would pass too,
/// so callers can print the floor. A non-finite or non-positive baseline is treated
/// as "no usable baseline" -> [`Verdict::NoBaseline`] (never blocks).
pub(crate) fn compare(baseline: f64, measured: f64, ratio: f64) -> (Verdict, f64) {
    if !baseline.is_finite() || baseline <= 0.0 {
        return (Verdict::NoBaseline, 0.0);
    }
    let floor = baseline * ratio;
    // `>=` so a measurement EXACTLY at the floor passes (boundary is inclusive).
    if measured >= floor {
        (Verdict::Pass, floor)
    } else {
        (Verdict::Fail, floor)
    }
}

/// Extract a numeric field from the harness's flat JSON object. The harness emits
/// a known, flat shape (`{"k":v,...}`) so a dependency-free scan suffices: find
/// `"key"`, skip to the `:`, then parse the run of number characters. Returns
/// `None` if the key is absent or the value isn't a finite number.
fn json_number(src: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\"");
    let start = src.find(&needle)? + needle.len();
    let after_colon = src[start..].find(':')? + start + 1;
    let tail = src[after_colon..].trim_start();
    let end = tail
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(tail.len());
    let num: f64 = tail[..end].parse().ok()?;
    num.is_finite().then_some(num)
}

/// Parse the harness's JSON line into a [`PerfReport`]. Pure (unit-tested).
pub(crate) fn parse_report(json: &str) -> Option<PerfReport> {
    Some(PerfReport {
        median_mbps: json_number(json, "median_mbps")?,
        min_mbps: json_number(json, "min_mbps")?,
        max_mbps: json_number(json, "max_mbps")?,
        workload_bytes: json_number(json, "workload_bytes")? as u64,
        n: json_number(json, "n")? as u64,
        warmup: json_number(json, "warmup")? as u64,
    })
}

/// Render a [`PerfReport`] as the committed baseline JSON (pretty, with metadata).
/// Hand-rolled so xtask gains no serde dependency. `ratio` is recorded for humans;
/// the live gate uses [`PASS_RATIO`] (the source of truth), not this echoed copy.
pub(crate) fn baseline_json(r: &PerfReport, ratio: f64) -> String {
    format!(
        "{{\n  \"_comment\": \"aterm wall-clock throughput baseline (PERF-WALLCLOCK-BASELINE). Median-of-N MB/s of Terminal::process over a deterministic ~32 MiB mixed VT workload. Re-record with `ATERM_PERF_RECORD=1 cargo run -p xtask -- gate perf` (or `gate perf --record`). The gate fails only if measured median < median_mbps * pass_ratio; pass_ratio is generous to tolerate multi-machine/throttle variance.\",\n  \"median_mbps\": {:.3},\n  \"min_mbps\": {:.3},\n  \"max_mbps\": {:.3},\n  \"workload_bytes\": {},\n  \"n\": {},\n  \"warmup\": {},\n  \"pass_ratio\": {:.3}\n}}\n",
        r.median_mbps, r.min_mbps, r.max_mbps, r.workload_bytes, r.n, r.warmup, ratio,
    )
}

/// Path to the committed golden baseline.
pub(crate) fn baseline_path() -> std::path::PathBuf {
    workspace_root().join("tools/golden/perf-baseline.json")
}

/// Run the release `perf_harness` and parse its throughput report. The harness is
/// built+run via `cargo run --release` so the engine is the optimized build (a
/// debug build would itself read as a "regression" — which is, deliberately, what
/// we want the gate to catch if someone ships one).
pub(crate) fn measure() -> Result<PerfReport, String> {
    eprintln!("  $ cargo run --release -q -p aterm-bench --example perf_harness");
    let out = Command::new("cargo")
        .args([
            "run",
            "--release",
            "-q",
            "-p",
            "aterm-bench",
            "--example",
            "perf_harness",
        ])
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("could not spawn perf_harness: {e}"))?;
    // Surface the harness's human line (stderr) for the gate log.
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if line.contains("perf_harness:") {
            eprintln!("  {line}");
        }
    }
    if !out.status.success() {
        return Err(format!(
            "perf_harness exited {:?}\n{stderr}",
            out.status.code()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.contains("median_mbps"))
        .ok_or_else(|| format!("perf_harness produced no JSON report:\n{stdout}"))?;
    parse_report(line).ok_or_else(|| format!("could not parse perf_harness JSON: {line}"))
}

/// Should the gate (re)write the baseline this run? Either `ATERM_PERF_RECORD` is
/// set to a truthy value, or `--record` appears anywhere on the argv.
pub(crate) fn record_requested() -> bool {
    let env_truthy = std::env::var("ATERM_PERF_RECORD")
        .map(|v| {
            let v = v.trim();
            !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false"))
        })
        .unwrap_or(false);
    env_truthy || std::env::args().any(|a| a == "--record")
}

/// The wall-clock throughput sub-gate. Returns `true` (PASS) on success, including
/// the "no baseline / record / fresh checkout" cases that must NEVER block.
pub(crate) fn gate_throughput() -> bool {
    let report = match measure() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  throughput: FAILED to measure — {e}");
            return false;
        }
    };

    let path = baseline_path();

    if record_requested() {
        let json = baseline_json(&report, PASS_RATIO);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, &json) {
            Ok(()) => {
                eprintln!(
                    "  throughput: RECORDED baseline {:.1} MB/s -> {}",
                    report.median_mbps,
                    path.display()
                );
                return true;
            }
            Err(e) => {
                eprintln!(
                    "  throughput: FAILED to write baseline {}: {e}",
                    path.display()
                );
                return false;
            }
        }
    }

    compare_against_baseline(&path, &report)
}

/// Read the baseline at `path` (if any) and apply the threshold. Split out so the
/// pure decision (read -> parse -> [`compare`]) is exercised without spawning cargo.
fn compare_against_baseline(path: &Path, report: &PerfReport) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!(
            "  throughput: no baseline at {} — REPORT-ONLY: {:.1} MB/s (median of {}). \
             PASS (a fresh checkout is never blocked; record with ATERM_PERF_RECORD=1).",
            path.display(),
            report.median_mbps,
            report.n,
        );
        return true;
    };
    let Some(base) = parse_report(&text) else {
        // A malformed baseline must not silently block; report and pass.
        eprintln!(
            "  throughput: baseline {} is unparseable — REPORT-ONLY: {:.1} MB/s. PASS.",
            path.display(),
            report.median_mbps,
        );
        return true;
    };

    let (verdict, floor) = compare(base.median_mbps, report.median_mbps, PASS_RATIO);
    match verdict {
        Verdict::NoBaseline => {
            eprintln!(
                "  throughput: baseline median non-positive — REPORT-ONLY: {:.1} MB/s. PASS.",
                report.median_mbps
            );
            true
        }
        Verdict::Pass => {
            eprintln!(
                "  throughput: GREEN — {:.1} MB/s >= floor {:.1} MB/s (baseline {:.1} MB/s x {:.2}).",
                report.median_mbps, floor, base.median_mbps, PASS_RATIO,
            );
            true
        }
        Verdict::Fail => {
            eprintln!(
                "  throughput: FAILED — {:.1} MB/s < floor {:.1} MB/s (baseline {:.1} MB/s x {:.2}). \
                 A catastrophic throughput regression (debug build? algorithmic blow-up? lock contention?).",
                report.median_mbps, floor, base.median_mbps, PASS_RATIO,
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(median: f64) -> PerfReport {
        PerfReport {
            median_mbps: median,
            min_mbps: median * 0.9,
            max_mbps: median * 1.1,
            workload_bytes: 32 << 20,
            n: 7,
            warmup: 2,
        }
    }

    #[test]
    fn compare_passes_above_floor() {
        let (v, floor) = compare(1000.0, 500.0, 0.45);
        assert_eq!(v, Verdict::Pass);
        assert!((floor - 450.0).abs() < 1e-9);
    }

    #[test]
    fn compare_fails_below_floor() {
        let (v, floor) = compare(1000.0, 449.0, 0.45);
        assert_eq!(v, Verdict::Fail);
        assert!((floor - 450.0).abs() < 1e-9);
    }

    #[test]
    fn compare_boundary_is_inclusive_pass() {
        // EXACTLY at the floor must PASS (>= floor), never flake at the edge.
        let (v, _) = compare(1000.0, 450.0, 0.45);
        assert_eq!(v, Verdict::Pass);
        // A hair below fails.
        let (v2, _) = compare(1000.0, 449.999, 0.45);
        assert_eq!(v2, Verdict::Fail);
    }

    #[test]
    fn compare_catastrophic_regression_fails() {
        // A 10x collapse (debug build / O(n^2)) is far below any generous floor.
        let (v, _) = compare(3000.0, 300.0, 0.45);
        assert_eq!(v, Verdict::Fail);
    }

    #[test]
    fn compare_faster_machine_passes() {
        // A faster box (2x baseline) trivially passes.
        let (v, _) = compare(1000.0, 2000.0, 0.45);
        assert_eq!(v, Verdict::Pass);
    }

    #[test]
    fn compare_nonpositive_baseline_is_no_baseline() {
        assert_eq!(compare(0.0, 1000.0, 0.45).0, Verdict::NoBaseline);
        assert_eq!(compare(-5.0, 1000.0, 0.45).0, Verdict::NoBaseline);
        assert_eq!(compare(f64::NAN, 1000.0, 0.45).0, Verdict::NoBaseline);
    }

    #[test]
    fn parse_report_round_trips_through_baseline_json() {
        let r = report(1234.5);
        let json = baseline_json(&r, PASS_RATIO);
        let back = parse_report(&json).expect("parse the json we just wrote");
        assert!((back.median_mbps - r.median_mbps).abs() < 1e-3);
        assert!((back.min_mbps - r.min_mbps).abs() < 1e-3);
        assert!((back.max_mbps - r.max_mbps).abs() < 1e-3);
        assert_eq!(back.workload_bytes, r.workload_bytes);
        assert_eq!(back.n, r.n);
        assert_eq!(back.warmup, r.warmup);
    }

    #[test]
    fn parse_report_reads_harness_line_shape() {
        // The exact one-line shape the harness prints on stdout.
        let line = "{\"median_mbps\":3142.000,\"min_mbps\":3000.500,\"max_mbps\":3300.250,\"workload_bytes\":33554432,\"n\":7,\"warmup\":2}";
        let r = parse_report(line).expect("parse harness stdout");
        assert!((r.median_mbps - 3142.0).abs() < 1e-3);
        assert!((r.min_mbps - 3000.5).abs() < 1e-3);
        assert!((r.max_mbps - 3300.25).abs() < 1e-3);
        assert_eq!(r.workload_bytes, 33_554_432);
        assert_eq!(r.n, 7);
        assert_eq!(r.warmup, 2);
    }

    #[test]
    fn json_number_handles_negative_and_missing() {
        assert_eq!(json_number("{\"a\":-1.5}", "a"), Some(-1.5));
        assert_eq!(json_number("{\"a\":1.0}", "b"), None);
        assert_eq!(json_number("{\"a\":}", "a"), None);
    }

    #[test]
    fn parse_report_rejects_incomplete_json() {
        // Missing fields -> None (won't be mistaken for a valid baseline).
        assert!(parse_report("{\"median_mbps\":100.0}").is_none());
    }

    #[test]
    fn compare_against_missing_baseline_passes_report_only() {
        // A path that does not exist => report-only PASS (fresh checkout).
        let missing = workspace_root().join("tools/golden/__no_such_perf_baseline__.json");
        assert!(super::compare_against_baseline(&missing, &report(1234.5)));
    }
}
