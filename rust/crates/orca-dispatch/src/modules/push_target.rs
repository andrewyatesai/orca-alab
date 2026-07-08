//! Parity dispatch for `orca_git::push_target` vs
//! `src/shared/git-publish-target-status.ts`.
//!
//! Only the pure target-naming helpers are covered; `getPublishTargetStatus`
//! (TS) / `validate_git_push_target` (Rust) are io-injected and out of scope.

use orca_git::push_target::{
    publish_target_display_name, publish_target_remote_ref, GitPushTarget,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "getPublishTargetDisplayName" => {
            Value::String(publish_target_display_name(&target_from_json(input)))
        }
        "getPublishTargetRemoteRef" => {
            Value::String(publish_target_remote_ref(&target_from_json(input)))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Build a `GitPushTarget` from the TS `GitPushTarget` JSON shape. The optional
/// `remoteUrl` maps to `None` when absent (mirrors TS `remoteUrl?`); the Rust
/// `remote_created` field has no analogue in these pure helpers and is unused.
fn target_from_json(input: &Value) -> GitPushTarget {
    GitPushTarget {
        remote_name: input
            .get("remoteName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        branch_name: input
            .get("branchName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        remote_url: input
            .get("remoteUrl")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}
