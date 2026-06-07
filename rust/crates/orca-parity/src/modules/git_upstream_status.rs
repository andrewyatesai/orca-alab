//! Parity dispatch for `orca_core::git_upstream_status` vs
//! `src/shared/git-upstream-status.ts`.

use orca_core::git_upstream_status::{
    should_force_push_with_lease_for_upstream, GitUpstreamStatus,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "shouldForcePushWithLeaseForUpstream" => {
            // null/non-object input mirrors the TS `undefined` status that the
            // optional chain short-circuits to false.
            let status = parse_status(input);
            Value::Bool(should_force_push_with_lease_for_upstream(status.as_ref()))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Rebuild the TS `GitUpstreamStatus` record from JSON. Absent fields take the
/// same defaults the TS truthiness checks see for `undefined` (false / 0 / None).
fn parse_status(input: &Value) -> Option<GitUpstreamStatus> {
    let obj = input.as_object()?;
    Some(GitUpstreamStatus {
        has_upstream: obj
            .get("hasUpstream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        upstream_name: obj
            .get("upstreamName")
            .and_then(Value::as_str)
            .map(str::to_string),
        ahead: obj.get("ahead").and_then(Value::as_i64).unwrap_or(0),
        behind: obj.get("behind").and_then(Value::as_i64).unwrap_or(0),
        behind_commits_are_patch_equivalent: obj
            .get("behindCommitsArePatchEquivalent")
            .and_then(Value::as_bool),
    })
}
