//! Parity dispatch for `orca_core::worktree_id` vs `src/shared/worktree-id.ts`.

use orca_core::worktree_id::{
    get_repo_id_from_worktree_id, get_worktree_path_basename_from_id, split_worktree_id,
    split_worktree_id_for_filesystem, ParsedWorktreeId,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Every case carries the single `worktreeId` string argument directly.
    let worktree_id = input.as_str().unwrap_or("");
    match function {
        "getRepoIdFromWorktreeId" => Value::String(get_repo_id_from_worktree_id(worktree_id)),
        "splitWorktreeId" => parsed_to_json(split_worktree_id(worktree_id)),
        "splitWorktreeIdForFilesystem" => {
            parsed_to_json(split_worktree_id_for_filesystem(worktree_id))
        }
        "getWorktreePathBasenameFromId" => match get_worktree_path_basename_from_id(worktree_id) {
            Some(basename) => Value::String(basename),
            None => Value::Null,
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `ParsedWorktreeId | null` return: `null` for
/// None, otherwise `{ repoId, worktreePath }` with the TS field names verbatim.
fn parsed_to_json(parsed: Option<ParsedWorktreeId>) -> Value {
    match parsed {
        Some(p) => json!({ "repoId": p.repo_id, "worktreePath": p.worktree_path }),
        None => Value::Null,
    }
}
