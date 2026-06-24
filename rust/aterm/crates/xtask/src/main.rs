// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! aterm build-graph tasks — the ALWAYS-RUN nodes of TRUST_VACUITY_GATE §2.
//!
//! Two subcommands, both independent of any one crate's `cargo test` binary
//! (finding 5 — "the teeth are there, the wiring isn't"):
//!
//!   * `harness-manifest` (§2.1 / finding 1a): enumerate every REAL
//!     `#[kani::proof] fn` across the workspace `crates/` and write a
//!     `HarnessManifest` JSON to `target/trust/harness-manifest.json` in the exact
//!     shape `trust-ir spec-link --harness-manifest` expects
//!     (`{"harnesses":[{"name","span"}]}`). This is the data trust-ir's L1 resolves
//!     `proof_name` against (the standalone IR has no compiler/DefId view, so the
//!     manifest must be produced HERE and handed to spec-link).
//!
//!   * `spec-link` (§2.5 / finding 5): the always-run cross-reference node. It (1)
//!     regenerates the manifest, (2) builds the anchor graph from the EMBEDDED models +
//!     external ISOLATION `.tla` + the cross-crate-collected `proof_anchor!`s
//!     (aterm-scrollback / aterm-grid, linked with `spec-anchors` ON in THIS binary),
//!     (3) lowers it with `aterm_spec::ir::lower_to_ir` (now emitting `proof` lines),
//!     and (4) shells `trust-ir spec-link --harness-manifest … --require-manifest`,
//!     asserting exit 0. So the proof-name resolution (L1), mandatory projection
//!     (L2), and Ob.1/Ob.3/Ob.4 are enforced by a build-graph node, not only by
//!     `cargo test -p aterm-gui`.
//!
//! NOTE on scope: the in-SOURCE `path_confine` / `window_routing` `#[cfg(test)]`
//! anchors collect ONLY in aterm-gui's test binary (inventory sees only LINKED object
//! code), so the FULL ISOLATION + window_routing in-source set is enforced by the
//! `spec_xref_gate` there. THIS node enforces the embedded models, the external
//! ISOLATION specs, and the cross-crate PROOF anchors — i.e. the L1 teeth that the
//! manifest unlocks — independent of that test binary.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use aterm_spec::tla_check::TlaSpec;
use aterm_spec::xref::{self, SpecModule};

mod gate;
mod perf;

