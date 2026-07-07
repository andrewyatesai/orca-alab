//! Parity dispatch for `orca_config::workspace_session_schema` vs
//! `src/shared/workspace-session-schema.ts` (`parseWorkspaceSession`, the entry
//! `src/main/persistence.ts` uses to validate persisted session blobs).
//!
//! Output mirrors `JSON.stringify` of the TS discriminated union:
//! `{ "ok": true, "value": <session> }` or `{ "ok": false, "error": "path: msg" }`.
//! Why: the Rust port documents two deliberate divergences (unknown keys are
//! preserved, not stripped; error wording differs from zod) — vectors hitting
//! them carry a KNOWN-DEVIATION note and pin the TS output as the golden.

use orca_config::{parse_workspace_session, ParsedWorkspaceSession};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // The vector input IS the raw session JSON value (including null/arrays).
        "parseWorkspaceSession" => match parse_workspace_session(input) {
            ParsedWorkspaceSession::Ok(value) => json!({ "ok": true, "value": value }),
            ParsedWorkspaceSession::Err(error) => json!({ "ok": false, "error": error }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
