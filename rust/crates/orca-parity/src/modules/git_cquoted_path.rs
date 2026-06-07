//! Parity dispatch for `orca_core::git_cquoted_path` vs
//! `src/shared/git-cquoted-path.ts`.

use orca_core::git_cquoted_path::decode_git_cquoted_path;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "decodeGitCQuotedPath" => match input.as_str() {
            Some(value) => Value::String(decode_git_cquoted_path(value)),
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "expected string input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