// Force the proof-anchor-bearing rlibs into the link graph: `inventory` only collects
// `submit!`s from LINKED object code, and a bin that references NOTHING from these
// crates would let the linker drop their rlibs (and the `spec_proof_anchors` module's
// `proof_anchor!` consts with them). The `extern crate` declarations + the
// `force_link` reference below pull them in so `xref::proof_anchors()` sees the kani
// half cross-crate (the same mechanism aterm-gui's test binary relies on).
extern crate aterm_grid;
extern crate aterm_scrollback;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    match cmd {
        Some("harness-manifest") => match write_harness_manifest() {
            Ok(path) => {
                eprintln!("xtask harness-manifest: wrote {}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("xtask harness-manifest FAILED: {e}");
                ExitCode::FAILURE
            }
        },
        Some("spec-link") => spec_link(),
        Some("gate") => gate::run(args.get(2).map(String::as_str)),
        _ => {
            eprintln!(
                "usage: xtask <harness-manifest|spec-link|gate <check>>\n\
                 \n\
                 harness-manifest  enumerate #[kani::proof] fns -> target/trust/harness-manifest.json\n\
                 spec-link         lower the anchor graph + run `trust-ir spec-link --require-manifest`\n\
                 gate <check>      local enforcement gate (NO CI): all|drift|dormant|lint|perf\n\
                                   see docs/EXCEED_GHOSTTY_PLAN.md"
            );
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace layout
// ---------------------------------------------------------------------------

/// The workspace root (the dir that holds `crates/` and `target/`). `xtask`'s
/// manifest dir is `<root>/crates/xtask`, so the root is two levels up.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .and_then(Path::parent) // <root>
        .expect("xtask manifest dir has a grandparent (the workspace root)")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// harness-manifest (finding 1a)
// ---------------------------------------------------------------------------

/// One `#[kani::proof]` harness: its fn name + a `file:line` span (opaque to L1,
/// which matches only on `name`).
struct HarnessEntry {
    name: String,
    span: String,
}

/// Enumerate every `#[kani::proof] fn <name>` under the workspace `crates/` and write
/// the `HarnessManifest` JSON. Returns the path written. The scan is a line walk:
/// a `#[kani::proof]` attribute line arms the next `fn <ident>` (allowing intervening
/// `#[kani::…]` / `#[cfg(kani)]` attribute lines), exactly as the harnesses are
/// authored. Names are de-duplicated (a harness name is the L1 key, unique per build).
fn write_harness_manifest() -> std::io::Result<PathBuf> {
    let root = workspace_root();
    let mut entries: Vec<HarnessEntry> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files)?;
    files.sort();
    for file in &files {
        let text = std::fs::read_to_string(file)?;
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned();
        let lines: Vec<&str> = text.lines().collect();
        let mut armed = false;
        for (i, raw) in lines.iter().enumerate() {
            let line = raw.trim_start();
            if line.starts_with("#[kani::proof") {
                armed = true;
                continue;
            }
            if armed {
                // Skip further attribute lines (#[kani::should_panic], #[cfg(kani)], …)
                // and blank/comment lines between the attr and the fn.
                if line.starts_with("#[") || line.is_empty() || line.starts_with("//") {
                    continue;
                }
                if let Some(name) = parse_fn_name(line) {
                    if seen.insert(name.clone()) {
                        entries.push(HarnessEntry {
                            name,
                            span: format!("{rel}:{}:1", i + 1),
                        });
                    }
                    armed = false;
                } else {
                    // A non-attr, non-fn line after the attr — not a harness; disarm.
                    armed = false;
                }
            }
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let out_dir = root.join("target").join("trust");
    std::fs::create_dir_all(&out_dir)?;
    let out_path = out_dir.join("harness-manifest.json");
    std::fs::write(&out_path, render_manifest_json(&entries))?;
    eprintln!("xtask: {} kani harness(es) enumerated", entries.len());
    Ok(out_path)
}

/// Recursively collect `*.rs` files under `dir`, skipping `target/` and any
/// hidden directory (name starting with `.`). The hidden-dir skip matters
/// because this tool's own workflow worktrees live under `.claude/worktrees/`
/// (each a full repo checkout); descending into them would count every source
/// file N+1 times. This matches the `grep -rn` semantics the count gates cite
/// (BSD/GNU `grep -r .` does not descend into dot-directories), so a developer
/// with active worktrees gets the same counts as a clean checkout.
pub(crate) fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect_rs_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Extract `<ident>` from a `(pub )?(unsafe )?fn <ident>…` line; `None` otherwise.
fn parse_fn_name(line: &str) -> Option<String> {
    let mut rest = line;
    for kw in [
        "pub ",
        "pub(crate) ",
        "unsafe ",
        "const ",
        "async ",
        "extern ",
    ] {
        if let Some(s) = rest.strip_prefix(kw) {
            rest = s.trim_start();
        }
    }
    let rest = rest.strip_prefix("fn ")?;
    let ident: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() { None } else { Some(ident) }
}

/// Render the `HarnessManifest` JSON in the documented shape. Hand-rolled (no serde
/// dep): each `name`/`span` is JSON-escaped (both are plain identifiers / file paths
/// here, but escape defensively).
fn render_manifest_json(entries: &[HarnessEntry]) -> String {
    let mut s = String::from("{\n  \"harnesses\": [");
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "\n    {{ \"name\": {}, \"span\": {} }}",
            json_str(&e.name),
            json_str(&e.span)
        ));
    }
    if !entries.is_empty() {
        s.push_str("\n  ");
    }
    s.push_str("]\n}\n");
    s
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// spec-link (finding 5) — the always-run cross-reference node
// ---------------------------------------------------------------------------

/// Touch a symbol from each proof-anchor-bearing crate so the linker retains its rlib
/// (and the `spec_proof_anchors` `inventory::submit!` consts). `black_box` defeats
/// dead-code elimination of the reference itself.
fn force_link() {
    std::hint::black_box(aterm_scrollback::DEFAULT_LINE_LIMIT);
    std::hint::black_box(aterm_grid::MAX_GRID_ROWS);
}

fn spec_link() -> ExitCode {
    force_link();
    // (1) Regenerate the manifest the L1 resolution needs.
    let manifest = match write_harness_manifest() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("xtask spec-link: could not write harness manifest: {e}");
            return ExitCode::FAILURE;
        }
    };

    // (2) Build the anchor graph from THIS binary's linked object code: every embedded
    // model + every external ISOLATION `.tla` + the cross-crate `proof_anchor!`s.
    let mut modules: Vec<SpecModule> = xref::model_registry()
        .into_iter()
        .map(SpecModule::Embedded)
        .collect();
    let dir = aterm_spec_models::specs_dir();
    let mut external = 0usize;
    for entry in std::fs::read_dir(&dir).expect("read aterm-spec-models specs/") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() || path.extension().and_then(|e| e.to_str()) != Some("tla") {
            continue;
        }
        let spec = TlaSpec::parse_file(&path)
            .unwrap_or_else(|e| panic!("failed to parse external spec {path:?}: {e}"));
        modules.push(SpecModule::External(spec));
        external += 1;
    }

    let refs: Vec<_> = xref::refinements().collect();
    let waivers: Vec<_> = xref::waivers().collect();
    let proofs: Vec<_> = xref::proof_anchors().collect();
    eprintln!(
        "xtask spec-link: anchor graph — {} module(s) ({} external ISOLATION), {} refinement(s), \
         {} waiver(s), {} proof anchor(s)",
        modules.len(),
        external,
        refs.len(),
        waivers.len(),
        proofs.len()
    );
    assert!(
        !proofs.is_empty(),
        "xtask spec-link: ZERO proof anchors collected — the cross-crate `proof_anchor!` \
         inventory (aterm-scrollback / aterm-grid with `spec-anchors`) did not link. The L1 \
         proof-name teeth would be untested."
    );

    // (3) Lower to a byte-conforming `.trust_irtxt` (now emitting `proof` lines).
    let module_txt =
        aterm_spec::ir::lower_to_ir("aterm_xtask_spec_link", &modules, &refs, &waivers, &proofs);
    let out_dir = workspace_root().join("target").join("trust");
    std::fs::create_dir_all(&out_dir).expect("mk target/trust");
    let ir_path = out_dir.join("xtask-spec-link.trust_irtxt");
    std::fs::write(&ir_path, &module_txt).expect("write .trust_irtxt");

    // (4) Shell `trust-ir spec-link --harness-manifest … --require-manifest`.
    let trust_ir = match find_trust_ir() {
        Some(p) => p,
        None => {
            eprintln!(
                "xtask spec-link: VERIFICATION GATE — `trust-ir` not found; build it at \
                 ~/trust/first-party/trust-ir/target/release/trust-ir (or put it on PATH). The \
                 always-run spec-link node FAILS rather than silently skipping."
            );
            return ExitCode::FAILURE;
        }
    };
    let out = Command::new(&trust_ir)
        .arg("spec-link")
        // aterm emits TEXT (`lower_to_ir`); trust-ir 0.2.0 maps the `.trust_ir`
        // extension to BINARY, so pin the format explicitly.
        .arg("--format")
        .arg("text")
        .arg(&ir_path)
        .arg("--harness-manifest")
        .arg(&manifest)
        .arg("--require-manifest")
        .output()
        .unwrap_or_else(|e| panic!("failed to run {trust_ir:?} spec-link: {e}"));
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("--- trust-ir spec-link (xtask always-run node) ---\n{report}");
    if out.status.success() {
        eprintln!(
            "xtask spec-link: GREEN — trust-ir certified Ob.1/Ob.3/Ob.4 + L2 (projection) + L1 \
             (every proof_name resolved against the manifest) over the embedded models, external \
             ISOLATION specs, and {} proof anchor(s).",
            proofs.len()
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("xtask spec-link: FAILED — trust-ir rejected the lowered module (see above).");
        ExitCode::FAILURE
    }
}

/// Locate `trust-ir` by the same canonical-path search the gate uses.
fn find_trust_ir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        for rel in [
            "trust/first-party/trust-ir/target/release/trust-ir",
            "trust-ir/target/release/trust-ir",
        ] {
            let p = PathBuf::from(&home).join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let out = Command::new("sh")
        .arg("-c")
        .arg("command -v trust-ir")
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}
