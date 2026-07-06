//! Parity dispatch for `orca_text::git_remote_error` vs
//! `src/shared/git-remote-error.ts`.

use orca_text::git_remote_error::{
    normalize_git_error_message, strip_credentials_from_message, GitRemoteOperation,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "stripCredentialsFromMessage" => match input.as_str() {
            // TS returns a plain string; mirror it as a JSON string.
            Some(message) => Value::String(strip_credentials_from_message(message)),
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "expected string input" }),
        },
        "normalizeGitErrorMessage" => {
            // A null/absent message models a non-Error throw (TS `undefined`) ->
            // Option::None; a string models the Error path.
            let message = input.get("message").and_then(Value::as_str);
            let operation = match input.get("operation").and_then(Value::as_str) {
                Some("push") => Some(GitRemoteOperation::Push),
                Some("pull") => Some(GitRemoteOperation::Pull),
                Some("fetch") => Some(GitRemoteOperation::Fetch),
                Some("upstream") => Some(GitRemoteOperation::Upstream),
                _ => None,
            };
            Value::String(normalize_git_error_message(message, operation))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
