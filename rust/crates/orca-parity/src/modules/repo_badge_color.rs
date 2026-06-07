//! Parity dispatch for `orca_core::repo_badge_color` vs
//! `src/shared/repo-badge-color.ts`.

use orca_core::repo_badge_color::{normalize_repo_badge_color, resolve_repo_badge_color};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // TS returns `string | null`; `JSON.stringify` keeps the literal `null`,
        // so a non-hex value maps to `Value::Null`, not an omitted key.
        "normalizeRepoBadgeColor" => match normalize_repo_badge_color(&string_field(input, "value"))
        {
            Some(hex) => Value::String(hex),
            None => Value::Null,
        },
        "resolveRepoBadgeColor" => {
            Value::String(resolve_repo_badge_color(&string_field(input, "value")))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Reads a string argument from the vector input object. Vectors always carry
/// the key, so a missing one is a vector bug; default to empty rather than panic.
/// Non-string also coerces to empty, mirroring the TS `typeof value !== 'string'`
/// guard which yields the same rejected/default result.
fn string_field(input: &Value, key: &str) -> String {
    input
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
