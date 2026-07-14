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

// The cwd leg drives a POSIX shell (`printf` emitting OSC-7), so it is unix-only;
// the Windows lifecycle twin below covers the same RPC surface under cmd.exe, and
// OSC-7 cwd parsing itself is covered cross-platform by orca-terminal's parity tests.
#[cfg(unix)]
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
            r["payload"]["cwd"] == json!("/tmp/dtest")
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
    // Kitty keyboard flags ride the snapshot modes so a reattach re-anchors CSI-u
    // state — parity with the Node daemon's TerminalKittyKeyboardModeTracker.
    assert!(snapshot["modes"]["kittyKeyboardFlags"].is_number());

    // getSize returns the session grid.
    let size = dispatch(
        &reg,
        client,
        json!({ "id": "sz", "type": "getSize", "payload": { "sessionId": "s1" } }),
    );
    assert_eq!(size["payload"]["size"]["cols"], json!(88));
    assert_eq!(size["payload"]["size"]["rows"], json!(26));

    // listSessions shows the live session.
    let list = dispatch(&reg, client, json!({ "id": "ls", "type": "listSessions" }));
    let sessions = list["payload"]["sessions"].as_array().unwrap();
    assert!(
        sessions
            .iter()
            .any(|s| s["sessionId"] == json!("s1") && s["isAlive"] == json!(true)),
        "listSessions includes the live session"
    );

    // takePendingOutput returns the incremental checkpoint batch (types.ts
    // TakePendingOutputResult): typed records + monotonic seq, no snapshot unless
    // requested. The accumulated output records carry the printed marker.
    let pending = dispatch(
        &reg,
        client,
        json!({ "id": "tp", "type": "takePendingOutput", "payload": { "sessionId": "s1" } }),
    );
    assert_eq!(pending["ok"], json!(true));
    let records = pending["payload"]["records"].as_array().unwrap();
    assert!(
        records.iter().any(|r| r["kind"] == json!("output")
            && r["data"].as_str().is_some_and(|d| d.contains("MARKER_XYZ"))),
        "checkpoint records carry the printed marker"
    );
    assert!(
        pending["payload"]["seq"].as_u64().unwrap() >= 1,
        "seq is monotonic from 1"
    );
    assert_eq!(pending["payload"]["overflowed"], json!(false));
    assert_eq!(
        pending["payload"]["snapshot"],
        Value::Null,
        "no snapshot without includeSnapshot"
    );

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

