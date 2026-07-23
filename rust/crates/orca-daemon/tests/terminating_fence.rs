//! Finding #2 regression: a session mid-teardown must be FENCED from reattach.
//! A graceful `kill` sends SIGHUP and arms a 5s SIGKILL escalation. Without a
//! terminating fence, a createOrAttach for the same stable sessionId within that
//! window rebinds the dying shell and returns a real snapshot — then at T+5s the
//! escalation SIGKILLs the freshly-reopened pane. createOrAttach must instead error
//! ("Session is terminating"), matching terminal-host.ts's isTerminating fence.

#![cfg(unix)]

use orca_daemon::bounded_stream_channel::stream_channel;
use orca_daemon::registry::Registry;
use orca_daemon::rpc::dispatch_request;
use serde_json::{json, Value};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn dispatch(reg: &Arc<Registry>, client: &str, req: Value) -> Value {
    serde_json::from_str(&dispatch_request(&req, reg, client)).expect("valid JSON")
}

#[test]
fn graceful_kill_fences_reattach_as_terminating() {
    let reg = Arc::new(Registry::new());
    let client = "c1";
    let (tx, _rx) = stream_channel();
    reg.register_stream(client.to_string(), tx);

    // A shell that IGNORES SIGHUP stays alive through the graceful-kill window — the
    // exact class of session the escalation (and this fence) exists for.
    let created = dispatch(
        &reg,
        client,
        json!({ "id": "c", "type": "createOrAttach",
            "payload": { "sessionId": "s", "cols": 80, "rows": 24,
                "command": "trap '' HUP; sleep 30" } }),
    );
    assert_eq!(created["payload"]["isNew"], json!(true));
    // Give the shell time to install the HUP trap before we signal it.
    thread::sleep(Duration::from_millis(400));

    // Graceful kill: sets `terminating`, sends SIGHUP (ignored), arms the 5s SIGKILL.
    let killed = dispatch(
        &reg,
        client,
        json!({ "id": "k", "type": "kill", "payload": { "sessionId": "s" } }),
    );
    assert_eq!(killed["ok"], json!(true));

    // Reattaching the SAME id within the window must be REJECTED — not reattached to
    // the dying shell (which the 5s SIGKILL would tear down under the user), and not
    // spawned fresh (which would orphan the dying child and let its reap remove the
    // fresh entry).
    let re = dispatch(
        &reg,
        client,
        json!({ "id": "re", "type": "createOrAttach",
            "payload": { "sessionId": "s", "cols": 80, "rows": 24 } }),
    );
    assert_eq!(re["ok"], json!(false), "reattach to a terminating session is rejected");
    assert!(
        re["error"].as_str().unwrap_or("").contains("terminating"),
        "error names the terminating fence: {re:?}"
    );
    assert!(
        re.get("payload").is_none(),
        "a fenced reattach must NOT spawn a fresh session"
    );

    // Cleanup.
    dispatch(
        &reg,
        client,
        json!({ "id": "k2", "type": "kill", "payload": { "sessionId": "s", "immediate": true } }),
    );
}
