//! Parity dispatch for `orca_git::worktree` vs `src/main/git/worktree.ts`.
//! Only the pure `parseWorktreeList` parser is covered; the rest of the TS
//! module is git IO/orchestration built on top of it.

use orca_git::worktree::{parse_worktree_list, worktree_list_to_json};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "parseWorktreeList" => {
            let output = input.get("output").and_then(Value::as_str).unwrap_or("");
            let nul_delimited = input
                .get("nulDelimited")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            // The napi export and this harness share one canonical serializer so
            // the production path and the parity check can't diverge.
            worktree_list_to_json(&parse_worktree_list(output, nul_delimited))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
