//! Regression tests for the wire-behavior parity fixes: shellOverride honored,
//! synthetic exit on write/resize to an unknown session, unknown-session getSize/
//! getCwd conventions, and kill-all on shutdown. Dispatch-level (no socket), with a
//! manually-registered stream Sender to observe events, matching destub_behaviors.rs.

#![cfg(unix)]

use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use orca_daemon::stream_coalescing::{encode_stream_item, StreamItem, StreamWireFormat};
use serde_json::{json, Value};
use orca_daemon::bounded_stream_channel::{stream_channel, StreamReceiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    serde_json::from_str(&dispatch_request(&req, reg, client)).expect("valid JSON")
}

/// Encode a queued semantic item exactly as the NDJSON writer thread would, so
/// line-level assertions survive the semantic-queue migration unchanged.
fn ndjson_line(item: &StreamItem) -> String {
    String::from_utf8(encode_stream_item(item, StreamWireFormat::Ndjson)).unwrap()
}

fn wait_for_exit_code(rx: &StreamReceiver, session: &str, timeout: Duration) -> Option<i64> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(item) = rx.recv_timeout(Duration::from_millis(100)) {
            let v: Value = serde_json::from_str(&ndjson_line(&item)).expect("event is JSON");
            if v["event"] == json!("exit") && v["sessionId"] == json!(session) {
                return v["payload"]["code"].as_i64();
            }
        }
    }
    None
}

/// shellOverride is honored: spawning against a nonexistent override path fails,
/// where the default ($SHELL) would have succeeded — proof the override IS the
/// spawned program, not a silently-dropped field.
#[test]
fn shell_override_is_used_as_the_spawn_program() {
    let reg = Arc::new(Registry::new());
    let client = "c-override";

    let bogus = dispatch(
        &reg,
        client,
        json!({ "id": "o1", "type": "createOrAttach",
            "payload": { "sessionId": "s-override", "cols": 80, "rows": 24,
                "shellOverride": "/nonexistent/shell-xyz" } }),
    );
    assert_eq!(
        bogus["ok"],
        json!(false),
        "a bogus shellOverride must fail to spawn"
    );

    // Control: no override → the default shell spawns fine.
    let ok = dispatch(
        &reg,
        client,
        json!({ "id": "o2", "type": "createOrAttach",
            "payload": { "sessionId": "s-default", "cols": 80, "rows": 24 } }),
    );
    assert_eq!(ok["ok"], json!(true), "default shell must spawn");
    assert_eq!(ok["payload"]["isNew"], json!(true));
}

/// write to an unknown session fires a synthetic exit(-1) on the client's stream —
/// the only signal the renderer gets to clear a stale pane binding.
#[test]
fn write_to_unknown_session_emits_synthetic_exit() {
    let reg = Arc::new(Registry::new());
    let client = "c-ghost";
    let (tx, rx) = stream_channel();
    reg.register_stream(client.to_string(), tx);

    let resp = dispatch(
        &reg,
        client,
        json!({ "id": "notify_1", "type": "write",
            "payload": { "sessionId": "ghost", "data": "x" } }),
    );
    assert_eq!(resp["ok"], json!(false), "write to unknown session errors");
    assert_eq!(
        wait_for_exit_code(&rx, "ghost", Duration::from_secs(2)),
        Some(-1),
        "the renderer must receive a synthetic exit(-1) for the ghost session"
    );
}

/// resize to an unknown session likewise emits a synthetic exit(-1).
#[test]
fn resize_to_unknown_session_emits_synthetic_exit() {
    let reg = Arc::new(Registry::new());
    let client = "c-ghost2";
    let (tx, rx) = stream_channel();
    reg.register_stream(client.to_string(), tx);

    dispatch(
        &reg,
        client,
        json!({ "id": "notify_2", "type": "resize",
            "payload": { "sessionId": "ghost2", "cols": 100, "rows": 40 } }),
    );
    assert_eq!(
        wait_for_exit_code(&rx, "ghost2", Duration::from_secs(2)),
        Some(-1),
    );
}

/// getSize on an unknown session is null-not-throw (`{ size: null }`, ok), matching
/// the Node getAppliedSize the renderer's resume drift-check reads.
#[test]
fn get_size_unknown_session_is_null_not_error() {
    let reg = Arc::new(Registry::new());
    let resp = dispatch(
        &reg,
        "c-size",
        json!({ "id": "z1", "type": "getSize", "payload": { "sessionId": "nope" } }),
    );
    assert_eq!(
        resp["ok"],
        json!(true),
        "getSize must not error on unknown session"
    );
    assert_eq!(resp["payload"]["size"], Value::Null);
}

/// getCwd on an unknown session errors, matching the Node getAliveSession throw
/// (a KNOWN session with no resolvable cwd still returns ok+null).
#[test]
fn get_cwd_unknown_session_errors() {
    let reg = Arc::new(Registry::new());
    let resp = dispatch(
        &reg,
        "c-cwd",
        json!({ "id": "z2", "type": "getCwd", "payload": { "sessionId": "nope" } }),
    );
    assert_eq!(
        resp["ok"],
        json!(false),
        "getCwd must error on unknown session"
    );
}

/// kill_all_sessions (the shutdown killSessions=true path) force-kills a live child.
/// Tested via the registry API directly — dispatching `shutdown` would process::exit
/// the test runner.
#[test]
fn kill_all_sessions_reaps_a_live_child() {
    let reg = Arc::new(Registry::new());
    let client = "c-killall";
    let (tx, rx) = stream_channel();
    reg.register_stream(client.to_string(), tx);

    let created = dispatch(
        &reg,
        client,
        json!({ "id": "k1", "type": "createOrAttach",
            "payload": { "sessionId": "s-killall", "cols": 80, "rows": 24,
                "command": "sleep 30" } }),
    );
    assert_eq!(created["ok"], json!(true));

    reg.kill_all_sessions();
    let code = wait_for_exit_code(&rx, "s-killall", Duration::from_secs(5));
    assert!(
        code.is_some(),
        "the killed child must surface an exit event"
    );
    assert_ne!(code, Some(0), "a SIGKILL'd sleep must not report exit 0");
}

/// A graceful kill (immediate absent/false) still reaps the child: a plain shell
/// exits on SIGHUP, so the pump reaps it and emits an exit event.
#[test]
fn graceful_kill_reaps_the_child() {
    let reg = Arc::new(Registry::new());
    let client = "c-graceful";
    let (tx, rx) = stream_channel();
    reg.register_stream(client.to_string(), tx);

    dispatch(
        &reg,
        client,
        json!({ "id": "g1", "type": "createOrAttach",
            "payload": { "sessionId": "s-graceful", "cols": 80, "rows": 24,
                "command": "sleep 30" } }),
    );
    let killed = dispatch(
        &reg,
        client,
        json!({ "id": "g2", "type": "kill", "payload": { "sessionId": "s-graceful" } }),
    );
    assert_eq!(killed["ok"], json!(true));
    assert!(
        wait_for_exit_code(&rx, "s-graceful", Duration::from_secs(5)).is_some(),
        "a graceful kill must still reap the child"
    );
}
