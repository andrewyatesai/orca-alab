//! Control-socket RPC dispatch + the per-session output pump. Requests arrive as
//! `serde_json::Value`; each returns the NDJSON response line the control socket
//! writes back. `createOrAttach` additionally spawns the PTY and the reader thread
//! that feeds the session engine (terminal + checkpoint records) and streams output
//! live to the client (dropped when detached — the reattach snapshot restores it).

use crate::pending_output::PendingOutput;
use crate::protocol::{rpc_err, rpc_ok};
use crate::registry::{Registry, SessionEngine, SessionEntry};
use crate::shell_ready_barrier::{
    GateTimer, ShellReadyBarrier, POST_READY_FLUSH_DELAY_MS, POST_READY_FLUSH_FALLBACK_MS,
    SHELL_READY_TIMEOUT_MS,
};
use crate::utf8_stream_decoder::Utf8StreamDecoder;
use orca_pty::{PtyCommand, PtySession, PtySize};
use orca_terminal::{HeadlessTerminal, MouseTracking, DEFAULT_SCROLLBACK};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Graceful-kill escalation window, matching the Node session's `KILL_TIMEOUT_MS`:
/// a child that ignores the graceful SIGHUP gets SIGKILL'd after this delay.
const KILL_TIMEOUT: Duration = Duration::from_secs(5);

fn field_str<'a>(payload: &'a Value, key: &str) -> &'a str {
    payload.get(key).and_then(Value::as_str).unwrap_or("")
}

fn field_u16(payload: &Value, key: &str, default: u16) -> u16 {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default as u64) as u16
}

/// A void ack. The Node daemon returns `{}` (not null) for side-effecting RPCs, so
/// match its payload shape for wire byte-parity.
fn void_ack(id: &str) -> String {
    rpc_ok(id, json!({}))
}

/// A write/resize hit an unknown session. Fire a synthetic exit(-1) on the client's
/// stream so the renderer clears the stale pane binding (write/resize are
/// fire-and-forget, so the control error alone is invisible), then return the error —
/// parity with the Node daemon's sendExitEvent on SessionNotFoundError.
fn unknown_session_exit(
    id: &str,
    registry: &Arc<Registry>,
    client_id: &str,
    session_id: &str,
) -> String {
    registry.route_exit_to_client(client_id, session_id, -1);
    rpc_err(id, "unknown session")
}

