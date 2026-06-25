// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! `aterm-dev` — one discoverable, AI-friendly front door to every dev/ops
//! utility script in the aterm workspace.
//!
//! This binary deliberately does NOT reimplement any of the underlying
//! (battle-tested) shell logic — codesign / sips / notarytool / cargo-deny /
//! kani / codex etc. Each subcommand simply resolves the repo root, locates the
//! existing script, and execs it via [`std::process::Command`], forwarding all
//! extra arguments and propagating the script's exit code. The value here is
//! discoverability: a single, grouped, polished `--help` that an AI (or human)
//! can read to learn what operational levers exist.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace version, threaded through from Cargo at build time.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A single dev/ops subcommand: a name, a one-line description, the relative
/// path (from the repo root) of the script it wraps, and the help group it
/// belongs to.
struct Sub {
    name: &'static str,
    about: &'static str,
    script: &'static str,
    group: Group,
}

/// Help groupings, in display order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    PackageRelease,
    QualityVerify,
    Setup,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::PackageRelease => "Package & Release",
            Group::QualityVerify => "Quality & Verify",
            Group::Setup => "Setup",
        }
    }

    /// Display order for the groups.
    const ORDER: [Group; 3] = [Group::PackageRelease, Group::QualityVerify, Group::Setup];
}

/// The full registry of subcommands. Adding a new dev script is a one-line
/// edit here.
const SUBS: &[Sub] = &[
    Sub {
        name: "visual-judge",
        about: "LLM-as-Judge visual loop over aterm introspection",
        script: "tools/visual-judge/visual-judge.sh",
        group: Group::QualityVerify,
    },
    Sub {
        name: "build-app",
        about: "Assemble the macOS app bundle (dist/aterm.app)",
        script: "apps/aterm-mac/build-app.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "make-dmg",
        about: "Package the .app into a distributable .dmg",
        script: "apps/aterm-mac/make-dmg.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "notarize",
        about: "Apple-notarize and staple the artifact",
        script: "apps/aterm-mac/notarize.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "release",
        about: "Full pipeline: sign -> dmg -> notarize",
        script: "apps/aterm-mac/release.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "prepare-release",
        about: "Bump version, roll the changelog, commit + tag a release",
        script: "tools/prepare-release.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "gen-appcast",
        about: "Emit the aterm-appcast.toml in-app-update manifest for a built DMG",
        script: "tools/gen-appcast.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "preflight-release",
        about: "Run the updater's own accept gate (codesign/Team-ID/spctl/monotonic) before publishing",
        script: "tools/preflight-release.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "extract-changelog",
        about: "Print the CHANGELOG.md notes for a version (used as release notes)",
        script: "tools/extract-changelog.sh",
        group: Group::PackageRelease,
    },
    Sub {
        name: "audit",
        about: "Supply-chain audit via cargo-deny",
        script: "scripts/audit-supply-chain.sh",
        group: Group::QualityVerify,
    },
    Sub {
        name: "verify-proofs",
        about: "Opt-in Kani formal-proof verification",
        script: "scripts/verify-kani-proofs.sh",
        group: Group::QualityVerify,
    },
    Sub {
        name: "setup-trust",
        about: "Stand up the trust-mc checker",
        script: "scripts/setup-trust-mc.sh",
        group: Group::Setup,
    },
];

fn main() {
    // Skip argv[0] (our own program name).
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => {
            print_help();
            std::process::exit(0);
        }
        Some("-h") | Some("--help") | Some("help") => {
            print_help();
            std::process::exit(0);
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("aterm-dev {VERSION}");
            std::process::exit(0);
        }
        Some(cmd) => {
            let Some(sub) = SUBS.iter().find(|s| s.name == cmd) else {
                eprintln!("aterm-dev: unknown command {cmd} (try --help)");
                std::process::exit(2);
            };
            // Everything after the subcommand name is forwarded verbatim to the
            // underlying script (so `aterm-dev visual-judge --judges claude`
            // reaches the script as `--judges claude`).
            let forwarded = &args[1..];
            std::process::exit(run_script(sub, forwarded));
        }
    }
}

/// Resolve the script path, exec it forwarding `forwarded`, and return the exit
/// code to propagate. On any dispatch failure (no repo root, missing /
/// non-executable script, failure to spawn) prints a clear error and returns a
/// non-zero code.
fn run_script(sub: &Sub, forwarded: &[String]) -> i32 {
    let root = match repo_root() {
        Some(r) => r,
        None => {
            eprintln!(
                "aterm-dev: could not locate the workspace root (no Cargo.toml with [workspace] \
                 found walking up, and `git rev-parse` failed)"
            );
            return 1;
        }
    };

    let script = root.join(sub.script);
    if !script.is_file() {
        eprintln!(
            "aterm-dev: script for `{}` not found at {}",
            sub.name,
            script.display()
        );
        return 1;
    }
    if !is_executable(&script) {
        eprintln!(
            "aterm-dev: script for `{}` is not executable: {} (try `chmod +x`)",
            sub.name,
            script.display()
        );
        return 1;
    }

    let status = Command::new(&script)
        .args(forwarded)
        // Run scripts from the repo root so their own relative paths resolve.
        .current_dir(&root)
        .status();

    match status {
        Ok(s) => {
            // Prefer the script's own exit code; fall back to 1 if terminated
            // by a signal (no code available).
            s.code().unwrap_or(1)
        }
        Err(e) => {
            eprintln!("aterm-dev: failed to execute {}: {e}", script.display());
            1
        }
    }
}

/// Locate the workspace root robustly. First walk up from the current
/// directory (and the executable's directory) looking for a `Cargo.toml` that
/// declares `[workspace]`; if that fails, fall back to `git rev-parse
/// --show-toplevel`.
fn repo_root() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir()
        && let Some(r) = find_workspace_root(&cwd)
    {
        return Some(r);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && let Some(r) = find_workspace_root(dir)
    {
        return Some(r);
    }
    // Fallback: ask git.
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

/// Walk up from `start`, returning the first ancestor containing a `Cargo.toml`
/// that contains a `[workspace]` table.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let manifest = dir.join("Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&manifest)
            && contents
                .lines()
                .any(|l| l.trim_start().starts_with("[workspace]"))
        {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// Best-effort executable check (owner/group/other execute bit) on Unix.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// On non-Unix, existence is the best we can do.
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Print the polished, grouped top-level help. This is the primary deliverable
/// an AI reads to discover the available operational levers.
fn print_help() {
    println!("aterm-dev {VERSION} — one discoverable front door to all aterm dev/ops scripts");
    println!();
    println!("USAGE:");
    println!("    aterm-dev <command> [args...]");
    println!();

    // Width for aligning the one-line descriptions.
    let name_width = SUBS.iter().map(|s| s.name.len()).max().unwrap_or(0);

    for group in Group::ORDER {
        println!("{}:", group.title());
        for sub in SUBS.iter().filter(|s| s.group == group) {
            println!(
                "    {:<width$}  {}",
                sub.name,
                sub.about,
                width = name_width
            );
        }
        println!();
    }

    println!("Other:");
    println!(
        "    {:<width$}  Print this help",
        "--help, -h",
        width = name_width
    );
    println!(
        "    {:<width$}  Print the workspace version",
        "--version, -V",
        width = name_width
    );
    println!();
    println!("Each command wraps the existing project script and forwards your arguments to it.");
    println!("Run `aterm-dev <command> --help` to forward to that script's own help/usage.");
}
