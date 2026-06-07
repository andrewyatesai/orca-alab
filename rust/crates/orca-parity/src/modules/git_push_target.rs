//! Parity dispatch for `orca_core::git_push_target` vs
//! `src/shared/git-push-target-validation.ts`.
//!
//! The TS `assertGitPushTargetShape` throws on invalid input and returns void on
//! success; the Rust port returns `Result<(), String>` with the same messages.
//! Both are shaped into `{ ok, error? }` so the JSON images are equal.

use orca_core::git_push_target::validate_git_push_target;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "assertGitPushTargetShape" => {
            // Vectors carry valid string fields; type-mismatch cases are out of
            // scope because the Rust port validates already-typed inputs.
            let remote_name = input.get("remoteName").and_then(Value::as_str).unwrap_or("");
            let branch_name = input.get("branchName").and_then(Value::as_str).unwrap_or("");
            // Absent remoteUrl -> None, mirroring the omitted-key TS branch.
            let remote_url = input.get("remoteUrl").and_then(Value::as_str);
            match validate_git_push_target(remote_name, branch_name, remote_url) {
                Ok(()) => json!({ "ok": true }),
                Err(message) => json!({ "ok": false, "error": message }),
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
