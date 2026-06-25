// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `aterm-drive` — the AI-friendly "sugar" CLI over the core `await`/`send`/`key`/
//! `text` primitives. It teaches itself through `--help` and emits actionable
//! errors, so an AI agent builds correct intuition for the kernel without docs.
//! All real work flows through `aterm-ctl` (the std-only core client), so this is
//! a thin, honest wrapper — no protocol re-implementation.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use aterm_agent::{CtlClient, DRIVE_HELP, SelfGovernor, Turn};

/// Resolve the `aterm-ctl` binary: `$ATERM_CTL`, then a sibling of this binary
/// (the cargo/install layout), then bare `aterm-ctl` on `PATH`.
fn resolve_ctl() -> PathBuf {
    if let Ok(p) = std::env::var("ATERM_CTL") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sib = dir.join("aterm-ctl");
        if sib.is_file() {
            return sib;
        }
    }
    PathBuf::from("aterm-ctl")
}

struct Opts {
    socket: Option<String>,
    idle_ms: u64,
    timeout_ms: u64,
    cmd: Vec<String>,
}

fn parse() -> Result<Opts, String> {
    let mut socket = None;
    let mut idle_ms = 600u64;
    let mut timeout_ms = 180_000u64;
    let mut cmd = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--socket" | "--sock" => {
                socket = Some(it.next().ok_or("--socket needs a PATH")?);
            }
            "--idle" => {
                idle_ms = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--idle needs a millisecond integer")?;
            }
            "--timeout" => {
                timeout_ms = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--timeout needs a millisecond integer")?;
            }
            "-h" | "--help" | "help" => {
                cmd = vec!["help".to_string()];
                break;
            }
            _ => {
                cmd.push(a);
                cmd.extend(it.by_ref());
                break;
            }
        }
    }
    Ok(Opts {
        socket,
        idle_ms,
        timeout_ms,
        cmd,
    })
}

fn main() -> ExitCode {
    let opts = match parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("aterm-drive: {e}\n\nRun `aterm-drive --help` for usage.");
            return ExitCode::FAILURE;
        }
    };
    match run(&opts) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("aterm-drive: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(opts: &Opts) -> Result<String, String> {
    let verb = opts.cmd.first().map(String::as_str).unwrap_or("help");
    if verb == "help" {
        return Ok(format!("{DRIVE_HELP}\n"));
    }

    let ctl = resolve_ctl();
    let mut client = CtlClient::new(ctl.clone(), opts.socket.clone());

    // A friendly preflight: if we cannot even read the screen, explain why before
    // attempting to drive — this is the error an AI hits most and learns from.
    if let Err(e) = client.run(&["cursor"]) {
        return Err(format!(
            "cannot reach a target aterm over the control socket ({e}).\n  \
             • Is a host aterm running? Launch one headless:\n      \
             ATERM_HEADLESS=1 aterm-gui &\n  \
             • Point at its socket (it prints 'control socket listening at <PATH>'):\n      \
             export ATERM_CONTROL_SOCK=<PATH>   (or pass --socket <PATH>)\n  \
             • aterm-ctl resolved to: {}",
            ctl.display()
        ));
    }

    match verb {
        "prompt" => {
            let text = opts.cmd[1..].join(" ");
            if text.is_empty() {
                return Err("prompt needs text, e.g. `aterm-drive prompt 'say hi'`".to_string());
            }
            // A permissive governor: this drives ANOTHER session (cross-session),
            // so self-write is enabled with ample headroom. (Self-driving a single
            // terminal is the case where the floor matters — see the lib docs.)
            let mut gov = SelfGovernor::disabled(64, 8, 5_000_000);
            gov.enable_self_write();
            let turn = Turn {
                idle: Duration::from_millis(opts.idle_ms),
                timeout: Duration::from_millis(opts.timeout_ms),
                ..Turn::default()
            };
            turn.run(&mut client, &mut gov, text.as_bytes())
                .map_err(|e| e.to_string())
        }
        "read" => client.run(&["text"]),
        "shot" => {
            let path = opts.cmd.get(1).cloned();
            let mut args = vec!["image"];
            if let Some(p) = &path {
                args.push(p);
            }
            client.run(&args)
        }
        "await" => {
            if opts.cmd.len() < 2 {
                return Err(
                    "await needs a condition: idle <ms> | match <regex> | seq | block\n  \
                     e.g. `aterm-drive await match 'BUILD SUCCESSFUL'`"
                        .to_string(),
                );
            }
            // Pass the condition straight through to the core verb, but supply the
            // tool's --timeout so a bare `await idle 500` still has a sane bound.
            let mut args: Vec<String> = opts.cmd[1..].to_vec();
            if !args.iter().any(|a| a == "timeout") {
                args.push("timeout".to_string());
                args.push(opts.timeout_ms.to_string());
            }
            let mut a: Vec<&str> = vec!["await"];
            a.extend(args.iter().map(String::as_str));
            client.run(&a)
        }
        other => Err(format!(
            "unknown command '{other}'. Valid: prompt | read | await | shot | help.\n  \
             Run `aterm-drive --help` for the full guide."
        )),
    }
}
