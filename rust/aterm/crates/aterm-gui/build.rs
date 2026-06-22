// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// build.rs — stamp build provenance into the binary as compile-time env vars,
// surfaced at runtime by `src/build_info.rs` (the About panel + `aterm-ctl version`).
//
//   ATERM_GIT_COMMIT  short commit the binary was built from (+ "-dirty" when the
//                     working tree had uncommitted changes); "unknown" w/o git.
//   ATERM_BUILD_TIME  UTC build timestamp (RFC3339), or "unknown".
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

    // Monotonic build number = commit depth (`git rev-list --count HEAD`). A later
    // commit (a descendant) always has strictly more ancestors, so the build number
    // increments EXACTLY when the build is from a newer commit, and is stable across
    // rebuilds of the same commit — the canonical CFBundleVersion. "0" without git.
    let build_number = run("git", &["rev-list", "--count", "HEAD"]).unwrap_or_else(|| "0".into());
    println!("cargo:rustc-env=ATERM_BUILD_NUMBER={build_number}");

    // Build timestamp (UTC, RFC3339). Honour SOURCE_DATE_EPOCH for reproducible
    // builds when set; otherwise stamp the current wall clock.
    let build_time = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(epoch) if !epoch.is_empty() => run("date", &["-u", "-r", &epoch, "+%Y-%m-%dT%H:%M:%SZ"]),
        _ => run("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
    }
    .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ATERM_BUILD_TIME={build_time}");

    // Re-stamp when HEAD moves (new commit / checkout) so the commit stays current.
    // The workspace `.git` is two levels up from this crate manifest.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
}