/// Windows twin of the lifecycle test: same RPC surface (createOrAttach → write →
/// getSnapshot → getSize → listSessions → takePendingOutput → reattach → kill)
/// through a real ConPTY running cmd.exe. cmd has no OSC-7 channel, so getCwd is
/// asserted null rather than dropped.
#[cfg(windows)]
#[test]
fn create_write_snapshot_lifecycle_windows() {
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

    // Drive cmd.exe: echo a marker the ConPTY renders and the engine parses.
    dispatch(
        &reg,
        client,
        json!({
            "id": "w1", "type": "write",
            "payload": { "sessionId": "s1", "data": "echo MARKER_XYZ\r" }
        }),
    );

    // Poll getSnapshot (non-draining) until the rendered grid carries the marker;
    // ConPTY + cmd.exe startup can be slow on loaded machines, hence 10s.
    let marker_ready = wait_until(
        || {
            let r = dispatch(
                &reg,
                client,
                json!({ "id": "g", "type": "getSnapshot", "payload": { "sessionId": "s1" } }),
            );
            r["payload"]["snapshot"]["snapshotAnsi"]
                .as_str()
                .is_some_and(|s| s.contains("MARKER_XYZ"))
        },
        Duration::from_secs(10),
    );
    assert!(marker_ready, "snapshotAnsi carries the echoed marker");

    let snap = dispatch(
        &reg,
        client,
        json!({ "id": "g2", "type": "getSnapshot", "payload": { "sessionId": "s1" } }),
    );
    let snapshot = &snap["payload"]["snapshot"];
    assert_eq!(snapshot["cols"], json!(88));
    assert_eq!(snapshot["rows"], json!(26));
    assert!(snapshot["modes"]["applicationCursor"].is_boolean());

    // No OSC-7 was emitted, so the engine has no cwd: the RPC answers ok with null.
    let cwd = dispatch(
        &reg,
        client,
        json!({ "id": "cwd", "type": "getCwd", "payload": { "sessionId": "s1" } }),
    );
    assert_eq!(cwd["ok"], json!(true));
    assert_eq!(cwd["payload"]["cwd"], Value::Null);

    // getSize returns the session grid.
    let size = dispatch(
        &reg,
        client,
        json!({ "id": "sz", "type": "getSize", "payload": { "sessionId": "s1" } }),
    );
    assert_eq!(size["payload"]["size"]["cols"], json!(88));
    assert_eq!(size["payload"]["size"]["rows"], json!(26));

    // listSessions shows the live session.
    let list = dispatch(&reg, client, json!({ "id": "ls", "type": "listSessions" }));
    let sessions = list["payload"]["sessions"].as_array().unwrap();
    assert!(
        sessions
            .iter()
            .any(|s| s["sessionId"] == json!("s1") && s["isAlive"] == json!(true)),
        "listSessions includes the live session"
    );

    // takePendingOutput returns the incremental checkpoint batch (types.ts
    // TakePendingOutputResult): typed records + monotonic seq, no snapshot unless
    // requested. The accumulated output records carry the printed marker.
    let pending = dispatch(
        &reg,
        client,
        json!({ "id": "tp", "type": "takePendingOutput", "payload": { "sessionId": "s1" } }),
    );
    assert_eq!(pending["ok"], json!(true));
    let records = pending["payload"]["records"].as_array().unwrap();
    assert!(
        records.iter().any(|r| r["kind"] == json!("output")
            && r["data"].as_str().is_some_and(|d| d.contains("MARKER_XYZ"))),
        "checkpoint records carry the printed marker"
    );
    assert!(
        pending["payload"]["seq"].as_u64().unwrap() >= 1,
        "seq is monotonic from 1"
    );
    assert_eq!(pending["payload"]["overflowed"], json!(false));
    assert_eq!(
        pending["payload"]["snapshot"],
        Value::Null,
        "no snapshot without includeSnapshot"
    );

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

/// createOrAttach with a `historySeed` reseeds the fresh engine and reports
/// `historySeeded:true`, so the adapter re-anchors checkpointing (canReanchorHistory)
/// instead of suspending it — parity with the Node daemon's writeSync(historySeed)
/// path (session.ts / terminal-host.ts). The seed is fed to the engine grid, so a
/// later getSnapshot serializes the pre-crash history the adapter would otherwise
/// have lost. Cross-platform: the seed is fed into the engine directly (not through
/// the PTY), so its scrollback placement is deterministic regardless of the shell.
#[test]
fn create_with_history_seed_reseeds_engine_and_reports_seeded() {
    let reg = Arc::new(Registry::new());
    let client = "test-client";

    // Marker at the top, then enough newlines to scroll it out of the 24-row visible
    // grid and into scrollback — the live shell's startup output lands in the visible
    // grid and won't touch scrollback, so the assertion stays deterministic.
    let seed = format!("HISTORY_SEED_MARKER\r\n{}", "\r\n".repeat(30));
    let created = dispatch(
        &reg,
        client,
        json!({
            "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "seed1", "cols": 80, "rows": 24, "historySeed": seed }
        }),
    );
    assert_eq!(created["ok"], json!(true), "create ok");
    assert_eq!(created["payload"]["isNew"], json!(true), "isNew");
    assert_eq!(
        created["payload"]["historySeeded"],
        json!(true),
        "a non-empty historySeed reports historySeeded:true so the client re-anchors"
    );

    // The seed was fed synchronously before the reply, so the engine already carries
    // it: the recovered marker is in scrollback (what a full checkpoint serializes).
    let snap = dispatch(
        &reg,
        client,
        json!({ "id": "g", "type": "getSnapshot", "payload": { "sessionId": "seed1" } }),
    );
    let snapshot = &snap["payload"]["snapshot"];
    assert!(
        snapshot["scrollbackAnsi"]
            .as_str()
            .unwrap()
            .contains("HISTORY_SEED_MARKER"),
        "the seeded history survives into the serialized scrollback"
    );

    dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "seed1" } }),
    );
}

/// Without a `historySeed`, createOrAttach omits `historySeeded` entirely (undefined
/// on the wire, exactly as before) — preserving the adapter's suspend-fallback path
/// for the genuinely-unseedable case. An empty seed behaves the same as absent.
#[test]
fn create_without_history_seed_omits_seeded_flag() {
    let reg = Arc::new(Registry::new());
    let client = "test-client";

    let created = dispatch(
        &reg,
        client,
        json!({
            "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "noseed1", "cols": 80, "rows": 24 }
        }),
    );
    assert_eq!(created["ok"], json!(true), "create ok");
    assert_eq!(created["payload"]["isNew"], json!(true), "isNew");
    assert!(
        !created["payload"]
            .as_object()
            .unwrap()
            .contains_key("historySeeded"),
        "no seed → historySeeded key is omitted (undefined), not false"
    );

    // An explicit empty seed is treated as no seed: still omitted.
    let empty = dispatch(
        &reg,
        client,
        json!({
            "id": "c2", "type": "createOrAttach",
            "payload": { "sessionId": "noseed2", "cols": 80, "rows": 24, "historySeed": "" }
        }),
    );
    assert!(
        !empty["payload"]
            .as_object()
            .unwrap()
            .contains_key("historySeeded"),
        "empty seed → historySeeded key omitted, same as absent"
    );

    for sid in ["noseed1", "noseed2"] {
        dispatch(
            &reg,
            client,
            json!({ "id": "k", "type": "kill", "payload": { "sessionId": sid } }),
        );
    }
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
    // Guards the fork's daemon namespace: 1000+ is fork-reserved so a public
    // Orca build (v18) never handshakes with this daemon (see protocol.rs).
    assert_eq!(orca_daemon::protocol::PROTOCOL_VERSION, 1018);
}
