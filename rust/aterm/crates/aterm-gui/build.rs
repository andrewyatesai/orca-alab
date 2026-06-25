// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates
//
// build.rs — stamp build provenance into the binary as compile-time env vars,
// surfaced at runtime by `src/build_info.rs` (the About panel + `aterm-ctl version`).
//
//   ATERM_GIT_COMMIT   short commit the binary was built from (+ "-dirty" when the
//                      working tree had uncommitted changes); "unknown" w/o git.
//   ATERM_BUILD_NUMBER explicit monotonic release counter (workspace-root
//                      `build_number`, bumped by tools/prepare-release.sh); falls
//                      back to commit depth, then "0", when the file is absent.
//   ATERM_BUILD_TIME   UTC build timestamp (RFC3339), or "unknown".
//
// All probes are best-effort: a missing `git`/`date` degrades to "unknown" rather
// than failing the build (so a source tarball without a .git still compiles).

use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    // Git commit (short, 12 hex) + a "-dirty" suffix when the tree isn't clean.
    let commit =
        run("git", &["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = run("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let commit = if commit != "unknown" && dirty {
        format!("{commit}-dirty")
    } else {
        commit
    };
    println!("cargo:rustc-env=ATERM_GIT_COMMIT={commit}");

    // Monotonic build number: an EXPLICIT release counter in the workspace-root
    // `build_number` file (bumped by tools/prepare-release.sh) — NOT commit depth.
    // Depth is topology-dependent: rebases, squashes, shallow clones and divergent
    // branches make it collide or even regress, which would break the in-app
    // updater's strictly-monotonic "apply only if greater" gate. The file is the
    // canonical CFBundleVersion and the value the updater compares. Falls back to
    // commit depth, then "0", when the file is absent (e.g. a .git-less tarball).
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| format!("{d}/../.."))
        .unwrap_or_else(|_| "../..".into());
    let build_number = std::fs::read_to_string(format!("{root}/build_number"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .or_else(|| run("git", &["rev-list", "--count", "HEAD"]))
        .unwrap_or_else(|| "0".into());
    println!("cargo:rustc-env=ATERM_BUILD_NUMBER={build_number}");

    // Build timestamp (UTC, RFC3339). Honour SOURCE_DATE_EPOCH for reproducible
    // builds when set; otherwise stamp the current wall clock.
    let build_time = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(epoch) if !epoch.is_empty() => run("date", &["-u", "-r", &epoch, "+%Y-%m-%dT%H:%M:%SZ"]),
        _ => run("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
    }
    .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ATERM_BUILD_TIME={build_time}");

    // Re-stamp when HEAD moves (new commit / checkout) or the build counter changes.
    // The workspace `.git` is two levels up from this crate manifest.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    println!("cargo:rerun-if-changed=../../build_number");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
}
