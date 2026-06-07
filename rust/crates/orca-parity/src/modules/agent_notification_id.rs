//! Parity dispatch for `orca_core::agent_notification_id` vs
//! `src/shared/agent-notification-id.ts`.

use orca_core::agent_notification_id::{build_agent_notification_id, BuildAgentNotificationIdArgs};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildAgentNotificationId" => {
            let args = BuildAgentNotificationIdArgs {
                // TS treats empty strings and absent fields as falsy; `as_str`
                // yields None for absent/null and the port filters the empties.
                worktree_id: input.get("worktreeId").and_then(Value::as_str),
                pane_key: input.get("paneKey").and_then(Value::as_str),
                // Non-number / non-finite stateStartedAt persists to JSON null;
                // `as_f64` is None there, matching the TS `typeof` guard.
                state_started_at: input.get("stateStartedAt").and_then(Value::as_f64),
            };
            // TS returns `null` (not an omitted key) when the id can't be built.
            match build_agent_notification_id(&args) {
                Some(id) => Value::String(id),
                None => Value::Null,
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
