//! Parity dispatch for `orca_core::commit_message_host_key` vs
//! `src/shared/commit-message-host-key.ts`.

use orca_core::commit_message_host_key::get_commit_message_model_discovery_host_key_for_scope;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "getCommitMessageModelDiscoveryHostKeyForScope" => {
            // TS arg is `string | null | undefined`; the JSON input mirrors
            // `JSON.stringify` of the call: an absent `scope` key ≙ undefined
            // (→ unknown), explicit null ≙ TS null (falsy → local, which Rust
            // folds into the empty-string case).
            let scope = match input.get("scope") {
                None => None,
                Some(Value::Null) => Some(""),
                Some(v) => v.as_str(),
            };
            Value::String(get_commit_message_model_discovery_host_key_for_scope(scope))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
