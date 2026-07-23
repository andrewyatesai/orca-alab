//! Warm-reattach + session-lifecycle regression tests — the cluster the audit
//! caught. The Node daemon rebinds a reattached session to the reconnecting client
//! and returns its live snapshot; a fresh app launch mints a NEW clientId, so
//! cross-client reattach MUST route live output + replay backlog to the new client
//! and repaint from the engine. Exited sessions must be reaped (removed), not kept
//! as zombies. Dispatch-level with manually-registered stream Senders.

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

fn wait_until(mut pred: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    pred()
}

/// Scan a stream Receiver's `data` events for `needle` until seen or timeout.
fn wait_for_data(rx: &StreamReceiver, session: &str, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(item) = rx.recv_timeout(Duration::from_millis(100)) {
            let v: Value = serde_json::from_str(&ndjson_line(&item)).expect("event JSON");
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

fn snapshot_ansi_contains(reg: &Arc<Registry>, client: &str, session: &str, needle: &str) -> bool {
    let snap = dispatch(
        reg,
        client,
        json!({ "id": "g", "type": "getSnapshot", "payload": { "sessionId": session } }),
    );
    snap["payload"]["snapshot"]["snapshotAnsi"]
        .as_str()
        .is_some_and(|s| s.contains(needle))
}

/// The full warm-reattach path: create under C1, detach, reattach under a DIFFERENT
/// client C2. The reattach must (1) return a real snapshot carrying BOTH the
/// pre-detach screen and the output produced while detached (the engine stays
/// current), and (2) rebind live routing so post-reattach output reaches C2 — the
/// behavior a blank snapshot + stale client_id used to break.
#[test]
fn cross_client_reattach_rebinds_routing_and_repaints() {
    let reg = Arc::new(Registry::new());

    // C1 (first app instance) creates a long-lived session and streams it.
    let c1 = "client-1";
    let (tx1, _rx1) = stream_channel();
    reg.register_stream(c1.to_string(), tx1);
    let created = dispatch(
        &reg,
        c1,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": "s-re", "cols": 80, "rows": 24,
                "command": "printf MARKER_PRE; sleep 30" } }),
    );
    assert_eq!(created["payload"]["isNew"], json!(true));
    assert!(
        wait_until(
            || snapshot_ansi_contains(&reg, c1, "s-re", "MARKER_PRE"),
            Duration::from_secs(10)
        ),
        "session should render its pre-detach marker"
    );

    // C1 detaches (app quit): its stream goes away, so live output is dropped — but
    // the pump keeps feeding the engine, so the state survives for the snapshot.
    reg.unregister_stream(c1);
    dispatch(
        &reg,
        c1,
        json!({ "id": "w", "type": "write",
            "payload": { "sessionId": "s-re", "data": "printf DETACHED_OUT\n" } }),
    );
    assert!(
        wait_until(
            || snapshot_ansi_contains(&reg, c1, "s-re", "DETACHED_OUT"),
            Duration::from_secs(10)
        ),
        "detached output should still reach the engine"
    );

    // C2 (relaunched app) — a NEW clientId. Register its stream, then reattach.
    let c2 = "client-2";
    let (tx2, rx2) = stream_channel();
    reg.register_stream(c2.to_string(), tx2);
    let reattach = dispatch(
        &reg,
        c2,
        json!({ "id": "re", "type": "createOrAttach",
            "payload": { "sessionId": "s-re", "cols": 80, "rows": 24 } }),
    );
    assert_eq!(
        reattach["payload"]["isNew"],
        json!(false),
        "reattach to the live session"
    );
    // (1) real snapshot — authoritative: it repaints BOTH the pre-detach screen and
    // the output produced while detached (every byte was teed into the engine). The
    // raw backlog is NOT also replayed as data events — that would double-apply it.
    let snap = &reattach["payload"]["snapshot"];
    assert!(snap.is_object(), "reattach returns a real snapshot");
    let ansi = snap["snapshotAnsi"].as_str().unwrap();
    assert!(
        ansi.contains("MARKER_PRE"),
        "snapshot repaints the pre-detach screen"
    );
    assert!(
        ansi.contains("DETACHED_OUT"),
        "snapshot includes output produced while detached"
    );
    // (2) live output produced AFTER reattach routes to C2 (routing rebound off the
    // now-dead C1). This is the byte the old stale-client_id bug never delivered.
    dispatch(
        &reg,
        c2,
        json!({ "id": "w2", "type": "write",
            "payload": { "sessionId": "s-re", "data": "printf LIVE_TO_C2\n" } }),
    );
    assert!(
        wait_for_data(&rx2, "s-re", "LIVE_TO_C2", Duration::from_secs(5)),
        "post-reattach live output must route to the rebound client"
    );

    dispatch(
        &reg,
        c2,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-re" } }),
    );
}

/// A session whose shell exited while detached must be reaped: gone from
/// listSessions, and createOrAttach on that id spawns a FRESH shell (isNew:true)
/// rather than handing back a dead zombie with a stale pid.
#[test]
fn exited_session_is_reaped_and_recreated_fresh() {
    let reg = Arc::new(Registry::new());
    let client = "client-x";

    let first = dispatch(
        &reg,
        client,
        json!({ "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "s-z", "cols": 80, "rows": 24, "command": "exit 7" } }),
    );
    assert_eq!(first["payload"]["isNew"], json!(true));
    let first_pid = first["payload"]["pid"].clone();

    // The shell exits on its own; the reaper removes the session from the map.
    let gone = wait_until(
        || {
            let list = dispatch(&reg, client, json!({ "id": "ls", "type": "listSessions" }));
            let sessions = list["payload"]["sessions"].as_array().unwrap();
            !sessions.iter().any(|s| s["sessionId"] == json!("s-z"))
        },
        Duration::from_secs(10),
    );
    assert!(
        gone,
        "an exited session must be reaped out of listSessions (no zombie)"
    );

    // Recreating the id spawns a fresh shell — a new pid, isNew:true.
    let second = dispatch(
        &reg,
        client,
        json!({ "id": "c2", "type": "createOrAttach",
            "payload": { "sessionId": "s-z", "cols": 80, "rows": 24, "command": "sleep 30" } }),
    );
    assert_eq!(
        second["payload"]["isNew"],
        json!(true),
        "recreate after exit spawns fresh"
    );
    assert_ne!(
        second["payload"]["pid"], first_pid,
        "fresh shell has a new pid"
    );

    dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-z" } }),
    );
}
