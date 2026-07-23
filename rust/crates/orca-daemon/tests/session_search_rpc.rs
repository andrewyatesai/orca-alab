//! Integration: the v1021 federated-search RPCs through `dispatch_request`
//! against a real PTY-backed session — the daemon-side leg of fed design
//! §2.3 (warm search + context) and §2.2 (cold replay handshake).

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

#[cfg(unix)]
#[test]
fn warm_session_search_and_context_over_the_rpc_surface() {
    let reg = Arc::new(Registry::new());
    let client = "search-client";
    let created = dispatch(
        &reg,
        client,
        json!({
            "id": "c1", "type": "createOrAttach",
            "payload": { "sessionId": "search-s1", "cols": 80, "rows": 24 }
        }),
    );
    assert_eq!(created["ok"], json!(true));

    dispatch(
        &reg,
        client,
        json!({
            "id": "w1", "type": "write",
            "payload": { "sessionId": "search-s1",
                "data": "printf 'FED_SEARCH_MARKER one\\nFED_SEARCH_MARKER two\\n'\n" }
        }),
    );

    // Poll until BOTH printed lines are matched (the echoed command text can
    // add a third — total >= 2 is the readiness signal).
    let mut last = json!(null);
    let found = wait_until(
        || {
            last = dispatch(
                &reg,
                client,
                json!({
                    "id": "q1", "type": "searchSessions",
                    "payload": { "query": "FED_SEARCH_MARKER", "gen": 42 }
                }),
            );
            last["payload"]["sessions"]
                .as_array()
                .is_some_and(|s| !s.is_empty() && s[0]["total"].as_u64().unwrap_or(0) >= 2)
        },
        Duration::from_secs(5),
    );
    assert!(found, "searchSessions should find the printed markers: {last}");
    assert_eq!(last["payload"]["gen"], json!(42), "gen echoes for cancellation");
    let session = &last["payload"]["sessions"][0];
    assert_eq!(session["sessionId"], json!("search-s1"));
    let first_match = &session["matches"][0];
    assert!(
        first_match["line"].as_str().unwrap().contains("FED_SEARCH_MARKER"),
        "summary carries the matched line: {first_match}"
    );

    // Context expansion around the newest match resolves on the same rows.
    let abs_row = first_match["absRow"].as_u64().expect("absRow");
    let ctx = dispatch(
        &reg,
        client,
        json!({
            "id": "q2", "type": "searchContext",
            "payload": { "sessionId": "search-s1", "absRow": abs_row, "before": 2, "after": 2 }
        }),
    );
    assert_eq!(ctx["ok"], json!(true));
    let lines = ctx["payload"]["lines"].as_array().expect("lines");
    assert!(
        lines.iter().any(|l| l.as_str().unwrap_or("").contains("FED_SEARCH_MARKER")),
        "context window contains the matched line: {lines:?}"
    );

    dispatch(
        &reg,
        client,
        json!({ "id": "k1", "type": "kill",
            "payload": { "sessionId": "search-s1", "immediate": true } }),
    );
}

#[test]
fn cold_replay_handshake_over_the_rpc_surface() {
    let reg = Arc::new(Registry::new());
    let client = "cold-client";
    // Miss without content → needsContent (never an error).
    let miss = dispatch(
        &reg,
        client,
        json!({
            "id": "r1", "type": "searchReplay",
            "payload": { "sessionId": "dead-sess", "generation": 9, "query": "needle" }
        }),
    );
    assert_eq!(miss["payload"]["needsContent"], json!(true));
    // Ship the persisted ANSI once; matches come back as summaries.
    let hit = dispatch(
        &reg,
        client,
        json!({
            "id": "r2", "type": "searchReplay",
            "payload": {
                "sessionId": "dead-sess", "generation": 9, "query": "needle",
                "content": { "rows": 4, "cols": 40,
                    "chunks": ["\u{1b}[33mcold needle\u{1b}[0m\r\nafter\r\n"] }
            }
        }),
    );
    assert_eq!(hit["ok"], json!(true));
    assert_eq!(hit["payload"]["total"], json!(1));
    assert_eq!(hit["payload"]["matches"][0]["line"], json!("cold needle"));
    // Dead-session inline expansion straight from the generation cache.
    let ctx = dispatch(
        &reg,
        client,
        json!({
            "id": "r3", "type": "searchReplayContext",
            "payload": { "sessionId": "dead-sess", "generation": 9,
                "absRow": hit["payload"]["matches"][0]["absRow"], "before": 1, "after": 1 }
        }),
    );
    assert_eq!(ctx["payload"]["needsContent"], json!(false));
    assert!(
        ctx["payload"]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "cold needle"),
        "expansion window: {}",
        ctx["payload"]["lines"]
    );
}