pub fn dispatch_request(request: &Value, registry: &Arc<Registry>, client_id: &str) -> String {
    let id = request.get("id").and_then(Value::as_str).unwrap_or("");
    let kind = request.get("type").and_then(Value::as_str).unwrap_or("");
    // Borrow the payload — do NOT clone it: a `write` payload carries the full data
    // chunk (up to NDJSON_MAX_LINE_BYTES), and cloning it here would copy megabytes
    // per keystroke burst before field_str even reads it.
    let null_payload = Value::Null;
    let payload = request.get("payload").unwrap_or(&null_payload);
    let sid = || field_str(payload, "sessionId").to_string();
    match kind {
        "createOrAttach" => create_or_attach(id, payload, registry, client_id),
        "write" => {
            // Borrow the data straight from the payload — with_session runs its
            // closure synchronously under the lock (FnOnce, not 'static), so no copy
            // is needed. A `.to_string()` here would re-copy up to 16MB on every
            // paste, the exact per-write cost the borrow-not-clone note above avoids.
            let session_id = sid();
            let data = field_str(payload, "data");
            match session_write(registry, &session_id, data) {
                Some(Ok(())) => void_ack(id),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => unknown_session_exit(id, registry, client_id, &session_id),
            }
        }
        "resize" => {
            let session_id = sid();
            let cols = field_u16(payload, "cols", 80);
            let rows = field_u16(payload, "rows", 24);
            match registry.with_session(&session_id, |e| {
                e.cols = cols;
                e.rows = rows;
                if let Ok(mut engine) = e.engine.lock() {
                    engine.terminal.resize(rows as usize, cols as usize);
                    engine.pending.record_resize(cols, rows);
                }
                e.pty.resize(PtySize { rows, cols })
            }) {
                Some(Ok(())) => void_ack(id),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => unknown_session_exit(id, registry, client_id, &session_id),
            }
        }
        // Kill the child; the pump's EOF then reaps the session + emits `exit`. An
        // unknown session errors, like the Node daemon's getAliveSession.
        //
        // `immediate` mirrors terminal-host.ts kill(): the immediate path force-kills
        // (SIGKILL) at once, while the default graceful path sends SIGHUP (node-pty's
        // default) so the shell can run its EXIT trap / save history, then escalates
        // to SIGKILL after KILL_TIMEOUT if the child ignored it.
        "kill" => {
            let session_id = sid();
            let immediate = payload
                .get("immediate")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if immediate {
                match registry.with_session(&session_id, |e| {
                    let _ = e.pty.kill();
                }) {
                    Some(()) => void_ack(id),
                    None => rpc_err(id, "unknown session"),
                }
            } else {
                match registry.with_session(&session_id, |e| {
                    let _ = e.pty.signal("SIGHUP");
                    e.pid
                }) {
                    Some(expected_pid) => {
                        let reg = registry.clone();
                        thread::spawn(move || {
                            thread::sleep(KILL_TIMEOUT);
                            reg.force_kill_if_still_pid(&session_id, expected_pid);
                        });
                        void_ack(id)
                    }
                    None => rpc_err(id, "unknown session"),
                }
            }
        }
        // The session already survives control-socket close, so detach is a no-op ack.
        "detach" => void_ack(id),
        // The incremental checkpoint batch: typed records + monotonic seq + overflow
        // flag, and (when requested) a snapshot serialized in the same atomic turn.
        // Mirrors the Node daemon's TakePendingOutputResult (types.ts) — the client
        // appends each batch to the on-disk history log for crash cold-restore.
        "takePendingOutput" => {
            let include_snapshot = payload
                .get("includeSnapshot")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let teardown_snapshot = payload
                .get("teardownSnapshot")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match registry.take_pending_output(&sid(), include_snapshot, teardown_snapshot) {
                Some((records, seq, overflowed, snapshot)) => rpc_ok(
                    id,
                    json!({ "records": records, "seq": seq, "overflowed": overflowed, "snapshot": snapshot }),
                ),
                // Missing/just-reaped session → ok+null, NOT an error. The Node host
                // is null-not-throw here (terminal-host.ts), and the client's
                // checkpoint loop relies on it (`if (!take) return 'done'`): an error
                // would spuriously log "[history] checkpoint failed" and leave the
                // session dirty until its exit event lands. Consistent with
                // getSnapshot, which also returns ok+null for an unknown session.
                None => rpc_ok(id, Value::Null),
            }
        }
        "listSessions" => rpc_ok(id, registry.list_sessions()),
        // Wire shape mirrors the Node daemon: `{ size: { cols, rows } }` (not the
        // dims at the payload top level) — see daemon-server.ts `getAppliedSize`.
        "getSize" => match registry.session_size(&sid()) {
            Some((cols, rows)) => rpc_ok(id, json!({ "size": { "cols": cols, "rows": rows } })),
            // Node's getAppliedSize is null-not-throw for a missing/dead session
            // (terminal-host.ts), so the renderer resume drift-check reads null, not
            // an error. Match that: `{ size: null }`, not an rpc error.
            None => rpc_ok(id, json!({ "size": Value::Null })),
        },
        "ping" => rpc_ok(id, json!({ "pong": true })),
        // Real probe: open a PTY and spawn a trivial child. If the PTY subsystem is
        // healthy it spawns + exits; any failure surfaces as an error. Mirrors the
        // Node daemon running checkPtySpawnHealth before answering healthy:true.
        "ptySpawnHealth" => match probe_pty_spawn() {
            Ok(()) => rpc_ok(id, json!({ "healthy": true })),
            Err(e) => rpc_err(id, &format!("pty spawn health failed: {e}")),
        },
        // Real resolver health from the daemon's own process (scutil on macOS);
        // "unknown" elsewhere. Lets the launcher's preserve/replace decision see a
        // Rust daemon that lost its scoped system resolver — same as the Node daemon.
        "systemResolverHealth" => rpc_ok(
            id,
            json!({ "health": crate::resolver_health::system_resolver_health() }),
        ),
        // Real engine state from the session's headless aterm terminal — no napi hop.
        "getSnapshot" => match registry.engine_of(&sid()) {
            Some(engine) => {
                let snapshot = build_snapshot(&mut engine.lock().unwrap().terminal);
                rpc_ok(id, json!({ "snapshot": snapshot }))
            }
            None => rpc_ok(id, json!({ "snapshot": Value::Null })),
        },
        // Wire shape mirrors the Node daemon: `{ cwd: <string|null> }`. Prefer the
        // engine's OSC-7 cwd, but Orca's shells emit OSC-133 (not OSC-7), so that's
        // usually absent — fall back to the live shell process's cwd (/proc on Linux,
        // lsof on macOS), exactly as daemon-server.ts getCwd → resolveProcessCwd does.
        "getCwd" => {
            let sid = sid();
            // Node's getCwd goes through getAliveSession, which THROWS on an unknown
            // session (terminal-host.ts); mirror that with an error. A KNOWN session
            // with no resolvable cwd still returns ok + null below.
            match registry.engine_of(&sid) {
                Some(engine) => {
                    let cwd = engine
                        .lock()
                        .unwrap()
                        .terminal
                        .cwd()
                        .map(str::to_string)
                        .or_else(|| {
                            registry
                                .session_pid(&sid)
                                .and_then(crate::process_query::process_cwd)
                        });
                    rpc_ok(id, json!({ "cwd": cwd }))
                }
                None => rpc_err(id, "unknown session"),
            }
        }
        // Wire shape mirrors the Node daemon: `{ foregroundProcess: <string|null> }`.
        // Resolve the PTY's foreground process group (tcgetpgrp) → its command name
        // (an agent, a build, or the shell at the prompt), mirroring node-pty's
        // `.process`. Null when the pty/platform has no such concept or the pgid is gone.
        "getForegroundProcess" => {
            let name = registry
                .with_session(&sid(), |e| e.pty.foreground_process_group())
                .flatten()
                .filter(|pgid| *pgid > 0)
                .and_then(|pgid| crate::process_query::process_name(pgid as u32));
            rpc_ok(id, json!({ "foregroundProcess": name }))
        }
        // An unknown session errors, like host.clearScrollback → getAliveSession.
        "clearScrollback" => match registry.engine_of(&sid()) {
            Some(engine) => {
                let mut engine = engine.lock().unwrap();
                engine.terminal.clear_scrollback();
                engine.pending.record_clear();
                void_ack(id)
            }
            None => rpc_err(id, "unknown session"),
        },
        "cancelCreateOrAttach" => void_ack(id),
        // Deliver a named signal to the child (node-pty's `kill(signal)`). Errors
        // from a dead child are dropped like the Node daemon; an unknown session
        // errors (host.signal throws on a missing session there too).
        "signal" => {
            let sig = field_str(payload, "signal");
            match registry.with_session(&sid(), |e| e.pty.signal(sig)) {
                Some(_) => void_ack(id),
                None => rpc_err(id, "unknown session"),
            }
        }
        "shutdown" => {
            // killSessions=true → SIGKILL every child now (parity with the Node
            // host.dispose()); a child that ignores the PTY-close SIGHUP would
            // otherwise outlive the daemon. Then unlink the socket file so a stale
            // path can't linger (parity with server.close→unlinkSync).
            if payload
                .get("killSessions")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                registry.kill_all_sessions();
            }
            registry.unlink_socket();
            // Reply first, then exit so the ok flushes to the client.
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(50));
                std::process::exit(0);
            });
            void_ack(id)
        }
        other => rpc_err(id, &format!("unsupported request type: {other}")),
    }
}

