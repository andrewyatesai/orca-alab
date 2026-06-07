//! Parity dispatch for `orca_agents::pull_request_generation` vs
//! `src/shared/pull-request-generation.ts`.

use orca_agents::{build_pull_request_fields_prompt, PullRequestDraftContext};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildPullRequestFieldsPrompt" => {
            let context = parse_context(input.get("context").unwrap_or(&Value::Null));
            let custom_prompt = input.get("customPrompt").and_then(Value::as_str).unwrap_or("");
            // TS returns a plain string; `JSON.stringify` of it is a JSON string.
            Value::String(build_pull_request_fields_prompt(&context, custom_prompt))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Build a `PullRequestDraftContext` from the vector's JSON object, mirroring the
/// TS field names: string fields default to "" when absent, `branch` is nullable
/// (`null` -> `None`).
fn parse_context(value: &Value) -> PullRequestDraftContext {
    let str_field = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let bool_field = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    PullRequestDraftContext {
        branch: value.get("branch").and_then(Value::as_str).map(str::to_string),
        base: str_field("base"),
        branch_changed_by_preparation: bool_field("branchChangedByPreparation"),
        current_title: str_field("currentTitle"),
        current_body: str_field("currentBody"),
        current_draft: bool_field("currentDraft"),
        commit_summary: str_field("commitSummary"),
        change_summary: str_field("changeSummary"),
        patch: str_field("patch"),
    }
}
