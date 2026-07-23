//! Finding #1 regression: PTY writes must NOT run under the global registry lock.
//! A child that stops draining its tty (SIGSTOP'd foreground process, `^S`/IXON, or
//! simply a foreground program that never reads stdin) fills the kernel PTY input
//! buffer, so the next large client `write` blocks inside `write_all`. If that write
//! is held under `Registry.inner`, every other session's pump + control RPC wedges
//! until the stuck child drains — potentially forever. The per-session writer lock
//! moves the blocking write off the global lock.

#![cfg(unix)]

use orca_daemon::bounded_stream_channel::stream_channel;
use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use serde_json::{json, Value};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    serde_json::from_str(&dispatch_request(&req, reg, client)).expect("valid JSON")
}

#[test]
fn a_blocked_pty_write_does_not_wedge_other_sessions() {
    let reg = Arc::new(Registry::new());
    let client = "c1";
    let (tx, _rx) = stream_channel();
    reg.register_stream(client.to_string(), tx);

    // A shell running `sleep` never drains its stdin, so writes to it fill the
    // kernel PTY input buffer and then block once it's full.
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": "stuck", "cols": 80, "rows": 24, "command": "sleep 60" } }),
    );
    assert_eq!(created["payload"]["isNew"], json!(true));

    // Flood the stuck session on a background thread. This blocks inside the PTY
    // write once the buffer fills; with the bug it blocks holding the registry lock.
    let flood_reg = Arc::clone(&reg);
    let flooder = thread::spawn(move || {
        let big = "x".repeat(8 * 1024 * 1024);
        dispatch_request(
            &json!({ "id": "notify_w", "type": "write",
                "payload": { "sessionId": "stuck", "data": big } }),
            &flood_reg,
            "c1",
        );
    });

    // Let the flood fill the buffer and park inside write_all.
    thread::sleep(Duration::from_millis(500));

    // An unrelated control RPC that needs the registry lock must complete promptly.
    // Run it on a thread with a bounded wait so a regression (write held under the
    // registry lock) surfaces as a clean assertion failure, not a hung test.
    let (done_tx, done_rx) = channel();
    let probe_reg = Arc::clone(&reg);
    thread::spawn(move || {
        let r = dispatch_request(
            &json!({ "id": "ls", "type": "listSessions" }),
            &probe_reg,
            "c1",
        );
        let _ = done_tx.send(r);
    });
    let completed = done_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    assert!(
        completed,
        "listSessions blocked behind a stuck PTY write — the registry lock was held across the blocking write"
    );

    // Cleanup: SIGKILL the child so the parked flood write unblocks and joins.
    dispatch(
        &reg,
        "c1",
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "stuck", "immediate": true } }),
    );
    let _ = flooder.join();
}
