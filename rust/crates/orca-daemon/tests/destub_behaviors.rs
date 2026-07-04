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
    assert_eq!(code, Some(42), "exit event must report the real code, not 0");
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
    assert_eq!(signalled["ok"], json!(true), "signal to a live session is ok");

    let code = wait_for_exit_code(&rx, "s-sig", Duration::from_secs(10));
    assert!(code.is_some(), "SIGKILL must end the session with an exit event");
    assert_ne!(code, Some(0), "a killed child must not report exit 0");

    // Signalling an unknown session errors (host.signal throws in the Node daemon too).
    let miss = dispatch(
        &reg,
        client,
        json!({ "id": "sg2", "type": "signal", "payload": { "sessionId": "nope", "signal": "SIGTERM" } }),
    );
    assert_eq!(miss["ok"], json!(false));
}

/// systemResolverHealth runs the daemon's OWN resolver probe (scutil on macOS)
/// end-to-end — not a hardcoded "unknown".
#[test]
fn system_resolver_health_probes_the_real_resolver() {
    let reg = Arc::new(Registry::new());
    let r = dispatch(&reg, "c", json!({ "id": "rh", "type": "systemResolverHealth" }));
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
    assert!(valid.contains(&shell_state), "shellState '{shell_state}' must be a ShellReadyState");

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
    assert!(rehydrated, "rehydrateSequences must replay the enabled bracketed-paste mode");

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
