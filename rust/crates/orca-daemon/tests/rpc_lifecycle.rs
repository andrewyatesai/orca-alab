//! Integration tests for the daemon's RPC lifecycle: drive `dispatch_request`
//! against a `Registry` (no socket) with real short-lived PTYs. This is the
//! durable form of the reattach/buffering + engine smoke tests, and the seed of
//! the differential parity corpus (sub-step 2 remainder).

use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use serde_json::{json, Value};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    let resp = dispatch_request(&req, reg, client);
    serde_json::from_str(&resp).expect("dispatch returns valid JSON")
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

#[test]
fn create_write_snapshot_cwd_lifecycle() {
    let reg = Arc::new(Registry::new());
    let client = "test-client";

    let created = dispatch(
        &reg,
        client,
        json!({
            "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s1", "cols": 88, "rows": 26 }
        }),
    );
    assert_eq!(created["ok"], json!(true), "create ok");
    assert_eq!(
        created["payload"]["isNew"],
        json!(true),
        "first create isNew"
    );

    // Drive the shell: set OSC-7 cwd + print a marker — both parsed by the aterm
    // engine the daemon feeds in-process (no napi hop).
    dispatch(
        &reg,
        client,
        json!({
            "id": "w1", "type": "write",
            "payload": { "sessionId": "s1",
                "data": "printf '\\033]7;file:///tmp/dtest\\007MARKER_XYZ\\n'\n" }
        }),
    );

    // Poll getCwd: it only becomes /tmp/dtest once printf actually ran and the engine
    // parsed the OSC-7 (a stronger readiness signal than the echoed command text).
    let cwd_ready = wait_until(
        || {
            let r = dispatch(
                &reg,
                client,
                json!({ "id": "cwd", "type": "getCwd", "payload": { "sessionId": "s1" } }),
            );
            r["payload"] == json!("/tmp/dtest")
        },
        Duration::from_secs(5),
    );
    assert!(
        cwd_ready,
        "getCwd should reflect the OSC-7 cwd the engine parsed"
    );

    // The rendered grid carries the marker.
    let snap = dispatch(
        &reg,
        client,
        json!({ "id": "g", "type": "getSnapshot", "payload": { "sessionId": "s1" } }),
    );
    let snapshot = &snap["payload"]["snapshot"];
    assert!(
        snapshot["snapshotAnsi"]
            .as_str()
            .unwrap()
            .contains("MARKER_XYZ"),
        "snapshotAnsi carries the printed marker"
    );
    assert_eq!(snapshot["cols"], json!(88));
    assert_eq!(snapshot["rows"], json!(26));
    assert_eq!(snapshot["cwd"], json!("/tmp/dtest"));
    assert!(snapshot["modes"]["applicationCursor"].is_boolean());

    // getSize returns the session grid.
    let size = dispatch(
        &reg,
        client,
        json!({ "id": "sz", "type": "getSize", "payload": { "sessionId": "s1" } }),
    );
    assert_eq!(size["payload"]["cols"], json!(88));
    assert_eq!(size["payload"]["rows"], json!(26));

    // listSessions shows the live session.
    let list = dispatch(&reg, client, json!({ "id": "ls", "type": "listSessions" }));
    let sessions = list["payload"]["sessions"].as_array().unwrap();
    assert!(
        sessions
            .iter()
            .any(|s| s["sessionId"] == json!("s1") && s["isAlive"] == json!(true)),
        "listSessions includes the live session"
    );

    // takePendingOutput drains the buffered raw bytes (no stream socket registered
    // in this dispatch-level test, so all output was buffered).
    let pending = dispatch(
        &reg,
        client,
        json!({ "id": "tp", "type": "takePendingOutput", "payload": { "sessionId": "s1" } }),
    );
    assert!(pending["payload"]["data"]
        .as_str()
        .unwrap()
        .contains("MARKER_XYZ"));

    // Reattach on the live id → isNew:false.
    let reattach = dispatch(
        &reg,
        client,
        json!({
            "id": "c2", "type": "createOrAttach",
            "payload": { "sessionId": "s1", "cols": 88, "rows": 26 }
        }),
    );
    assert_eq!(
        reattach["payload"]["isNew"],
        json!(false),
        "reattach isNew:false"
    );

    // Clean up the child.
    dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s1" } }),
    );
}

#[test]
fn ping_and_unknown_session_error() {
    let reg = Arc::new(Registry::new());
    let pong = dispatch(&reg, "c", json!({ "id": "p", "type": "ping" }));
    assert_eq!(pong["ok"], json!(true));
    assert_eq!(pong["payload"]["pong"], json!(true));

    let miss = dispatch(
        &reg,
        "c",
        json!({ "id": "w", "type": "write", "payload": { "sessionId": "nope", "data": "x" } }),
    );
    assert_eq!(miss["ok"], json!(false), "write to unknown session errors");
}

#[test]
fn protocol_version_is_pinned() {
    // Guards against an accidental bump away from the Node daemon's version.
    assert_eq!(orca_daemon::protocol::PROTOCOL_VERSION, 18);
}
