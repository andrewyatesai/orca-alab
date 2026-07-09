//! Parity dispatch for `orca_core::hook_command_source_policy` vs
//! `src/shared/hook-command-source-policy.ts`.

use orca_core::hook_command_source_policy::{
    normalize_hook_command_source_policy, resolve_hook_command_source_policy,
};
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
        "resolveHookCommandSourcePolicy" => {
            // Tri-state: both an absent key and JSON `null` decode to `None` (the
            // absent-setting branch that can default to local-only when a local
            // script exists); a present but invalid string stays `Some(str)`, so
            // it parses to None yet does NOT take that branch — mirroring the TS
            // `policy === undefined` check where only undefined enables local-only.
            let policy = input.get("policy").and_then(Value::as_str);
            let has_local_script =
                input.get("hasLocalScript").and_then(Value::as_bool).unwrap_or(false);
            let resolved = resolve_hook_command_source_policy(policy, has_local_script);
            Value::String(resolved.as_wire().to_string())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
