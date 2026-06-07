//! Parity dispatch for `orca_git::repo_clone_path` vs
//! `src/main/git/repo-clone-path.ts`.
//!
//! `deriveValidatedClonePath` throws on invalid input and returns the clone path
//! string on success; the Rust port returns `Result<String, String>` with the
//! same messages. Both are shaped into `{ clonePath }` / `{ error }` so the JSON
//! images are equal. `getClonePathComparisonKey` returns a plain string.

use orca_core::cross_platform_path::PathFlavor;
use orca_git::repo_clone_path::{derive_validated_clone_path, get_clone_path_comparison_key};
use serde_json::{json, Value};

/// The TS `deriveValidatedClonePath` reads `process.platform`; mirror the host so
/// the Rust port and the live TS agree. The goldens assume a posix host.
fn host_path_flavor() -> PathFlavor {
    if std::env::consts::OS == "windows" {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    }
}

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "deriveValidatedClonePath" => {
            let url = input.get("url").and_then(Value::as_str).unwrap_or("");
            let destination = input.get("destination").and_then(Value::as_str).unwrap_or("");
            match derive_validated_clone_path(url, destination, host_path_flavor()) {
                Ok(clone_path) => json!({ "clonePath": clone_path }),
                Err(message) => json!({ "error": message }),
            }
        }
        "getClonePathComparisonKey" => {
            let clone_path = input.get("clonePath").and_then(Value::as_str).unwrap_or("");
            Value::String(get_clone_path_comparison_key(clone_path))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
