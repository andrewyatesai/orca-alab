//! Parity dispatch for `orca_agents::commit_message_plan` vs
//! `src/shared/commit-message-plan.ts`.
//!
//! `plan_commit_message_generation` is a pure transform from "agent choice +
//! prompt" to a spawn plan, returning `Result<CommitMessagePlan, String>`. We
//! shape that into the TS `CommitMessagePlanResult` discriminated union:
//! `{ ok: true, plan: {...} }` on success and `{ ok: false, error }` on failure.

use orca_agents::{plan_commit_message_generation, CommitMessagePlan, CommitMessagePlanInput};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "planCommitMessageGeneration" => {
            // Vectors carry both args under one object; mirror the TS destructure
            // of `{ planInput, prompt }`.
            let plan_input = input.get("planInput");
            let agent_id = plan_input.and_then(|v| v.get("agentId")).and_then(Value::as_str).unwrap_or_default();
            let model = plan_input.and_then(|v| v.get("model")).and_then(Value::as_str).unwrap_or_default();
            let thinking_level = plan_input.and_then(|v| v.get("thinkingLevel")).and_then(Value::as_str);
            let custom_agent_command = plan_input.and_then(|v| v.get("customAgentCommand")).and_then(Value::as_str);
            let agent_command_override =
                plan_input.and_then(|v| v.get("agentCommandOverride")).and_then(Value::as_str);
            let prompt = input.get("prompt").and_then(Value::as_str).unwrap_or_default();

            let parsed = CommitMessagePlanInput {
                agent_id,
                model,
                thinking_level,
                custom_agent_command,
                agent_command_override,
            };
            match plan_commit_message_generation(&parsed, prompt) {
                Ok(plan) => json!({ "ok": true, "plan": plan_to_json(plan) }),
                Err(error) => json!({ "ok": false, "error": error }),
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `CommitMessagePlan`.
fn plan_to_json(plan: CommitMessagePlan) -> Value {
    json!({
        "binary": plan.binary,
        "args": plan.args,
        // TS always emits `stdinPayload` as an explicit `string | null` (never
        // absent), so map None -> null instead of omitting the key — the one
        // optional in this module that mirrors null rather than omission.
        "stdinPayload": plan.stdin_payload,
        "label": plan.label,
    })
}
