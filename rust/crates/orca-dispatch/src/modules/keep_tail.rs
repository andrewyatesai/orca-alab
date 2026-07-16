//! Parity dispatch for `orca_flow_control::keep_tail` vs the TS twin
//! `src/main/daemon/daemon-stream-keep-tail-drop.ts`.
//!
//! Second F4 seam promotion (after `provider-backoff`), proving the promote leg
//! generalizes across kernel classes: this pair is a u64 **division + clamp**
//! (`clamp(BUDGET / max(1, n), [MIN, MAX])`) plus a `keepTail * 2` drop cap —
//! distinct from provider-backoff's shift/saturate. Both sides close over the
//! budget/floor/ceiling constants, so each function takes only the session count;
//! the compared pair differs only in language. Additive parity wiring — no call
//! site is cut over (the shipping daemon keep-tail stays TS).

use orca_flow_control::keep_tail::{
    background_session_drop_cap_chars, background_session_keep_tail_chars,
};
use serde_json::{json, Value};

/// A droppable-session count is non-negative; a negative/absent/non-integer input
/// reads as 0, matching the TS `Math.max(0, …)` guard (both then feed `max(1, n)`).
fn droppable_sessions(input: &Value) -> u64 {
    input
        .get("droppableSessions")
        .and_then(Value::as_i64)
        .map_or(0, |n| n.max(0) as u64)
}

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "backgroundSessionKeepTailChars" => {
            json!(background_session_keep_tail_chars(droppable_sessions(input)))
        }
        "backgroundSessionDropCapChars" => {
            json!(background_session_drop_cap_chars(droppable_sessions(input)))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
