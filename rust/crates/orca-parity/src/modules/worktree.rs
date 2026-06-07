//! Parity dispatch for `orca_git::worktree` vs `src/main/git/worktree.ts`.
//! Only the pure `parseWorktreeList` parser is covered; the rest of the TS
//! module is git IO/orchestration built on top of it.

use orca_git::worktree::{parse_worktree_list, GitWorktreeInfo};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "parseWorktreeList" => {
            let output = input.get("output").and_then(Value::as_str).unwrap_or("");
            let nul_delimited = input
                .get("nulDelimited")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let worktrees = parse_worktree_list(output, nul_delimited);
            Value::Array(worktrees.iter().map(worktree_to_json).collect())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `GitWorktreeInfo`: `isSparse` is only
/// emitted when true (the TS port spreads it conditionally), so omit the key
/// for non-sparse worktrees rather than serializing `false`.
fn worktree_to_json(info: &GitWorktreeInfo) -> Value {
    let mut map = Map::new();
    map.insert("path".to_string(), Value::String(info.path.clone()));
    map.insert("head".to_string(), Value::String(info.head.clone()));
    map.insert("branch".to_string(), Value::String(info.branch.clone()));
    map.insert("isBare".to_string(), Value::Bool(info.is_bare));
    if info.is_sparse {
        map.insert("isSparse".to_string(), Value::Bool(true));
    }
    map.insert("isMainWorktree".to_string(), Value::Bool(info.is_main_worktree));
    Value::Object(map)
}