/// Health probe for `ptySpawnHealth`: open a PTY and spawn a trivial child that
/// exits at once, then reap it. Bypasses the login shell (no `-lc`) so the check
/// stays fast and free of user-profile side effects. Any error means the PTY
/// subsystem can't currently spawn.
fn probe_pty_spawn() -> std::io::Result<()> {
    let mut probe = PtySession::spawn(&probe_command(), PtySize { rows: 1, cols: 1 })?;
    // `exit 0` returns immediately; wait() reaps it so the probe leaves no child.
    let _ = probe.wait();
    Ok(())
}

#[cfg(unix)]
fn probe_command() -> PtyCommand {
    PtyCommand {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), "exit 0".to_string()],
        ..PtyCommand::default()
    }
}

#[cfg(windows)]
fn probe_command() -> PtyCommand {
    PtyCommand {
        program: default_shell(),
        args: vec!["/C".to_string(), "exit".to_string(), "0".to_string()],
        ..PtyCommand::default()
    }
}

fn create_or_attach(
    id: &str,
    payload: &Value,
    registry: &Arc<Registry>,
    client_id: &str,
) -> String {
    let session_id = field_str(payload, "sessionId").to_string();
    if session_id.is_empty() {
        return rpc_err(id, "missing sessionId");
    }
    // Reattach a live session: rebind it to this (possibly new) client and return a
    // REAL snapshot so the reattacher repaints — matching terminal-host.ts's live
    // branch (getSnapshot + detachAllClients + attachClient). A blank snapshot here
    // would leave a warm-reattached pane frozen after relaunch.
    if let Some((engine, shell_state)) = registry.reattach_if_alive(&session_id, client_id) {
        let snapshot = build_snapshot(&mut engine.lock().unwrap().terminal);
        let pid = registry.session_pid(&session_id);
        return rpc_ok(
            id,
            json!({ "isNew": false, "snapshot": snapshot, "pid": pid, "shellState": shell_state }),
        );
    }
    // Not a live session: drop any lingering dead entry for this id, then spawn fresh.
    registry.remove_session(&session_id);

    let cols = field_u16(payload, "cols", 80);
    let rows = field_u16(payload, "rows", 24);
    let launch = build_command(payload);
    let pty = match PtySession::spawn(&launch.command, PtySize { rows, cols }) {
        Ok(p) => p,
        Err(e) => return rpc_err(id, &format!("spawn failed: {e}")),
    };
    let pid = pty.process_id();
    let reader = match pty.try_clone_reader() {
        Ok(r) => r,
        Err(e) => return rpc_err(id, &format!("reader clone failed: {e}")),
    };
    // Per-session engine (headless aterm terminal + checkpoint record log) behind one
    // lock, so the pump feeds both atomically and getSnapshot/getCwd/takePendingOutput
    // read consistent state — no napi hop.
    let engine = Arc::new(Mutex::new(SessionEngine {
        terminal: HeadlessTerminal::with_scrollback(
            rows as usize,
            cols as usize,
            DEFAULT_SCROLLBACK,
        ),
        pending: PendingOutput::default(),
    }));
    // The shell-ready barrier (session.ts): while pending, stdin writes queue and
    // the pump scans output for the wrapper's OSC 777 marker. Bounded by the
    // client's shellReadyTimeoutMs (Codex markerless: 300ms) or the 15s default.
    let shell_ready_supported = payload
        .get("shellReadySupported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let barrier =
        shell_ready_supported.then(|| Arc::new(Mutex::new(ShellReadyBarrier::new_pending())));
    let shell_state = if barrier.is_some() {
        "pending"
    } else {
        "unsupported"
    };
    let created_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    registry.insert_session(
        session_id.clone(),
        SessionEntry {
            pty,
            client_id: client_id.to_string(),
            cols,
            rows,
            pid,
            created_at_ms,
            engine: Arc::clone(&engine),
            barrier: barrier.clone(),
        },
    );
    // Barrier timers + pump start only AFTER the session is registered: a fast
    // child (e.g. /bin/echo) can produce output — or even exit — before the
    // entry exists, and route_output / the timeout's flush would no-op on the
    // missing session, dropping its first bytes (or stranding the queue).
    if let Some(barrier) = &barrier {
        let timeout_ms = payload
            .get("shellReadyTimeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(SHELL_READY_TIMEOUT_MS);
        spawn_ready_timeout(
            registry.clone(),
            session_id.clone(),
            Arc::clone(barrier),
            timeout_ms,
        );
    }
    // Pump raw PTY output → fed into the engine (terminal + records) AND streamed live
    // by the registry; on EOF, reap the child (remove the session + emit `exit`). The
    // reader is an independent clone of the master, so it keeps reading after `pty`
    // moves into the registry entry.
    let pump_registry = registry.clone();
    let pump_session = session_id.clone();
    thread::spawn(move || pump_output(reader, pump_registry, pump_session, engine, barrier));
    // Interactive-spawn sessions get their startup command through stdin (queued
    // behind the barrier when one is armed) — terminal-host.ts createOrAttach.
    // Legacy `-lc` spawns already carry it in argv.
    if let Some(command) = launch.stdin_command {
        let submit_terminated = command.ends_with('\n') || command.ends_with('\r');
        let payload = if submit_terminated {
            command
        } else {
            format!("{command}\n")
        };
        let _ = session_write(registry, &session_id, &payload);
    }
    rpc_ok(
        id,
        json!({ "isNew": true, "snapshot": Value::Null, "pid": pid, "shellState": shell_state }),
    )
}

/// A resolved spawn: the PTY command plus the startup command to deliver via
/// stdin after spawn, when the launch args don't already carry it.
struct ShellLaunch {
    command: PtyCommand,
    stdin_command: Option<String>,
}

/// Resolve the shell/args to spawn. The shell is the renderer's per-pane
/// `shellOverride` (the "+" menu / persisted default), then the payload env's
/// SHELL, then the daemon's `$SHELL` — without honoring the override the daemon
/// always spawned `$SHELL` regardless of the shell the pane asked for.
///
/// Args, in order of preference:
/// - `shellArgs` from the payload: the client pre-computes the full launch
///   config (login `-l`, ZDOTDIR/rcfile wrapper args — see
///   docs/rust-migration/daemon-shell-launch.md), and any `command` is
///   delivered via stdin so it runs inside the long-lived interactive shell.
/// - no `shellArgs`, `command` present: legacy non-interactive `-lc` (kept for
///   older clients and the parity corpus).
/// - neither: a plain LOGIN shell (`-l`), matching pty-subprocess.ts:758 and
///   local-pty-provider.ts:459 — without it terminals lose .zprofile/.zlogin
///   env (brew shellenv PATH etc.).
fn build_command(payload: &Value) -> ShellLaunch {
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let env = payload_env(payload);
    let shell = payload
        .get("shellOverride")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| env_shell_fallback(&env))
        .unwrap_or_else(default_shell);
    let shell_args: Option<Vec<String>> =
        payload
            .get("shellArgs")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            });
    let command = payload
        .get("command")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    let (args, stdin_command) = match (shell_args, command) {
        (Some(args), command) => (args, command),
        (None, Some(cmd)) => (shell_run_args(&cmd), None),
        (None, None) => (plain_session_args(), None),
    };
    ShellLaunch {
        command: PtyCommand {
            program: shell,
            args,
            cwd,
            // Per-session env overrides (agent hooks, per-profile vars) and deletions —
            // the createOrAttach `env` / `envToDelete` the adapter forwards. Dropping
            // these ran daemon-spawned shells with only the daemon's inherited env.
            env,
            env_remove: payload_env_to_delete(payload),
        },
        stdin_command,
    }
}

