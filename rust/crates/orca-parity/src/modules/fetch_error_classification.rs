//! Parity dispatch for `orca_git::fetch_error_classification` vs
//! `src/main/git/fetch-error-classification.ts`.

use orca_git::fetch_error_classification::is_missing_remote_ref_git_error;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isMissingRemoteRefGitError" => {
            // TS resolves the message via String(error); vectors carry it directly.
            let message = input.as_str().unwrap_or_default();
            Value::Bool(is_missing_remote_ref_git_error(message))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
