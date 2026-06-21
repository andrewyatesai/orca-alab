// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! P0 regression test: the daily-driver CLI must run a real shell through the
//! PROTECTED spawn seam (`aterm_pty::spawn_shell`, NOT raw `forkpty`/`execvp`) and
//! stay fully functional — given a command it produces the command's OUTPUT and
//! exits with the shell's status. Complements the static guard `A6` (no
//! `libc::forkpty` in `aterm-cli/src`) with a behavioral check that the protected
//! spawn actually works end-to-end.

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn cli_runs_a_command_through_the_protected_spawn_and_exits_cleanly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aterm"))
        .env("SHELL", "/bin/sh") // a known POSIX shell — env-independent
        .env_remove("ATERM_CONTAINMENT_MODE") // default User mode: no sandbox, fast
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the aterm CLI binary");

    // Feed a command whose output PROVES the shell evaluated it (the arithmetic
    // `$((6*7))` becomes 42 only if a real shell ran it — the PTY echo of the input
    // line still shows the literal `$((6*7))`), then exit. Dropping stdin after the
    // write delivers EOF so the shell runs to its own exit.
    child
        .stdin
        .take()
        .expect("aterm stdin")
        .write_all(b"echo ATERM_P0_MARKER_$((6*7))\nexit\n")
        .expect("write to aterm stdin");

    let out = child.wait_with_output().expect("wait for aterm");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ATERM_P0_MARKER_42"),
        "the shell did not evaluate the command through the protected spawn; stdout={stdout:?}"
    );
    assert!(
        out.status.success(),
        "aterm must exit with the shell's success status; got {:?}",
        out.status
    );
}

/// `ATERM_CONTAINMENT_MODE=containment` wraps the spawn in `sandbox-exec` (deny
/// network + credential/private-data reads). A basic shell command must STILL run
/// under the sandbox — the OS confinement must not break normal shell operation.
/// macOS-only (Seatbelt `sandbox-exec` is the actuated path).
#[cfg(target_os = "macos")]
#[test]
fn cli_runs_under_the_os_sandbox_in_containment_mode() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aterm"))
        .env("SHELL", "/bin/sh")
        .env("ATERM_CONTAINMENT_MODE", "containment")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the aterm CLI binary in containment mode");
    child
        .stdin
        .take()
        .expect("aterm stdin")
        .write_all(b"echo ATERM_SANDBOXED_$((3+4))\nexit\n")
        .expect("write to aterm stdin");
    let out = child.wait_with_output().expect("wait for aterm");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ATERM_SANDBOXED_7"),
        "shell must run under sandbox-exec in containment mode; stdout={stdout:?}"
    );
    assert!(
        out.status.success(),
        "the sandboxed shell must still exit success; got {:?}",
        out.status
    );
}

/// Security: `ATERM_CONTAINMENT_MODE` is attacker-influenceable. A MALFORMED value
/// must FAIL CLOSED to Containment (the most restrictive mode) — never silently
/// fall through to the unconfined `User` default. The binary still spawns and runs
/// (Containment is confined, not a refusal-to-start), but in the confined mode, and
/// it announces the fallback rather than silently swallowing the garbage. Platform-
/// independent (the fallback path is the mode-parse logic; on non-macOS Containment
/// simply has no actuated OS sandbox, but the mode is still confined).
#[test]
fn malformed_containment_mode_fails_closed_not_open() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aterm"))
        .env("SHELL", "/bin/sh")
        .env("ATERM_CONTAINMENT_MODE", "definitely-not-a-real-mode")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the aterm CLI binary with a malformed mode");
    child
        .stdin
        .take()
        .expect("aterm stdin")
        .write_all(b"echo ATERM_FAILCLOSED_$((5+5))\nexit\n")
        .expect("write to aterm stdin");
    let out = child.wait_with_output().expect("wait for aterm");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // It announced the fail-closed fallback — did NOT silently accept the garbage.
    assert!(
        stderr.contains("failing closed to Containment"),
        "a malformed mode must announce fail-closed-to-Containment; stderr={stderr:?}"
    );
    // And it still ran the shell (Containment is confined, not refuse-to-start).
    assert!(
        stdout.contains("ATERM_FAILCLOSED_10"),
        "the confined shell must still run a basic command; stdout={stdout:?}"
    );
    assert!(out.status.success(), "aterm must still exit success; got {:?}", out.status);
}