/// The `env` object (`{ KEY: "value" }`) as override pairs; empty if absent.
fn payload_env(payload: &Value) -> Vec<(String, String)> {
    payload
        .get("env")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// The `envToDelete` array of inherited var names to remove; empty if absent.
fn payload_env_to_delete(payload: &Value) -> Vec<String> {
    payload
        .get("envToDelete")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

/// The Node daemon resolves the payload env's SHELL before its own `$SHELL`
/// (shell-ready.ts resolvePtyShellPath) — mirror that on unix only; Windows
/// shell resolution never keys off SHELL.
#[cfg(unix)]
fn env_shell_fallback(env: &[(String, String)]) -> Option<String> {
    env.iter()
        .find(|(k, _)| k == "SHELL")
        .map(|(_, v)| v.clone())
        .filter(|s| !s.is_empty())
}

#[cfg(windows)]
fn env_shell_fallback(_env: &[(String, String)]) -> Option<String> {
    None
}

#[cfg(unix)]
fn shell_run_args(cmd: &str) -> Vec<String> {
    vec!["-lc".to_string(), cmd.to_string()]
}

/// F4b: a plain session runs the user shell as a LOGIN shell, so terminals get
/// .zprofile/.zlogin env — same `['-l']` default as both Node references.
#[cfg(unix)]
fn plain_session_args() -> Vec<String> {
    vec!["-l".to_string()]
}

/// Windows twin: ConPTY sessions run under `%ComSpec%` (cmd.exe), the platform's
/// interactive default; `/C` is the `-lc` analogue (cmd has no login semantics).
#[cfg(windows)]
fn default_shell() -> String {
    std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
}

#[cfg(windows)]
fn shell_run_args(cmd: &str) -> Vec<String> {
    vec!["/C".to_string(), cmd.to_string()]
}

/// cmd.exe has no login semantics — plain Windows sessions stay arg-less.
#[cfg(windows)]
fn plain_session_args() -> Vec<String> {
    Vec::new()
}

/// Write to a session's PTY through the shell-ready barrier: while the barrier
/// is pending (or its post-ready flush gate hasn't fired), the data queues so
/// it can't race ahead of the buffered startup command — session.ts write().
/// Barrier locked INSIDE the registry lock (the crate-wide lock order).
fn session_write(
    registry: &Arc<Registry>,
    session_id: &str,
    data: &str,
) -> Option<std::io::Result<()>> {
    registry.with_session(session_id, |e| {
        if let Some(barrier) = &e.barrier {
            let mut barrier = barrier.lock().unwrap();
            if barrier.should_queue() {
                barrier.push_queued(data.to_string());
                return Ok(());
            }
        }
        e.pty.write_all(data.as_bytes())
    })
}

/// Drain the barrier's stdin queue into the PTY iff `take` (a generation-checked
/// gate/timeout acceptance) approves — all under the registry lock so no
/// concurrent write can interleave with the flushed startup command.
fn flush_queue_if(
    registry: &Arc<Registry>,
    session_id: &str,
    barrier: &Arc<Mutex<ShellReadyBarrier>>,
    take: impl FnOnce(&mut ShellReadyBarrier) -> bool,
) {
    registry.with_session(session_id, |e| {
        let mut barrier = barrier.lock().unwrap();
        if !take(&mut barrier) {
            return;
        }
        for data in barrier.drain_queue() {
            let _ = e.pty.write_all(data.as_bytes());
        }
    });
}

/// Schedule the post-ready flush gate timer a barrier transition asked for.
/// Stale generations are no-ops inside the barrier, so a superseded timer
/// firing late is harmless.
fn spawn_gate_timer(
    registry: Arc<Registry>,
    session_id: String,
    barrier: Arc<Mutex<ShellReadyBarrier>>,
    timer: GateTimer,
) {
    thread::spawn(move || match timer {
        GateTimer::PostData(generation) => {
            thread::sleep(Duration::from_millis(POST_READY_FLUSH_DELAY_MS));
            flush_queue_if(&registry, &session_id, &barrier, |b| {
                b.on_post_data_elapsed(generation)
            });
        }
        GateTimer::Fallback(generation) => {
            thread::sleep(Duration::from_millis(POST_READY_FLUSH_FALLBACK_MS));
            flush_queue_if(&registry, &session_id, &barrier, |b| {
                b.on_fallback_elapsed(generation)
            });
        }
    });
}

/// Bound the wait for a marker that may never come (wrapper-less shell, slow
/// rc files): after `timeout_ms`, release the held partial-marker bytes
/// downstream and flush the queued stdin — session.ts onShellReadyTimeout.
fn spawn_ready_timeout(
    registry: Arc<Registry>,
    session_id: String,
    barrier: Arc<Mutex<ShellReadyBarrier>>,
    timeout_ms: u64,
) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(timeout_ms));
        // Transition + engine feed + queue flush in ONE registry-lock turn so a
        // concurrent write can't slip between the state flip and the flush.
        let held = registry
            .with_session(&session_id, |e| {
                let mut b = barrier.lock().unwrap();
                let held = b.on_ready_timeout_elapsed()?;
                if !held.is_empty() {
                    if let Ok(mut engine) = e.engine.lock() {
                        engine.terminal.process(held.as_bytes());
                        engine.pending.record_output(&held);
                    }
                }
                for data in b.drain_queue() {
                    let _ = e.pty.write_all(data.as_bytes());
                }
                Some(held)
            })
            .flatten();
        // Stream the released bytes outside the registry lock (route_output
        // re-takes it). Ordering vs concurrent output is cosmetic here: the
        // held bytes are at most a partial ESC]777 prefix.
        if let Some(held) = held {
            if !held.is_empty() {
                registry.route_output(&session_id, &held);
            }
        }
    });
}

