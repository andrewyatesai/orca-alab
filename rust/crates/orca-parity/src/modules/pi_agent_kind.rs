//! Parity dispatch for `orca_text::pi_agent_kind` vs
//! `src/shared/pi-agent-kind.ts`.

use orca_text::pi_agent_kind::{detect_pi_agent_kind_from_command, PiAgentKind};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // JSON null (TS `undefined`) and any non-string both yield `as_str() ==
        // None`, matching the TS bare-shell default branch.
        "detectPiAgentKindFromCommand" => {
            Value::String(kind_to_id(detect_pi_agent_kind_from_command(input.as_str())).to_string())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Serialize the enum to its TS string id (`PiAgentKind` = `'pi' | 'omp'`).
fn kind_to_id(kind: PiAgentKind) -> &'static str {
    match kind {
        PiAgentKind::Pi => "pi",
        PiAgentKind::Omp => "omp",
    }
}
