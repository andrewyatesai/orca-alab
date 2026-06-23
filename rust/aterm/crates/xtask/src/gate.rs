// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! The local enforcement gate — aterm's replacement for CI (there is NO CI).
//!
//! Run via `cargo run -p xtask -- gate <check>` (wrapped by `tools/verify.sh`
//! and surfaced as `aterm-dev gate`). Hung off `.githooks/pre-push`, this is the
//! mechanical, fail-closed substitute for a CI required-status-check.
//!
//! The structured checks here are the ones plain shell cannot express:
//!
//! - `drift`: ADVERTISE-vs-IMPLEMENT. Every capability `TerminalCapabilities`
//!   advertises (`field: true` in `aterm_capabilities()`) must have a real
//!   implementation witness in the tree. Fail-closed on unknown capabilities, so
//!   adding a flag without registering a witness is caught. This catches the
//!   `kitty_graphics`/`soft_fonts` "advertised but the payload is discarded" class.
//! - `dormant`: COMPUTED-BUT-UNCONSUMED. Every feature value the engine computes
//!   must have at least one live (non-test) consumer in its required crate.
//!   Catches the `bidi_visual_order_cells`-with-no-renderer class. Entries are
//!   `enforced` once the feature is wired; until then they are reported as
//!   `pending` (the roadmap, in the gate).
//! - `fault`: INJECTED-BUT-UNEXERCISED. Every fault point injected into production
//!   code (`fault::triggered("name")`, M7 FAULT-INJECT) must be armed by some test,
//!   and every armed name must have a real injection site. Keeps the deterministic
//!   fault-injection harness honest — an untested fail-closed path rots silently.
//! - `lint`: clippy `-D warnings` + rustfmt + grep_guard + license headers.
//! - `perf`: MEM-BUDGET retained-heap ceiling (M2); wall-clock baseline deferred.
//! - `linux` (opt-in, NOT in `all`): the codebase must keep compiling for
//!   `x86_64-unknown-linux-gnu` (no macOS-only API sneaks in un-cfg-gated). With
//!   `cargo-zigbuild` on PATH it checks the WHOLE WORKSPACE (zig cc cross-compiles
//!   the zstd C-dep); else the pure-Rust engine. Skips gracefully if that rustup
//!   target is absent. Matches M5's "uname-gated state probe".
//! - `all`: every check above EXCEPT `linux` (which needs the Linux target); what
//!   the pre-push hook runs.
//!
//! See docs/EXCEED_GHOSTTY_PLAN.md.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use crate::{collect_rs_files, workspace_root};

