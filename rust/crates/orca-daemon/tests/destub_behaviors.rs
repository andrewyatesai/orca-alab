//! Regression tests for the daemon de-stubbing: the RPCs that used to return
//! faked/placeholder values now return REAL engine/process state. Dispatch-level
//! (no socket); a manually-registered stream Sender captures `exit` events so the
//! real child exit code is observable without a live client.

#![cfg(unix)]

use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    serde_json::from_str(&dispatch_request(&req, reg, client)).expect("valid JSON")
}

/// Read the stream Receiver until an `exit` event for `session` arrives; returns
/// its `code`. Data events (buffered output) are skipped.
fn wait_for_exit_code(rx: &Receiver<String>, session: &str, timeout: Duration) -> Option<i64> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(100)) {
            let v: Value = serde_json::from_str(&line).expect("event is JSON");
            if v["event"] == json!("exit") && v["sessionId"] == json!(session) {
                return v["payload"]["code"].as_i64();
            }
        }
    }
    None
}

fn wait_until(mut pred: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    pred()
}

/// A child that exits 42 must surface code 42 on the exit event — not the old
/// hardcoded 0.
#[test]
fn exit_event_carries_the_real_child_code() {
    let reg = Arc::new(Registry::new());
    let client = "c-exit";
    let (tx, rx) = channel::<String>();
    reg.register_stream(client.to_string(), tx);

    let created = dispatch(
        &reg,
        client,
        json!({ "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s-exit", "cols": 80, "rows": 24, "command": "exit 42" } }),
    );
    assert_eq!(created["ok"], json!(true));

    let code = wait_for_exit_code(&rx, "s-exit", Duration::from_secs(10));
    assert_eq!(
        code,
        Some(42),
        "exit event must report the real code, not 0"
    );
}

/// Signalling a live session with SIGKILL ends it (and the exit event carries a
/// non-zero, signal-derived code).
#[test]
fn signal_kills_a_live_session() {
    let reg = Arc::new(Registry::new());
    let client = "c-sig";
    let (tx, rx) = channel::<String>();
    reg.register_stream(client.to_string(), tx);

    dispatch(
        &reg,
        client,
        json!({ "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s-sig", "cols": 80, "rows": 24, "command": "sleep 30" } }),
    );

    let signalled = dispatch(
        &reg,
        client,
        json!({ "id": "sg", "type": "signal", "payload": { "sessionId": "s-sig", "signal": "SIGKILL" } }),
    );
    assert_eq!(
        signalled["ok"],
        json!(true),
        "signal to a live session is ok"
    );

    let code = wait_for_exit_code(&rx, "s-sig", Duration::from_secs(10));
    assert!(
        code.is_some(),
        "SIGKILL must end the session with an exit event"
    );
    assert_ne!(code, Some(0), "a killed child must not report exit 0");

    // Signalling an unknown session errors (host.signal throws in the Node daemon too).
    let miss = dispatch(
        &reg,
        client,
        json!({ "id": "sg2", "type": "signal", "payload": { "sessionId": "nope", "signal": "SIGTERM" } }),
    );
    assert_eq!(miss["ok"], json!(false));
}

/// Read the stream Receiver's `data` events until `needle` is seen. Used to observe
/// what the spawned child actually printed.
fn wait_for_data(rx: &Receiver<String>, session: &str, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(100)) {
            let v: Value = serde_json::from_str(&line).expect("event JSON");
            if v["event"] == json!("data")
                && v["sessionId"] == json!(session)
                && v["payload"]["data"]
                    .as_str()
                    .is_some_and(|d| d.contains(needle))
            {
                return true;
            }
        }
    }
    false
}

/// createOrAttach honors the `env` overrides and `envToDelete` deletions the adapter
/// forwards (agent hooks / per-profile vars) — they used to be silently dropped, so
/// daemon-spawned shells ran with only the daemon's inherited env. `env` is applied
/// then `env_remove`, so a var present in both is deleted.
#[test]
fn create_or_attach_applies_session_env_and_deletions() {
    let reg = Arc::new(Registry::new());
    let client = "c-env";
    let (tx, rx) = channel::<String>();
    reg.register_stream(client.to_string(), tx);
    dispatch(
        &reg,
        client,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": "s-env", "cols": 80, "rows": 24,
                "env": { "ORCA_KEEP": "keep_xyz", "ORCA_DROP": "drop_xyz" },
                "envToDelete": ["ORCA_DROP"],
                "command": "printf 'K=[%s] D=[%s]' \"$ORCA_KEEP\" \"$ORCA_DROP\"" } }),
    );
    assert!(
        wait_for_data(&rx, "s-env", "K=[keep_xyz] D=[]", Duration::from_secs(10)),
        "child must see the env override (ORCA_KEEP) and the deletion (ORCA_DROP removed)"
    );
}

/// systemResolverHealth runs the daemon's OWN resolver probe (scutil on macOS)
/// end-to-end — not a hardcoded "unknown".
#[test]
fn system_resolver_health_probes_the_real_resolver() {
    let reg = Arc::new(Registry::new());
    let r = dispatch(
        &reg,
        "c",
        json!({ "id": "rh", "type": "systemResolverHealth" }),
    );
    assert_eq!(r["ok"], json!(true));
    let health = r["payload"]["health"].as_str().unwrap();
    assert!(["healthy", "unhealthy", "unknown"].contains(&health));
    // A dev machine with working DNS must classify healthy through the real scutil
    // output — proving the classifier matches production `scutil --dns`.
    #[cfg(target_os = "macos")]
    assert_eq!(health, "healthy", "a host with DNS should probe healthy");
}

