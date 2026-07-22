//! Integration tests for the daemon shell-launch layer (staging audit F4b/F4c):
//! login-shell defaults for plain sessions, client-provided `shellArgs`,
//! stdin startup-command delivery, and the shell-ready barrier (marker
//! stripping, pre-ready stdin queue, timeout flush, teardown held-bytes
//! release). Dispatch-level with real PTYs, like rpc_lifecycle.rs.

#![cfg(unix)]

use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use orca_daemon::stream_coalescing::{encode_stream_item, StreamItem, StreamWireFormat};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    serde_json::from_str(&dispatch_request(&req, reg, client)).expect("valid JSON")
}

/// Encode a queued semantic item exactly as the NDJSON writer thread would, so
/// line-level assertions survive the semantic-queue migration unchanged.
fn ndjson_line(item: &StreamItem) -> String {
    String::from_utf8(encode_stream_item(item, StreamWireFormat::Ndjson)).unwrap()
}

/// Collect streamed `data` payloads for `session` until its `exit` event (or
/// the timeout). Lets tests observe short-lived children (e.g. /bin/echo)
/// whose sessions are reaped before a snapshot can be polled.
fn collect_stream_output(rx: &Receiver<StreamItem>, session: &str, timeout: Duration) -> String {
    let start = Instant::now();
    let mut out = String::new();
    while start.elapsed() < timeout {
        let Ok(item) = rx.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let v: Value = serde_json::from_str(&ndjson_line(&item)).expect("event is JSON");
        if v["sessionId"] != json!(session) {
            continue;
        }
        if v["event"] == json!("data") {
            out.push_str(v["payload"]["data"].as_str().unwrap_or_default());
        } else if v["event"] == json!("exit") {
            break;
        }
    }
    out
}

fn wait_until(mut pred: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(25));
    }
    pred()
}

fn snapshot_ansi(reg: &Arc<Registry>, client: &str, sid: &str) -> String {
    let snap = dispatch(
        reg,
        client,
        json!({ "id": "snap", "type": "getSnapshot", "payload": { "sessionId": sid } }),
    );
    snap["payload"]["snapshot"]["snapshotAnsi"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// F4b: a plain session (no command, no shellArgs) spawns the shell as a LOGIN
/// shell — observed by overriding the "shell" with /bin/echo, which prints its
/// argv: the `-l` must appear in the terminal.
#[test]
fn plain_session_spawns_a_login_shell() {
    let reg = Arc::new(Registry::new());
    let client = "c-login";
    let (tx, rx) = channel::<StreamItem>();
    reg.register_stream(client.to_string(), tx);
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "l1", "type": "createOrAttach",
            "payload": { "sessionId": "s-login", "cols": 80, "rows": 24,
                "shellOverride": "/bin/echo" } }),
    );
    assert_eq!(created["ok"], json!(true));
    assert_eq!(created["payload"]["shellState"], json!("unsupported"));
    let output = collect_stream_output(&rx, "s-login", Duration::from_secs(5));
    assert!(
        output.contains("-l"),
        "plain sessions must pass the login flag (echo argv was {output:?})"
    );
}

/// Client-provided shellArgs are used verbatim as the spawn argv.
#[test]
fn shell_args_from_the_payload_are_the_spawn_argv() {
    let reg = Arc::new(Registry::new());
    let client = "c-args";
    let (tx, rx) = channel::<StreamItem>();
    reg.register_stream(client.to_string(), tx);
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "a1", "type": "createOrAttach",
            "payload": { "sessionId": "s-args", "cols": 80, "rows": 24,
                "shellOverride": "/bin/echo",
                "shellArgs": ["WRAPPER_ARGV_MARKER"] } }),
    );
    assert_eq!(created["ok"], json!(true));
    let output = collect_stream_output(&rx, "s-args", Duration::from_secs(5));
    assert!(
        output.contains("WRAPPER_ARGV_MARKER"),
        "shellArgs must reach the child argv (echo argv was {output:?})"
    );
}

