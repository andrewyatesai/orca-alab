//! Parity dispatch for `orca_core::hook_command_source_policy` vs
//! `src/shared/hook-command-source-policy.ts`.

use orca_core::hook_command_source_policy::normalize_hook_command_source_policy;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeHookCommandSourcePolicy" => {
            // The TS arg is `unknown`; only a string can match a policy id, so a
            // non-string input maps to `None` and falls back to `shared-only`,
            // matching the TS branch where any non-matching value returns it.
            let policy = normalize_hook_command_source_policy(input.as_str());
            Value::String(policy.as_wire().to_string())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
