//! Parity dispatch for `orca_core::wsl_paths` vs `src/shared/wsl-paths.ts`.

use orca_core::wsl_paths::{is_wsl_unc_path, parse_wsl_unc_path, WslUncPathInfo};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "parseWslUncPath" => match input.as_str() {
            Some(path) => info_to_json(parse_wsl_unc_path(path)),
            // Vectors always pass a string path; a non-string is a vector bug.
            None => json!({ "__parity_error__": "expected string input for parseWslUncPath" }),
        },
        "isWslUncPath" => match input.as_str() {
            Some(path) => Value::Bool(is_wsl_unc_path(path)),
            None => json!({ "__parity_error__": "expected string input for isWslUncPath" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `WslUncPathInfo | null` return: `null` when
/// no match, else `{ distro, linuxPath }` with the camelCase TS field names.
fn info_to_json(info: Option<WslUncPathInfo>) -> Value {
    match info {
        Some(info) => json!({
            "distro": info.distro,
            "linuxPath": info.linux_path,
        }),
        None => Value::Null,
    }
}