fn pump_output(
    mut reader: Box<dyn Read + Send>,
    registry: Arc<Registry>,
    session_id: String,
    engine: Arc<Mutex<SessionEngine>>,
    barrier: Option<Arc<Mutex<ShellReadyBarrier>>>,
) {
    let mut buf = [0u8; 65536];
    // Barrier-less sessions feed the engine RAW bytes (its VT parser is
    // byte-accurate and buffers incomplete sequences), but the checkpoint records
    // + live stream are text, so decode with a boundary-carrying decoder: a
    // multibyte char split across two reads is completed on the next chunk
    // instead of becoming U+FFFD, which would desync the stream/records from the
    // (correct) engine grid.
    let mut decoder = Utf8StreamDecoder::new();
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = decoder.decode(&buf[..n]);
                // While the barrier scans for the ready marker, the SCANNED text
                // (marker stripped, partial prefix withheld) replaces the chunk
                // everywhere downstream — engine, records, and stream — matching
                // session.ts, where scanning runs before all fan-out. Once
                // readiness resolves, the barrier only observes chunks for its
                // post-ready flush gate.
                let (scanned, timer) = match &barrier {
                    Some(b) => {
                        let mut b = b.lock().unwrap();
                        if b.is_scanning() {
                            let (text, timer) = b.process_output(&data);
                            (Some(text), timer)
                        } else {
                            (None, b.notify_output())
                        }
                    }
                    None => (None, None),
                };
                let text = scanned.as_deref().unwrap_or(&data);
                // Feed the engine ATOMICALLY: bytes into the VT parser AND the
                // same chunk into the checkpoint record log, under one lock so a
                // concurrent takePendingOutput can't see the terminal updated but
                // the record missing (which would duplicate bytes on cold restore).
                // A fully-withheld chunk (all bytes held by the marker scanner)
                // feeds nothing, like session.ts's empty-output early return.
                if scanned.is_none() || !text.is_empty() {
                    if let Ok(mut engine) = engine.lock() {
                        // Barrier sessions keep the DECODED engine feed for their whole
                        // lifetime: the decoder can hold a split multibyte char as carry
                        // across the scan→post-scan boundary, so switching back to raw
                        // bytes there would hand the engine orphan continuation bytes
                        // and corrupt one glyph in its grid vs the records/stream.
                        if barrier.is_some() {
                            engine.terminal.process(text.as_bytes());
                        } else {
                            engine.terminal.process(&buf[..n]);
                        }
                        engine.pending.record_output(text);
                    }
                    // Stream the same boundary-safe copy live to the attached client
                    // (dropped if detached — the reattach snapshot restores it).
                    registry.route_output(&session_id, text);
                }
                if let Some(timer) = timer {
                    spawn_gate_timer(
                        registry.clone(),
                        session_id.clone(),
                        Arc::clone(barrier.as_ref().expect("timer implies barrier")),
                        timer,
                    );
                }
            }
        }
    }
    // A child that exits mid-scan releases the held partial-marker bytes so the
    // final records/stream carry everything it wrote — session.ts
    // handleSubprocessExit → releaseHeldShellReadyBytes.
    if let Some(b) = &barrier {
        let held = b.lock().unwrap().release_held_bytes();
        if !held.is_empty() {
            if let Ok(mut engine) = engine.lock() {
                engine.terminal.process(held.as_bytes());
                engine.pending.record_output(&held);
            }
            registry.route_output(&session_id, &held);
        }
    }
    // EOF means the child closed the PTY — i.e. it exited. Reap it for the REAL
    // exit code (wait() returns at once now) and notify the client.
    registry.reap_and_mark_exited(&session_id);
}

