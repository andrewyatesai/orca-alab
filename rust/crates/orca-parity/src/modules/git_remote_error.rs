//! Parity dispatch for `orca_text::git_remote_error` vs
//! `src/shared/git-remote-error.ts`.

use orca_text::git_remote_error::strip_credentials_from_message;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "stripCredentialsFromMessage" => match input.as_str() {
            // TS returns a plain string; mirror it as a JSON string.
            Some(message) => Value::String(strip_credentials_from_message(message)),
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "expected string input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
