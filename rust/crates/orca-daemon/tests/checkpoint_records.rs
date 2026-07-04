//! takePendingOutput returns the real TakePendingOutputResult (types.ts): typed
//! output/resize/clear records with a monotonic seq, and — only when includeSnapshot
//! is set — a full snapshot in place of the records (the snapshot supersedes the
//! incremental log). The old daemon faked `{ data: string }`, silently breaking the
//! client's incremental checkpoint / cold-restore history.

#![cfg(unix)]

use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    serde_json::from_str(&dispatch_request(&req, reg, client)).expect("valid JSON")
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

fn take(reg: &Arc<Registry>, client: &str, session: &str, include_snapshot: bool) -> Value {
    dispatch(
        reg,
        client,
        json!({ "id": "tp", "type": "takePendingOutput",
            "payload": { "sessionId": session, "includeSnapshot": include_snapshot } }),
    )
}

#[test]
fn incremental_records_seq_and_control_events() {
    let reg = Arc::new(Registry::new());
    let client = "c-ckpt";

    dispatch(
        &reg,
        client,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": "s-ck", "cols": 80, "rows": 24,
                "command": "printf CKPT_MARKER; sleep 30" } }),
    );
    // Wait until the marker has been recorded (the pump fed the engine + records).
    assert!(
        wait_until(
            || {
                let r = take(&reg, client, "s-ck", false);
                r["payload"]["records"]
                    .as_array()
                    .is_some_and(|recs| recs.iter().any(|x| x["data"].as_str().is_some_and(|d| d.contains("CKPT_MARKER"))))
            },
            Duration::from_secs(10),
        ),
        "an output record must carry the printed marker"
    );

    // A resize and a clearScrollback become typed records, interleaved with output.
    dispatch(
        &reg,
        client,
        json!({ "id": "rs", "type": "resize", "payload": { "sessionId": "s-ck", "cols": 100, "rows": 30 } }),
    );
    dispatch(
        &reg,
        client,
        json!({ "id": "cl", "type": "clearScrollback", "payload": { "sessionId": "s-ck" } }),
    );
    let batch = take(&reg, client, "s-ck", false);
    let records = batch["payload"]["records"].as_array().unwrap();
    assert!(
        records
            .iter()
            .any(|r| r["kind"] == json!("resize") && r["cols"] == json!(100) && r["rows"] == json!(30)),
        "resize is recorded as a typed record"
    );
    assert!(
        records.iter().any(|r| r["kind"] == json!("clear")),
        "clearScrollback is recorded as a typed record"
    );
    // seq is monotonic across takes; a drained batch resets to empty.
    let s1 = batch["payload"]["seq"].as_u64().unwrap();
    let empty = take(&reg, client, "s-ck", false);
    assert!(empty["payload"]["records"].as_array().unwrap().is_empty(), "drained batch resets");
    assert!(empty["payload"]["seq"].as_u64().unwrap() > s1, "seq advances each take");

    dispatch(&reg, client, json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-ck" } }));
}

#[test]
fn include_snapshot_supersedes_records() {
    let reg = Arc::new(Registry::new());
    let client = "c-snap-ck";

    dispatch(
        &reg,
        client,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": "s-sk", "cols": 80, "rows": 24,
                "command": "printf SNAP_CKPT; sleep 30" } }),
    );
    assert!(
        wait_until(
            || {
                let snap = dispatch(
                    &reg,
                    client,
                    json!({ "id": "g", "type": "getSnapshot", "payload": { "sessionId": "s-sk" } }),
                );
                snap["payload"]["snapshot"]["snapshotAnsi"].as_str().is_some_and(|s| s.contains("SNAP_CKPT"))
            },
            Duration::from_secs(10),
        ),
        "the engine renders the marker"
    );

    // A full-snapshot checkpoint returns the snapshot and DROPS the incremental
    // records (they're redundant with the snapshot), matching session.ts.
    let full = take(&reg, client, "s-sk", true);
    assert!(
        full["payload"]["snapshot"]["snapshotAnsi"].as_str().unwrap().contains("SNAP_CKPT"),
        "includeSnapshot returns the full snapshot"
    );
    assert!(
        full["payload"]["records"].as_array().unwrap().is_empty(),
        "records are empty when includeSnapshot supersedes them"
    );

    dispatch(&reg, client, json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s-sk" } }));
}

/// takePendingOutput for a missing/just-reaped session returns ok+null, NOT an
/// error — matching the Node host's null-not-throw. The client's checkpoint loop
/// treats null as "done" (`if (!take) return 'done'`); an error would spuriously log
/// a checkpoint failure and leave the session dirty until its exit event lands. This
/// races with reap on real exits, so it must never surface as an RPC error.
#[test]
fn take_pending_output_on_unknown_session_is_ok_null() {
    let reg = Arc::new(Registry::new());
    let r = take(&reg, "c-none", "no-such-session", false);
    assert_eq!(r["ok"], json!(true), "unknown session must be ok, not an error");
    assert_eq!(r["payload"], Value::Null, "unknown session payload is null (null-not-throw)");

    // Same for the includeSnapshot variant — null supersedes everything.
    let r2 = take(&reg, "c-none", "no-such-session", true);
    assert_eq!(r2["ok"], json!(true));
    assert_eq!(r2["payload"], Value::Null);
}
