//! Parity dispatch for `orca_git::effective_upstream` vs
//! `src/shared/git-effective-upstream.ts`.

use orca_git::effective_upstream::split_remote_branch_name;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // TS `splitRemoteBranchName` returns `{ remoteName, branchName } | null`.
        "splitRemoteBranchName" => match input.as_str() {
            Some(ref_name) => match split_remote_branch_name(ref_name) {
                Some((remote_name, branch_name)) => json!({
                    "remoteName": remote_name,
                    "branchName": branch_name,
                }),
                None => Value::Null,
            },
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "expected string input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