/// Build a `TerminalSnapshot` (types.ts) from the session's headless aterm engine.
/// ansi/cwd/modes are REAL engine state; `rehydrateSequences` replays the screen and
/// input modes on reattach, and `oscLinks` carries the scrollback/screen hyperlink
/// ranges so links survive a reconnect. `pub(crate)` so the registry can serialize a
/// snapshot in the same engine-lock turn as a checkpoint drain (takePendingOutput).
pub(crate) fn build_snapshot(term: &mut HeadlessTerminal) -> Value {
    let snapshot_ansi = term.serialize_ansi(None);
    let scrollback_ansi = term.serialize_scrollback_ansi(None);
    let osc_links: Vec<Value> = term
        .osc_link_ranges(None)
        .into_iter()
        .map(
            |l| json!({ "row": l.row, "startCol": l.start_col, "endCol": l.end_col, "uri": l.uri }),
        )
        .collect();
    let cwd = term.cwd().map(str::to_string);
    let bracketed = term.bracketed_paste();
    let app_cursor = term.application_cursor();
    let alt_screen = term.is_alternate_screen();
    let title = term.title();
    let (rows, cols) = term.size();
    let scrollback_lines = term.scrollback_len();
    let (mouse_on, mouse_mode) = match term.mouse_tracking() {
        MouseTracking::None => (false, "none"),
        MouseTracking::X10 => (true, "x10"),
        MouseTracking::Normal => (true, "vt200"),
        MouseTracking::Button => (true, "drag"),
        MouseTracking::Any => (true, "any"),
    };
    // SGR mouse encoding (DECSET 1006) + its pixel variant (1016). The Node
    // daemon carries both in TerminalModes; aterm exposes them directly.
    let sgr_mouse = term.sgr_mouse();
    let sgr_pixels = term.sgr_pixels();
    let rehydrate = rehydrate_sequences(
        mouse_on, mouse_mode, bracketed, app_cursor, alt_screen, sgr_mouse, sgr_pixels,
    );
    let mut snapshot = json!({
        "snapshotAnsi": snapshot_ansi,
        "scrollbackAnsi": scrollback_ansi,
        "rehydrateSequences": rehydrate,
        "oscLinks": osc_links,
        "cwd": cwd,
        "modes": {
            "bracketedPaste": bracketed,
            "mouseTracking": mouse_on,
            "mouseTrackingMode": mouse_mode,
            "sgrMouseMode": sgr_mouse,
            "sgrMousePixelsMode": sgr_pixels,
            "applicationCursor": app_cursor,
            "alternateScreen": alt_screen,
        },
        "cols": cols,
        "rows": rows,
        "scrollbackLines": scrollback_lines,
    });
    // `lastTitle` is an OPTIONAL field: the Node daemon OMITS the key when no title
    // has been set, rather than emitting null. Match that so the wire shape agrees.
    if let Some(title) = title {
        snapshot["lastTitle"] = json!(title);
    }
    snapshot
}