/// F4c (unsupported-shell path): with shellArgs present, the startup command is
/// delivered through stdin into the INTERACTIVE shell (not `-lc` argv), and
/// with no barrier it is written immediately.
#[test]
fn startup_command_is_delivered_via_stdin_with_shell_args() {
    let reg = Arc::new(Registry::new());
    let client = "c-stdin";
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "d1", "type": "createOrAttach",
            "payload": { "sessionId": "s-stdin", "cols": 80, "rows": 24,
                "shellOverride": "/bin/sh",
                "shellArgs": [],
                "command": "printf CMD_VIA_%s STDIN" } }),
    );
    assert_eq!(created["ok"], json!(true));
    assert!(
        wait_until(
            || snapshot_ansi(&reg, client, "s-stdin").contains("CMD_VIA_STDIN"),
            Duration::from_secs(5)
        ),
        "the stdin-delivered startup command must execute in the shell"
    );
    dispatch(
        &reg,
        client,
        json!({ "id": "dk", "type": "kill", "payload": { "sessionId": "s-stdin" } }),
    );
}

/// F4c: the shell-ready barrier holds the startup command until the wrapper's
/// OSC 777 marker appears, strips the marker from downstream output, and then
/// flushes the queue — writes issued pre-ready stay ordered behind the command.
#[test]
fn barrier_queues_stdin_until_the_ready_marker_then_flushes_in_order() {
    let reg = Arc::new(Registry::new());
    let client = "c-barrier";
    // The "shell" prints the ready marker after a beat, then echoes stdin back
    // (cat) — a deterministic stand-in for a wrapper rcfile reaching precmd.
    let script = "sleep 1; printf '\\033]777;orca-shell-ready\\007'; exec cat";
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "b1", "type": "createOrAttach",
            "payload": { "sessionId": "s-barrier", "cols": 100, "rows": 24,
                "shellOverride": "/bin/sh",
                "shellArgs": ["-c", script],
                "command": "FIRST_QUEUED",
                "shellReadySupported": true } }),
    );
    assert_eq!(created["ok"], json!(true));
    assert_eq!(
        created["payload"]["shellState"],
        json!("pending"),
        "a barrier session reports pending at create"
    );

    // A pre-ready write must queue BEHIND the startup command.
    dispatch(
        &reg,
        client,
        json!({ "id": "notify_w", "type": "write",
            "payload": { "sessionId": "s-barrier", "data": "SECOND_QUEUED\n" } }),
    );

    assert!(
        wait_until(
            || {
                let ansi = snapshot_ansi(&reg, client, "s-barrier");
                ansi.contains("FIRST_QUEUED") && ansi.contains("SECOND_QUEUED")
            },
            Duration::from_secs(10)
        ),
        "queued startup command + pre-ready write must flush after the marker (got {:?})",
        snapshot_ansi(&reg, client, "s-barrier")
    );
    let ansi = snapshot_ansi(&reg, client, "s-barrier");
    let first = ansi.find("FIRST_QUEUED").unwrap();
    let second = ansi.find("SECOND_QUEUED").unwrap();
    assert!(
        first < second,
        "flush order must preserve queue order: {ansi:?}"
    );
    assert!(
        !ansi.contains("777;orca-shell-ready"),
        "the marker must be stripped from downstream output"
    );

    // listSessions reports the live barrier state (ready after the flush).
    let list = dispatch(&reg, client, json!({ "id": "ls", "type": "listSessions" }));
    let state = list["payload"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["sessionId"] == json!("s-barrier"))
        .map(|s| s["shellState"].clone());
    assert_eq!(state, Some(json!("ready")));

    dispatch(
        &reg,
        client,
        json!({ "id": "bk", "type": "kill", "payload": { "sessionId": "s-barrier" } }),
    );
}

