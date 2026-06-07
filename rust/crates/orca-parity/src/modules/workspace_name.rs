//! Parity dispatch for `orca_text::workspace_name` vs
//! `src/shared/workspace-name.ts`.

use orca_text::workspace_name::slugify_for_workspace_name;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single string arg; returns the git-ref-safe workspace seed slug.
        "slugifyForWorkspaceName" => match input.as_str() {
            Some(text) => Value::String(slugify_for_workspace_name(text)),
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "slugifyForWorkspaceName expects a string input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