/// Control sequences that re-apply screen/input modes on reattach — a faithful
/// port of headless-emulator.ts `buildRehydrateSequences`. Order matters (alt
/// screen, bracketed paste, app cursor, mouse tracking, then SGR encoding), and
/// the SGR encoding is preserved even when mouse reporting is off.
fn rehydrate_sequences(
    mouse_on: bool,
    mouse_mode: &str,
    bracketed: bool,
    app_cursor: bool,
    alt_screen: bool,
    sgr_mouse: bool,
    sgr_pixels: bool,
) -> String {
    let mut s = String::new();
    if alt_screen {
        s.push_str("\x1b[?1049h");
    }
    if bracketed {
        s.push_str("\x1b[?2004h");
    }
    if app_cursor {
        s.push_str("\x1b[?1h");
    }
    match if mouse_on { mouse_mode } else { "none" } {
        "x10" => s.push_str("\x1b[?9h"),
        "vt200" => s.push_str("\x1b[?1000h"),
        "drag" => s.push_str("\x1b[?1002h"),
        "any" => s.push_str("\x1b[?1003h"),
        _ => {}
    }
    if sgr_pixels {
        s.push_str("\x1b[?1016h");
    } else if sgr_mouse {
        s.push_str("\x1b[?1006h");
    }
    s
}
