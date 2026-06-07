//! Parity dispatch for `orca_config::workspace_session_terminal_buffers` vs
//! `src/shared/workspace-session-terminal-buffers.ts`.

use orca_config::workspace_session_terminal_buffers::{
    prune_local_terminal_scrollback_buffers, should_preserve_terminal_scrollback_buffers,
    RepoConnection,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "shouldPreserveTerminalScrollbackBuffers" => {
            // A JSON `null`/absent worktreeId maps to `None` (TS `undefined`).
            let worktree_id = input.get("worktreeId").and_then(Value::as_str);
            let repos = parse_repos(input.get("repos"));
            Value::Bool(should_preserve_terminal_scrollback_buffers(worktree_id, &repos))
        }
        "pruneLocalTerminalScrollbackBuffers" => {
            let repos = parse_repos(input.get("repos"));
            let session = input.get("session").cloned().unwrap_or(Value::Null);
            prune_local_terminal_scrollback_buffers(&session, &repos)
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// `connectionId: null` (or absent) → `None`, matching the TS `string | null`.
fn parse_repos(value: Option<&Value>) -> Vec<RepoConnection> {
    value
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_object)
                .map(|object| RepoConnection {
                    id: object.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                    connection_id: object.get("connectionId").and_then(|value| {
                        if value.is_null() {
                            None
                        } else {
                            value.as_str().map(str::to_string)
                        }
                    }),
                })
                .collect()
        })
        .unwrap_or_default()
}