/// ptySpawnHealth actually opens a PTY + spawns a probe child; a healthy subsystem
/// answers ok with healthy:true (not an unconditional stub).
#[test]
fn pty_spawn_health_runs_a_real_probe() {
    let reg = Arc::new(Registry::new());
    let health = dispatch(&reg, "c", json!({ "id": "h", "type": "ptySpawnHealth" }));
    assert_eq!(health["ok"], json!(true));
    assert_eq!(health["payload"]["healthy"], json!(true));
}

/// getCwd falls back to the LIVE shell process's cwd when the engine has no OSC-7
/// cwd. Orca's shells emit OSC-133 (not OSC-7), so this fallback (/proc on Linux,
/// lsof on macOS) is the common path — it used to return null, breaking the cwd the
/// client shows per tab. The child `cd`s into a temp dir without emitting OSC-7, so
/// the ONLY way getCwd can report it is the process fallback.
#[test]
fn get_cwd_falls_back_to_live_process_cwd_without_osc7() {
    let reg = Arc::new(Registry::new());
    let client = "c-cwd";

    // A unique temp dir (pid-derived, no rand needed) the shell will sit in.
    let dir = std::env::temp_dir().join(format!("orca-cwd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    let want = std::fs::canonicalize(&dir).expect("canonicalize temp");
    let dir_arg = dir.to_str().unwrap().to_string();

    dispatch(
        &reg,
        client,
        json!({ "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s-cwd", "cols": 80, "rows": 24,
                // `cd` alone emits no OSC-7; the shell stays alive on the sleep.
                "command": format!("cd {dir_arg} && sleep 30") } }),
    );

    let resolved = wait_until(
        || {
            let r = dispatch(
                &reg,
                client,
                json!({ "id": "g", "type": "getCwd", "payload": { "sessionId": "s-cwd" } }),
            );
            r["payload"]["cwd"]
                .as_str()
                .and_then(|c| std::fs::canonicalize(c).ok())
                .is_some_and(|c| c == want)
        },
        Duration::from_secs(10),
    );
    assert!(
        resolved,
        "getCwd must fall back to the live process cwd ({want:?}) with no OSC-7"
    );

    dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-cwd" } }),
    );
    let _ = std::fs::remove_dir(&dir);
}

/// getForegroundProcess resolves the PTY's foreground process group to a command
/// name (node-pty's `.process`) — it used to be an unconditional null. The wire shape
/// is `{ foregroundProcess: <string|null> }`; on a live PTY with a running child it
/// resolves a non-empty name (the shell or the command it's running).
#[test]
fn get_foreground_process_resolves_a_command_name() {
    let reg = Arc::new(Registry::new());
    let client = "c-fg";

    dispatch(
        &reg,
        client,
        json!({ "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s-fg", "cols": 80, "rows": 24, "command": "sleep 30" } }),
    );

    let mut last = Value::Null;
    let resolved = wait_until(
        || {
            let r = dispatch(
                &reg,
                client,
                json!({ "id": "g", "type": "getForegroundProcess", "payload": { "sessionId": "s-fg" } }),
            );
            last = r["payload"]["foregroundProcess"].clone();
            last.as_str().is_some_and(|n| !n.is_empty())
        },
        Duration::from_secs(10),
    );
    // Null is a valid wire value where no pgid exists, but a live PTY child on
    // Linux/macOS must resolve a real name.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert!(
        resolved,
        "a live PTY child must resolve a foreground process name, got {last:?}"
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = resolved;

    dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-fg" } }),
    );
}

/// createOrAttach reports a VALID ShellReadyState (types.ts union) — the old
/// "unknown" was not a member. getSnapshot carries an oscLinks array and a
/// rehydrateSequences string, both derived from real engine state.
#[test]
fn create_reports_valid_shell_state_and_snapshot_has_rehydrate_and_osc_links() {
    let reg = Arc::new(Registry::new());
    let client = "c-snap";

    let created = dispatch(
        &reg,
        client,
        json!({ "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s-snap", "cols": 80, "rows": 24 } }),
    );
    let valid = ["pending", "ready", "timed_out", "unsupported"];
    let shell_state = created["payload"]["shellState"].as_str().unwrap();
    assert!(
        valid.contains(&shell_state),
        "shellState '{shell_state}' must be a ShellReadyState"
    );

    // Turn bracketed paste on (DECSET 2004); the engine tracks it and the snapshot's
    // rehydrateSequences must replay it on reattach.
    dispatch(
        &reg,
        client,
        json!({ "id": "w", "type": "write",
            "payload": { "sessionId": "s-snap", "data": "printf '\\033[?2004h'\n" } }),
    );
    let rehydrated = wait_until(
        || {
            let snap = dispatch(
                &reg,
                client,
                json!({ "id": "g", "type": "getSnapshot", "payload": { "sessionId": "s-snap" } }),
            );
            snap["payload"]["snapshot"]["rehydrateSequences"]
                .as_str()
                .is_some_and(|s| s.contains("\u{1b}[?2004h"))
        },
        Duration::from_secs(5),
    );
    assert!(
        rehydrated,
        "rehydrateSequences must replay the enabled bracketed-paste mode"
    );

    let snap = dispatch(
        &reg,
        client,
        json!({ "id": "g2", "type": "getSnapshot", "payload": { "sessionId": "s-snap" } }),
    );
    assert!(
        snap["payload"]["snapshot"]["oscLinks"].is_array(),
        "snapshot carries an oscLinks array"
    );

    dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-snap" } }),
    );
}