pub(crate) fn run(check: Option<&str>) -> ExitCode {
    let ok = match check {
        Some("drift") => gate_drift(),
        Some("dormant") => gate_dormant(),
        Some("fault") => gate_fault(),
        Some("linux") => gate_linux(),
        Some("lint") => gate_lint(),
        Some("perf") => gate_perf(),
        Some("all") => {
            // Run all; report every failure (don't short-circuit) so one run
            // surfaces the full picture, then fail if any failed.
            let results = [
                ("drift", gate_drift()),
                ("dormant", gate_dormant()),
                ("fault", gate_fault()),
                ("perf", gate_perf()),
                ("lint", gate_lint()),
            ];
            let failed: Vec<&str> = results
                .iter()
                .filter(|(_, ok)| !ok)
                .map(|(n, _)| *n)
                .collect();
            if failed.is_empty() {
                eprintln!("\ngate all: GREEN — drift, dormant, fault, perf, lint all passed.");
                true
            } else {
                eprintln!("\ngate all: FAILED — {}", failed.join(", "));
                false
            }
        }
        other => {
            eprintln!(
                "usage: xtask gate <all|drift|dormant|fault|linux|lint|perf>\n\
                 (unknown check {other:?})"
            );
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------------------
// Source scanning helpers
// ---------------------------------------------------------------------------

/// Is this file a test-only source file (excluded from "implementation" scans)?
fn is_test_file(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/tests/")
        || s.ends_with("_tests.rs")
        || s.contains("/benches/")
        || s.ends_with("/proofs.rs")
        || s.contains("proofs_")
}

/// All non-test `*.rs` files under `crates/`, optionally excluding one file by
/// suffix (e.g. the advertise site itself).
fn impl_source_files(exclude_suffix: Option<&str>) -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = Vec::new();
    let _ = collect_rs_files(&root.join("crates"), &mut files);
    files
        .into_iter()
        .filter(|p| !is_test_file(p))
        .filter(|p| match exclude_suffix {
            Some(suf) => !p.to_string_lossy().ends_with(suf),
            None => true,
        })
        .collect()
}

/// Does any non-test source line under `crates/` contain `needle` (excluding the
/// advertise site `terminal_core.rs`)?
fn needle_present(needle: &str) -> bool {
    for file in impl_source_files(Some("terminal_core.rs")) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // Ignore pure-comment lines so a TODO mention isn't a witness.
        if text
            .lines()
            .any(|l| !l.trim_start().starts_with("//") && l.contains(needle))
        {
            return true;
        }
    }
    false
}

/// Count non-test source lines under `consumer_path` (a file OR a dir) that
/// reference `symbol` as a USE, not its definition. The `fn <symbol>` definition
/// line is excluded so pointing the check at the crate that also DEFINES the
/// symbol still measures real consumers.
fn consumer_count(symbol: &str, consumer_path: &str) -> usize {
    let root = workspace_root();
    let target = root.join(consumer_path);
    let mut files = Vec::new();
    if target.is_file() {
        files.push(target);
    } else {
        let _ = collect_rs_files(&target, &mut files);
    }
    let def_marker = format!("fn {symbol}");
    let mut count = 0;
    for file in files.into_iter().filter(|p| !is_test_file(p)) {
        if let Ok(text) = std::fs::read_to_string(&file) {
            for l in text.lines() {
                let t = l.trim_start();
                if !t.starts_with("//") && l.contains(symbol) && !l.contains(&def_marker) {
                    count += 1;
                }
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// G-DRIFT: advertise-vs-implement
// ---------------------------------------------------------------------------

/// The implementation evidence required for an advertised capability.
enum Proof {
    /// A substring that must appear in non-test source (outside the advertise file).
    Needle(&'static str),
    /// A path (relative to the workspace root) that must exist.
    Path(&'static str),
}

struct Witness {
    cap: &'static str,
    proof: Proof,
    /// What implements it (for the failure message when a `true` flag lacks it).
    desc: &'static str,
}

/// One entry per field of `TerminalCapabilities`. Fail-closed: if
/// `aterm_capabilities()` advertises a `true` capability with NO entry here, the
/// gate fails (a new flag must register its witness). Capabilities advertised
/// `false` are not required to have a live witness (that is the honest state).
const WITNESS_REGISTRY: &[Witness] = &[
    Witness {
        cap: "true_color",
        proof: Proof::Needle("Rgb"),
        desc: "SGR 38;2/48;2 truecolor (handler_sgr.rs)",
    },
    Witness {
        cap: "color_256",
        proof: Proof::Path("crates/aterm-core/src/terminal/color_resolve.rs"),
        desc: "256-color palette resolution",
    },
    Witness {
        cap: "hyperlinks",
        proof: Proof::Needle("fn handle_osc_8"),
        desc: "OSC 8 hyperlinks",
    },
    Witness {
        cap: "sixel_graphics",
        proof: Proof::Path("crates/aterm-sixel"),
        desc: "Sixel DCS decoder crate",
    },
    Witness {
        cap: "iterm_images",
        proof: Proof::Needle("fn handle_osc_1337"),
        desc: "iTerm2 OSC 1337 inline images",
    },
    Witness {
        cap: "kitty_graphics",
        proof: Proof::Needle("fn handle_kitty_command"),
        desc: "Kitty graphics APC 'G' decode + display (KITTY-CORE)",
    },
    Witness {
        cap: "clipboard",
        proof: Proof::Needle("fn handle_osc_52"),
        desc: "OSC 52 clipboard",
    },
    Witness {
        cap: "shell_integration",
        proof: Proof::Path("crates/aterm-shell-integration"),
        desc: "OSC 133/633 shell integration",
    },
    Witness {
        cap: "synchronized_output",
        proof: Proof::Needle("2026"),
        desc: "DEC mode 2026 synchronized output",
    },
    Witness {
        cap: "kitty_keyboard",
        proof: Proof::Path("crates/aterm-core/src/terminal/keyboard_mode.rs"),
        desc: "Kitty keyboard protocol",
    },
    Witness {
        cap: "soft_fonts",
        proof: Proof::Needle("fn handle_decdld"),
        desc: "DRCS/DECDLD soft fonts",
    },
    Witness {
        cap: "unicode",
        proof: Proof::Path("crates/aterm-grapheme"),
        desc: "Unicode grapheme segmentation",
    },
    Witness {
        cap: "bracketed_paste",
        proof: Proof::Needle("2004"),
        desc: "DEC mode 2004 bracketed paste",
    },
    Witness {
        cap: "focus_reporting",
        proof: Proof::Needle("1004"),
        desc: "DEC mode 1004 focus reporting",
    },
    Witness {
        cap: "mouse_tracking",
        proof: Proof::Needle("1000"),
        desc: "DEC mode 1000 mouse tracking",
    },
    Witness {
        cap: "alternate_screen",
        proof: Proof::Needle("1049"),
        desc: "DEC mode 1049 alternate screen",
    },
];

/// Parse `aterm_capabilities()` from `terminal_core.rs`, returning each
/// `field -> advertised(bool)` pair.
fn parse_advertised_caps() -> Result<Vec<(String, bool)>, String> {
    let path = workspace_root().join("crates/aterm-types/src/terminal_core.rs");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    let start = text
        .find("fn aterm_capabilities()")
        .ok_or("aterm_capabilities() not found")?;
    let body = &text[start..];
    let end = body.find('}').unwrap_or(body.len());
    let body = &body[..end];
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        // Match `name: true,` / `name: false,`
        if let Some((name, rest)) = t.split_once(':') {
            let name = name.trim();
            let val = rest.trim().trim_end_matches(',').trim();
            if val == "true" {
                out.push((name.to_string(), true));
            } else if val == "false" {
                out.push((name.to_string(), false));
            }
        }
    }
    Ok(out)
}

fn gate_drift() -> bool {
    eprintln!("=== gate drift (advertise-vs-implement) ===");
    let caps = match parse_advertised_caps() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gate drift: FAILED to parse capabilities: {e}");
            return false;
        }
    };
    if caps.is_empty() {
        eprintln!("gate drift: FAILED — parsed zero capabilities (parser broke?)");
        return false;
    }
    let mut failures = Vec::new();
    for (cap, advertised) in &caps {
        let entry = WITNESS_REGISTRY.iter().find(|w| w.cap == cap);
        match entry {
            None => {
                // Fail-closed only when an UNKNOWN cap is advertised true.
                if *advertised {
                    failures.push(format!(
                        "  '{cap}' is advertised true but has NO witness registered in gate.rs \
                         (add a Witness entry mapping it to its implementation)"
                    ));
                }
            }
            Some(w) if *advertised => {
                let present = match &w.proof {
                    Proof::Needle(n) => needle_present(n),
                    Proof::Path(p) => workspace_root().join(p).exists(),
                };
                if !present {
                    failures.push(format!(
                        "  '{cap}' advertised true but witness MISSING: {} (expected {})",
                        w.desc,
                        match &w.proof {
                            Proof::Needle(n) => format!("source containing `{n}`"),
                            Proof::Path(p) => format!("path {p}"),
                        }
                    ));
                }
            }
            Some(_) => { /* advertised false: no witness required */ }
        }
    }
    let advertised_true = caps.iter().filter(|(_, a)| *a).count();
    if failures.is_empty() {
        eprintln!(
            "gate drift: GREEN — {advertised_true} advertised capabilities all have implementation witnesses; \
             {} honestly advertised false.",
            caps.len() - advertised_true
        );
        true
    } else {
        eprintln!("gate drift: FAILED — advertise-vs-implement drift:");
        for f in &failures {
            eprintln!("{f}");
        }
        eprintln!(
            "  Fix: implement the capability, or set its `aterm_capabilities()` flag false \
             (honest non-advertisement)."
        );
        false
    }
}

// ---------------------------------------------------------------------------
// G-DORMANT: computed-but-unconsumed
// ---------------------------------------------------------------------------

struct DormantWatch {
    feature: &'static str,
    /// The symbol the engine computes (the producer).
    producer: &'static str,
    /// The crate dir whose non-test code MUST reference the producer.
    consumer_path: &'static str,
    /// `true` once the feature is wired: the gate then FAILS if the consumer
    /// disappears. `false` while the wiring is still pending (reported, not failed).
    enforced: bool,
}

/// Features that must not be computed-and-dropped. Flip `enforced` to true as
/// each is wired (the milestone that wires it owns the flip).
const DORMANCY_REGISTRY: &[DormantWatch] = &[
    // M1 WIRE-BIDI: the render snapshot (cell_frame_into) must invoke the
    // visual-reorder pass, so BOTH renderers + the image capture get visual
    // order. Enforced: the gate fails if render_cells.rs stops calling it.
    DormantWatch {
        feature: "bidi visual reorder",
        producer: "apply_bidi_reorder",
        consumer_path: "crates/aterm-core/src/terminal/render_cells.rs",
        enforced: true,
    },
    // M1 WIRE-MODIFIERS: Caps/Num Lock must be folded into the key modifier byte
    // (winit omits lock state). Enforced: the key path must consume lock_modifiers.
    DormantWatch {
        feature: "caps/num lock modifiers",
        producer: "lock_modifiers",
        consumer_path: "crates/aterm-gui/src/app_input.rs",
        enforced: true,
    },
    // WIRE-COLORSCHEME: the engine reports/pushes the OS color scheme (DEC 2031 +
    // DSR ?996n). Feeding it the REAL OS appearance is the GUI's job — pending
    // until M5 WINDOW-THEME-AUTO calls set_color_scheme from NSAppearance.
    DormantWatch {
        feature: "OS color-scheme source",
        producer: "set_color_scheme",
        consumer_path: "crates/aterm-gui/src",
        enforced: false,
    },
    // WIRE-INBAND-SIZE: DEC mode 2048 must emit a report on enable AND on resize.
    // Enforced: the report builder must be called (handler_dec enable + resize).
    DormantWatch {
        feature: "in-band size report (DEC 2048)",
        producer: "push_in_band_size_report",
        consumer_path: "crates/aterm-core/src/terminal",
        enforced: true,
    },
    // OSC 9;4 taskbar progress: the OSC 9 handler must parse it into state.
    // Enforced: handle_osc_9 must consume the ConEmu parser.
    DormantWatch {
        feature: "OSC 9;4 taskbar progress",
        producer: "parse_conemu_taskbar_progress",
        consumer_path: "crates/aterm-core/src/terminal/handler_osc_notify.rs",
        enforced: true,
    },
];

fn gate_dormant() -> bool {
    eprintln!("=== gate dormant (computed-but-unconsumed) ===");
    let mut failures = Vec::new();
    let mut pending = 0;
    for w in DORMANCY_REGISTRY {
        let count = consumer_count(w.producer, w.consumer_path);
        if w.enforced && count == 0 {
            failures.push(format!(
                "  '{}' is DORMANT: `{}` has zero live consumers in {} (computed but never used)",
                w.feature, w.producer, w.consumer_path
            ));
        } else if !w.enforced {
            pending += 1;
            eprintln!(
                "  pending: '{}' (`{}` -> {}): {} consumer(s); not yet enforced",
                w.feature, w.producer, w.consumer_path, count
            );
        }
    }
    if failures.is_empty() {
        eprintln!(
            "gate dormant: GREEN — {} enforced feature(s) consumed, {pending} pending wiring.",
            DORMANCY_REGISTRY.iter().filter(|w| w.enforced).count()
        );
        true
    } else {
        eprintln!("gate dormant: FAILED — features computed but never consumed:");
        for f in &failures {
            eprintln!("{f}");
        }
        false
    }
}

// ---------------------------------------------------------------------------
// G-LINT
// ---------------------------------------------------------------------------

fn run_shell(desc: &str, program: &str, args: &[&str]) -> bool {
    eprintln!("  $ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .status();
    match status {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!("  {desc}: FAILED (exit {:?})", s.code());
            false
        }
        Err(e) => {
            eprintln!("  {desc}: could not run ({e})");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// G-LINUX (M5: the headless engine must stay cross-platform — Linux-clean)
// ---------------------------------------------------------------------------

/// The codebase must keep compiling for Linux, so a macOS-only API never sneaks in
/// un-cfg-gated. Verified by a type-check against the Linux target. When
/// `cargo-zigbuild` is on PATH, it checks the WHOLE WORKSPACE (its `zig cc` shim
/// cross-compiles the zstd C-dep); otherwise it falls back to the pure-Rust engine
/// (`aterm-core --no-default-features`, no C-dep). Gracefully SKIPS (not a failure)
/// when the `x86_64-unknown-linux-gnu` rustup target's std is absent. Opt-in (NOT in
/// `gate all`) — matches the plan's M5 "uname-gated state probe".
/// Is `bin` resolvable on `PATH`?
fn on_path(bin: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn gate_linux() -> bool {
    const TARGET: &str = "x86_64-unknown-linux-gnu";
    let have_zig = on_path("cargo-zigbuild") && on_path("zig");

    let mut cmd = Command::new("cargo");
    cmd.current_dir(workspace_root())
        .arg("check")
        .arg("--target")
        .arg(TARGET);
    if have_zig {
        // zig cc translates the rustc triple cc-rs passes, so the zstd C-dep builds.
        cmd.arg("--workspace");
        cmd.env(format!("CC_{TARGET}"), "cargo-zigbuild zig cc --");
        cmd.env(format!("CXX_{TARGET}"), "cargo-zigbuild zig c++ --");
        eprintln!("=== gate linux (WHOLE WORKSPACE cross-compiles for {TARGET}, via zig cc) ===");
    } else {
        // No C cross-compiler: check the pure-Rust engine (drops the zstd C-dep).
        cmd.args(["-p", "aterm-core", "--no-default-features"]);
        eprintln!(
            "=== gate linux (engine cross-compiles for {TARGET}; install cargo-zigbuild for the full workspace) ==="
        );
    }

    match cmd.output() {
        Ok(o) if o.status.success() => {
            let scope = if have_zig {
                "the whole workspace is"
            } else {
                "the headless engine is"
            };
            eprintln!("gate linux: GREEN — {scope} Linux-clean.");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            // The Linux target's std is not installed here — skip, don't fail.
            if stderr.contains("may not be installed")
                || stderr.contains("can't find crate for `std`")
                || stderr.contains("note: the `x86_64-unknown-linux-gnu` target")
            {
                eprintln!(
                    "gate linux: SKIPPED — rustup target {TARGET} not installed \
                     (`rustup target add {TARGET}`). Not a failure."
                );
                true
            } else {
                eprintln!("gate linux: FAILED — no longer compiles for Linux:");
                eprintln!("{stderr}");
                false
            }
        }
        Err(e) => {
            eprintln!("gate linux: could not run cargo ({e}); skipping.");
            true
        }
    }
}

fn gate_lint() -> bool {
    eprintln!("=== gate lint (clippy -D warnings + rustfmt + guards) ===");
    let mut ok = true;
    ok &= run_shell(
        "clippy",
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    );
    ok &= run_shell("rustfmt", "cargo", &["fmt", "--all", "--", "--check"]);
    // Both guards take the repo root as their argument (as verify.sh passes it).
    let root = workspace_root();
    let root_str = root.to_string_lossy().into_owned();
    // Execute the guards directly so their `#!/usr/bin/env bash` shebang is
    // honored — they use bash-only process substitution and break under `sh`.
    let guard = root.join("tools/grep_guard.sh");
    if guard.exists() {
        ok &= run_shell("grep_guard", &guard.to_string_lossy(), &[&root_str]);
    }
    let license = root.join("tools/license_check.sh");
    if license.exists() {
        ok &= run_shell("license_check", &license.to_string_lossy(), &[&root_str]);
    }
    if ok {
        eprintln!("gate lint: GREEN");
    } else {
        eprintln!("gate lint: FAILED");
    }
    ok
}

// ---------------------------------------------------------------------------
// G-FAULT (M7: every injected fault point must be exercised by a test)
// ---------------------------------------------------------------------------

/// Extract the string-literal first argument of every `marker("…")` call in
/// `text`. For marker `triggered`, returns the names in `fault::triggered("x")`;
/// note `arm` also matches `disarm("x")` (substring) — intentional, both mean a
/// test touches that fault point.
fn extract_call_string_args(text: &str, marker: &str) -> Vec<String> {
    let pat = format!("{marker}(\"");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(&pat) {
        let after = &rest[i + pat.len()..];
        match after.find('"') {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

/// FAULT discipline (M7 FAULT-INJECT): a fault point injected into production code
/// (`fault::triggered("name")`) that no test arms is an untested fail-closed path —
/// dead weight that rots. Conversely a test that arms a name with no injection site
/// is a stale/typo'd fault. Enforce both directions so the harness stays honest.
/// The registry's own self-tests (`fault.rs`) are excluded — they arm synthetic
/// names to test the registry itself, not real injection sites.
fn gate_fault() -> bool {
    eprintln!("=== gate fault (injected-but-unexercised) ===");
    let root = workspace_root();
    let mut files = Vec::new();
    let _ = collect_rs_files(&root.join("crates"), &mut files);

    let mut injected: std::collections::BTreeMap<String, String> = Default::default();
    let mut armed: std::collections::BTreeSet<String> = Default::default();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned();
        if rel.ends_with("aterm-core/src/fault.rs") || rel.ends_with("xtask/src/gate.rs") {
            // The harness's own definition + self-tests, and THIS scanner (whose doc
            // comments + pattern strings mention `triggered("…")` literally).
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !is_test_file(file) {
            for name in extract_call_string_args(&text, "triggered") {
                injected.entry(name).or_insert_with(|| rel.clone());
            }
        }
        // `arm("x")` also catches `disarm("x")`; collect `with_armed("x")` too.
        for name in extract_call_string_args(&text, "arm") {
            armed.insert(name);
        }
        for name in extract_call_string_args(&text, "with_armed") {
            armed.insert(name);
        }
    }

    let mut failures = Vec::new();
    for (name, site) in &injected {
        if !armed.contains(name) {
            failures.push(format!(
                "  fault '{name}' injected at {site} but NO test arms it (untested fail-closed path)"
            ));
        }
    }
    for name in &armed {
        if !injected.contains_key(name) {
            failures.push(format!(
                "  fault '{name}' is armed by a test but has NO injection site (stale/typo'd fault)"
            ));
        }
    }

    if failures.is_empty() {
        eprintln!(
            "gate fault: GREEN — {} fault point(s) injected, all exercised by a test.",
            injected.len()
        );
        true
    } else {
        eprintln!("gate fault: FAILED — fault-injection registry is inconsistent:");
        for f in &failures {
            eprintln!("{f}");
        }
        false
    }
}

// ---------------------------------------------------------------------------
// G-PERF (M2): the DETERMINISTIC memory budget is enforced now; the wall-clock
// throughput baseline (tools/golden/perf-baseline.json) is the remaining piece.
// ---------------------------------------------------------------------------

fn gate_perf() -> bool {
    eprintln!("=== gate perf ===");
    // Both gates are DETERMINISTIC (allocation-based, no wall-clock) so they never
    // flake. They are self-contained in aterm-core (no heavy comparison deps).
    // MEM-BUDGET: retained-heap ceiling. PERF-BASELINE: steady-state processing is
    // allocation-free (catches per-line/per-cell O(n)-allocation regressions).
    let mut ok = run_shell(
        "mem-budget",
        "cargo",
        &["test", "-p", "aterm-core", "--test", "mem_budget"],
    );
    ok &= run_shell(
        "perf-scaling",
        "cargo",
        &["test", "-p", "aterm-core", "--test", "perf_scaling"],
    );
    // The wall-clock THROUGHPUT baseline (tools/golden/perf-baseline.json) stays
    // deferred: flaky on shared/throttled machines, needs median-of-N + a generous
    // threshold. The deterministic allocation guards above are the enforced substrate.
    let baseline = workspace_root().join("tools/golden/perf-baseline.json");
    if baseline.exists() {
        eprintln!("  perf-baseline.json present; wall-clock throughput comparison lands later.");
    } else {
        eprintln!("  (wall-clock throughput baseline deferred; deterministic guards enforced.)");
    }
    if ok {
        eprintln!("gate perf: GREEN — MEM-BUDGET + PERF-BASELINE (allocation) within bounds.");
    } else {
        eprintln!("gate perf: FAILED — perf regression (memory or allocation scaling).");
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::extract_call_string_args;

    #[test]
    fn extracts_triggered_names() {
        let src = r#"if crate::fault::triggered("kitty.chunk_alloc") || x { }"#;
        assert_eq!(
            extract_call_string_args(src, "triggered"),
            vec!["kitty.chunk_alloc".to_string()]
        );
    }

    #[test]
    fn arm_pattern_also_catches_disarm_but_not_with_armed() {
        let src = r#"arm("a"); disarm("b"); with_armed("c", || {});"#;
        // `arm("` is a substring of `disarm("` (intended) but NOT of `with_armed("`.
        let mut got = extract_call_string_args(src, "arm");
        got.sort();
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            extract_call_string_args(src, "with_armed"),
            vec!["c".to_string()]
        );
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(extract_call_string_args("let x = 1;", "triggered").is_empty());
    }
}
