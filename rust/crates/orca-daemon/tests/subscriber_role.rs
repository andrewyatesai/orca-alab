//! v1019 read-only SUBSCRIBER role: additional clients mirror a session without
//! stealing it. Covered here at dispatch level (rpc_lifecycle.rs pattern) with
//! manually-registered stream Senders: hydration snapshot on subscribe, live
//! fan-out to owner + subscribers, typed write/resize denial (followers pin to
//! the owner's grid — the SIGWINCH-bounce lesson), detach cleanup that never
//! touches the owner, and exit fan-out. The registry logic under test is
//! platform-independent; the tests drive a POSIX shell (printf markers), so the
//! file is unix-only like reattach_lifecycle.rs — the Windows lifecycle twin in
//! rpc_lifecycle.rs covers the shared RPC surface there.

#![cfg(unix)]

use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use orca_daemon::stream_coalescing::{encode_stream_item, StreamItem, StreamWireFormat};
use serde_json::{json, Value};
use orca_daemon::bounded_stream_channel::{stream_channel, StreamReceiver, StreamSender};
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

/// Scan a stream Receiver for a `data` event carrying `needle` (drains as it goes).
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

/// Drain everything currently queued on a Receiver into one string of data-event
/// payloads (for MUST-NOT-receive assertions).
fn drain_data(rx: &StreamReceiver, session: &str) -> String {
    let mut out = String::new();
    while let Ok(item) = rx.try_recv() {
        let v: Value = serde_json::from_str(&ndjson_line(&item)).expect("event JSON");
        if v["event"] == json!("data") && v["sessionId"] == json!(session) {
            out.push_str(v["payload"]["data"].as_str().unwrap_or(""));
        }
    }
    out
}

fn wait_for_exit(rx: &StreamReceiver, session: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(item) = rx.recv_timeout(Duration::from_millis(100)) {
            let v: Value = serde_json::from_str(&ndjson_line(&item)).expect("event JSON");
            if v["event"] == json!("exit") && v["sessionId"] == json!(session) {
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

/// Create a session owned by `owner` (stream registered), render `marker`, and
/// return the owner's stream Receiver.
fn create_marked_session(
    reg: &Arc<Registry>,
    owner: &str,
    session: &str,
    marker: &str,
) -> StreamReceiver {
    let (tx, rx) = stream_channel();
    reg.register_stream(owner.to_string(), tx);
    let created = dispatch(
        reg,
        owner,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": session, "cols": 80, "rows": 24,
                "command": format!("printf {marker}; sleep 30") } }),
    );
    assert_eq!(created["payload"]["isNew"], json!(true), "fresh session");
    assert!(
        wait_until(
            || snapshot_ansi_contains(reg, owner, session, marker),
            Duration::from_secs(10)
        ),
        "session should render its startup marker"
    );
    rx
}

fn register_stream(reg: &Arc<Registry>, client: &str) -> (StreamSender, StreamReceiver) {
    let (tx, rx) = stream_channel();
    reg.register_stream(client.to_string(), tx.clone());
    (tx, rx)
}

fn kill(reg: &Arc<Registry>, client: &str, session: &str) {
    dispatch(
        reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": session } }),
    );
}

/// The core non-steal contract: subscribe hydrates the follower with the same
/// engine serialize a reattach gets, then live output fans out to BOTH the owner
/// and the subscriber — ownership is not rebound (the verified createOrAttach
/// steal does not happen here).
#[test]
fn subscribe_hydrates_and_mirrors_without_stealing_ownership() {
    let reg = Arc::new(Registry::new());
    let (owner, sub, sid) = ("c-own", "c-sub", "s-sub1");
    let rx_own = create_marked_session(&reg, owner, sid, "MARKER_PRE");
    let (_tx_sub, rx_sub) = register_stream(&reg, sub);

    let subscribed = dispatch(
        &reg,
        sub,
        json!({ "id": "s", "type": "subscribe", "payload": { "sessionId": sid } }),
    );
    assert_eq!(subscribed["ok"], json!(true), "subscribe ok");
    let payload = &subscribed["payload"];
    assert!(
        payload["snapshot"]["snapshotAnsi"]
            .as_str()
            .is_some_and(|s| s.contains("MARKER_PRE")),
        "hydration snapshot repaints the pre-subscribe screen"
    );
    assert!(payload["pid"].is_number(), "hydration carries the pid");
    assert!(
        payload["shellState"].is_string(),
        "hydration carries shellState"
    );

    // Live output after subscribe reaches BOTH: the owner still receives (not
    // stolen), and the subscriber mirrors.
    dispatch(
        &reg,
        owner,
        json!({ "id": "w", "type": "write",
            "payload": { "sessionId": sid, "data": "printf LIVE_BOTH\n" } }),
    );
    assert!(
        wait_for_data(&rx_own, sid, "LIVE_BOTH", Duration::from_secs(5)),
        "the OWNER keeps receiving after a subscriber attaches (no steal)"
    );
    assert!(
        wait_for_data(&rx_sub, sid, "LIVE_BOTH", Duration::from_secs(5)),
        "the subscriber mirrors live output"
    );

    kill(&reg, owner, sid);
}

