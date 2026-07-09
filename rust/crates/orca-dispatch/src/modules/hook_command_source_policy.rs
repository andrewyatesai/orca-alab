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
            // Match the TS `policy === undefined` gate exactly: ONLY an absent key
            // (JS `undefined`, which JSON.stringify omits) enables the local-only
            // default. A present `null`/non-string is not `undefined`, so it must
            // resolve to shared-only — map it to an invalid string so the core's
            // `is_none()` (absent) branch is skipped and it falls through.
            let policy: Option<String> = match input.get("policy") {
                None => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => Some(String::new()),
            };
            let has_local_script =
                input.get("hasLocalScript").and_then(Value::as_bool).unwrap_or(false);
            let resolved = resolve_hook_command_source_policy(policy.as_deref(), has_local_script);
            Value::String(resolved.as_wire().to_string())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