/// F4c: a shell that never emits the marker is bounded by shellReadyTimeoutMs —
/// the queue flushes on timeout and the state reports timed_out.
#[test]
fn barrier_timeout_flushes_the_queue() {
    let reg = Arc::new(Registry::new());
    let client = "c-timeout";
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "t1", "type": "createOrAttach",
            "payload": { "sessionId": "s-timeout", "cols": 100, "rows": 24,
                "shellOverride": "/bin/sh",
                "shellArgs": ["-c", "exec cat"],
                "command": "AFTER_TIMEOUT",
                "shellReadySupported": true,
                "shellReadyTimeoutMs": 200 } }),
    );
    assert_eq!(created["ok"], json!(true));
    assert!(
        wait_until(
            || snapshot_ansi(&reg, client, "s-timeout").contains("AFTER_TIMEOUT"),
            Duration::from_secs(10)
        ),
        "the queued command must flush once the ready wait times out"
    );
    let list = dispatch(&reg, client, json!({ "id": "ls", "type": "listSessions" }));
    let state = list["payload"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["sessionId"] == json!("s-timeout"))
        .map(|s| s["shellState"].clone());
    assert_eq!(state, Some(json!("timed_out")));
    dispatch(
        &reg,
        client,
        json!({ "id": "tk", "type": "kill", "payload": { "sessionId": "s-timeout" } }),
    );
}

/// A multibyte char split exactly across the barrier's scan→post-scan
/// transition must render as ONE glyph in the engine grid: while scanning, the
/// engine ate DECODED text (the utf8 decoder held the lead byte as carry), so
/// a raw-bytes engine feed after readiness would start with orphan
/// continuation bytes and corrupt the glyph in snapshots only.
#[test]
fn multibyte_char_split_across_ready_transition_stays_intact_in_the_engine() {
    let reg = Arc::new(Registry::new());
    let client = "c-utf8";
    // One write carries the marker + 'A' + the LEAD byte of é (\303); the
    // continuation byte (\251) + 'B' arrive in a later read, after readiness.
    let script =
        "printf '\\033]777;orca-shell-ready\\007A\\303'; sleep 1; printf '\\251B'; sleep 30";
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "u1", "type": "createOrAttach",
            "payload": { "sessionId": "s-utf8", "cols": 80, "rows": 24,
                "shellOverride": "/bin/sh",
                "shellArgs": ["-c", script],
                "shellReadySupported": true } }),
    );
    assert_eq!(created["ok"], json!(true));
    assert!(
        wait_until(
            || snapshot_ansi(&reg, client, "s-utf8").contains("A\u{e9}B"),
            Duration::from_secs(10)
        ),
        "the engine grid must join the char split across the ready transition (got {:?})",
        snapshot_ansi(&reg, client, "s-utf8")
    );
    dispatch(
        &reg,
        client,
        json!({ "id": "uk", "type": "kill", "payload": { "sessionId": "s-utf8" } }),
    );
}

/// A teardown take releases held partial-marker bytes as a post-checkpoint
/// record — session.ts prepareForFinalSnapshot (they aren't representable in
/// the snapshot, so they must survive as a log tail).
#[test]
fn teardown_take_releases_held_partial_marker_bytes() {
    let reg = Arc::new(Registry::new());
    let client = "c-teardown";
    // Emit ONLY a partial marker prefix, then idle: the scanner holds it.
    let script = "printf '\\033]777;orca-shell-rea'; sleep 30";
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "h1", "type": "createOrAttach",
            "payload": { "sessionId": "s-teardown", "cols": 80, "rows": 24,
                "shellOverride": "/bin/sh",
                "shellArgs": ["-c", script],
                "shellReadySupported": true,
                "shellReadyTimeoutMs": 60000 } }),
    );
    assert_eq!(created["ok"], json!(true));

    // Wait for the partial prefix to be produced and withheld: records must
    // stay clear of it while the scanner holds the bytes.
    sleep(Duration::from_millis(1500));
    let take = dispatch(
        &reg,
        client,
        json!({ "id": "h2", "type": "takePendingOutput",
            "payload": { "sessionId": "s-teardown",
                "includeSnapshot": true, "teardownSnapshot": true } }),
    );
    assert_eq!(take["ok"], json!(true));
    let records = take["payload"]["records"].as_array().unwrap();
    assert!(
        records.iter().any(|r| r["kind"] == json!("output")
            && r["data"]
                .as_str()
                .is_some_and(|d| d.contains("777;orca-shell-rea"))),
        "held partial-marker bytes must be released as a post-checkpoint record: {records:?}"
    );
    dispatch(
        &reg,
        client,
        json!({ "id": "hk", "type": "kill", "payload": { "sessionId": "s-teardown" } }),
    );
}