/// Subscribers have NO write/resize authority: both are rejected with the typed
/// `subscriber-read-only` error, the PTY grid stays pinned to the owner's dims
/// (no SIGWINCH bounce), and the owner's own write authority is untouched.
#[test]
fn subscriber_write_and_resize_rejected_typed() {
    let reg = Arc::new(Registry::new());
    let (owner, sub, sid) = ("c-own2", "c-sub2", "s-sub2");
    let _rx_own = create_marked_session(&reg, owner, sid, "MARKER_RO");
    let (_tx_sub, _rx_sub) = register_stream(&reg, sub);
    dispatch(
        &reg,
        sub,
        json!({ "id": "s", "type": "subscribe", "payload": { "sessionId": sid } }),
    );

    let denied_write = dispatch(
        &reg,
        sub,
        json!({ "id": "w", "type": "write",
            "payload": { "sessionId": sid, "data": "echo NOPE\n" } }),
    );
    assert_eq!(
        denied_write["ok"],
        json!(false),
        "subscriber write rejected"
    );
    assert!(
        denied_write["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("subscriber-read-only")),
        "write denial carries the typed error code: {denied_write}"
    );

    let denied_resize = dispatch(
        &reg,
        sub,
        json!({ "id": "r", "type": "resize",
            "payload": { "sessionId": sid, "cols": 120, "rows": 40 } }),
    );
    assert_eq!(
        denied_resize["ok"],
        json!(false),
        "subscriber resize rejected"
    );
    assert!(
        denied_resize["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("subscriber-read-only")),
        "resize denial carries the typed error code: {denied_resize}"
    );

    // The grid stays the OWNER's 80x24 — the denied resize never reached the PTY.
    let size = dispatch(
        &reg,
        owner,
        json!({ "id": "sz", "type": "getSize", "payload": { "sessionId": sid } }),
    );
    assert_eq!(size["payload"]["size"]["cols"], json!(80));
    assert_eq!(size["payload"]["size"]["rows"], json!(24));

    // Owner authority is untouched by the follower's denials.
    let owner_write = dispatch(
        &reg,
        owner,
        json!({ "id": "w2", "type": "write",
            "payload": { "sessionId": sid, "data": "printf OWNER_OK\n" } }),
    );
    assert_eq!(owner_write["ok"], json!(true), "owner write still allowed");

    kill(&reg, owner, sid);
}

/// Fan-out scales past one follower: two subscribers both mirror the owner's
/// output, the owner still receives, and on kill the exit event reaches all
/// three streams.
#[test]
fn two_subscribers_both_receive_and_get_exit() {
    let reg = Arc::new(Registry::new());
    let (owner, sid) = ("c-own3", "s-sub3");
    let rx_own = create_marked_session(&reg, owner, sid, "MARKER_TWO");
    let (_t1, rx_s1) = register_stream(&reg, "c-s1");
    let (_t2, rx_s2) = register_stream(&reg, "c-s2");
    for sub in ["c-s1", "c-s2"] {
        let r = dispatch(
            &reg,
            sub,
            json!({ "id": "s", "type": "subscribe", "payload": { "sessionId": sid } }),
        );
        assert_eq!(r["ok"], json!(true), "{sub} subscribe ok");
    }

    dispatch(
        &reg,
        owner,
        json!({ "id": "w", "type": "write",
            "payload": { "sessionId": sid, "data": "printf FAN_OUT_2\n" } }),
    );
    assert!(
        wait_for_data(&rx_own, sid, "FAN_OUT_2", Duration::from_secs(5)),
        "owner receives"
    );
    assert!(
        wait_for_data(&rx_s1, sid, "FAN_OUT_2", Duration::from_secs(5)),
        "subscriber 1 receives"
    );
    assert!(
        wait_for_data(&rx_s2, sid, "FAN_OUT_2", Duration::from_secs(5)),
        "subscriber 2 receives"
    );

    kill(&reg, owner, sid);
    assert!(
        wait_for_exit(&rx_own, sid, Duration::from_secs(10)),
        "owner gets exit"
    );
    assert!(
        wait_for_exit(&rx_s1, sid, Duration::from_secs(10)),
        "subscriber 1 gets exit"
    );
    assert!(
        wait_for_exit(&rx_s2, sid, Duration::from_secs(10)),
        "subscriber 2 gets exit"
    );
}

/// Subscriber detach — explicit `unsubscribe` or the stream-teardown purge
/// (`remove_subscriber_from_all`, what a socket disconnect runs) — stops the
/// mirror WITHOUT touching the owner: output keeps flowing to the owner, and a
/// reattach still finds the session live.
#[test]
fn owner_unaffected_by_subscriber_attach_and_detach() {
    let reg = Arc::new(Registry::new());
    let (owner, sub, sid) = ("c-own4", "c-sub4", "s-sub4");
    let rx_own = create_marked_session(&reg, owner, sid, "MARKER_DETACH");
    let (_tx_sub, rx_sub) = register_stream(&reg, sub);

    // Explicit unsubscribe.
    dispatch(
        &reg,
        sub,
        json!({ "id": "s", "type": "subscribe", "payload": { "sessionId": sid } }),
    );
    let unsub = dispatch(
        &reg,
        sub,
        json!({ "id": "u", "type": "unsubscribe", "payload": { "sessionId": sid } }),
    );
    assert_eq!(unsub["ok"], json!(true), "unsubscribe acks");
    dispatch(
        &reg,
        owner,
        json!({ "id": "w1", "type": "write",
            "payload": { "sessionId": sid, "data": "printf AFTER_UNSUB\n" } }),
    );
    assert!(
        wait_for_data(&rx_own, sid, "AFTER_UNSUB", Duration::from_secs(5)),
        "owner still receives after unsubscribe"
    );
    // The owner received this chunk, so route_output already ran for it; the
    // unsubscribed follower's queue must not contain it.
    assert!(
        !drain_data(&rx_sub, sid).contains("AFTER_UNSUB"),
        "an unsubscribed client receives nothing"
    );

    // Disconnect purge (what serve_stream teardown runs on socket close).
    dispatch(
        &reg,
        sub,
        json!({ "id": "s2", "type": "subscribe", "payload": { "sessionId": sid } }),
    );
    reg.unregister_stream(sub);
    reg.remove_subscriber_from_all(sub);
    dispatch(
        &reg,
        owner,
        json!({ "id": "w2", "type": "write",
            "payload": { "sessionId": sid, "data": "printf AFTER_DROP\n" } }),
    );
    assert!(
        wait_for_data(&rx_own, sid, "AFTER_DROP", Duration::from_secs(5)),
        "owner still receives after the subscriber's disconnect purge"
    );
    assert!(
        !drain_data(&rx_sub, sid).contains("AFTER_DROP"),
        "a disconnected subscriber's old channel receives nothing"
    );

    // The session itself is untouched: a reattach finds it live.
    let reattach = dispatch(
        &reg,
        owner,
        json!({ "id": "re", "type": "createOrAttach",
            "payload": { "sessionId": sid, "cols": 80, "rows": 24 } }),
    );
    assert_eq!(
        reattach["payload"]["isNew"],
        json!(false),
        "session survives subscriber churn"
    );

    kill(&reg, owner, sid);
}

#[test]
fn subscribe_unknown_session_errors() {
    let reg = Arc::new(Registry::new());
    let r = dispatch(
        &reg,
        "c-any",
        json!({ "id": "s", "type": "subscribe", "payload": { "sessionId": "nope" } }),
    );
    assert_eq!(
        r["ok"],
        json!(false),
        "subscribe to an unknown session errors"
    );
    // unsubscribe stays an idempotent ack even for an unknown session (detach parity).
    let u = dispatch(
        &reg,
        "c-any",
        json!({ "id": "u", "type": "unsubscribe", "payload": { "sessionId": "nope" } }),
    );
    assert_eq!(u["ok"], json!(true));
}
