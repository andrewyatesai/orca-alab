//! Parity dispatch for `orca_core::protocol_compat` vs
//! `src/shared/protocol-compat.ts`.

use orca_core::protocol_compat::{
    describe_runtime_compat_block, evaluate_compat, evaluate_runtime_compat, CompatBlockReason,
    CompatVerdict, RuntimeBlockReason, RuntimeCompatVerdict,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "evaluateRuntimeCompat" => runtime_verdict_to_json(evaluate_runtime_compat(
            opt_i64(input, "clientProtocolVersion").unwrap_or(0),
            opt_i64(input, "minCompatibleServerProtocolVersion").unwrap_or(0),
            opt_i64(input, "serverProtocolVersion"),
            opt_i64(input, "serverMinCompatibleClientProtocolVersion"),
        )),
        "describeRuntimeCompatBlock" => {
            Value::from(describe_runtime_compat_block(&parse_runtime_verdict(input)))
        }
        "evaluateCompat" => compat_verdict_to_json(evaluate_compat(
            opt_i64(input, "mobileProtocolVersion").unwrap_or(0),
            opt_i64(input, "minCompatibleDesktopVersion").unwrap_or(0),
            opt_i64(input, "desktopProtocolVersion"),
            opt_i64(input, "desktopMinCompatibleMobileVersion"),
        )),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// `undefined`/missing/null all map to `None`, mirroring TS `?? 0` coercion.
fn opt_i64(input: &Value, key: &str) -> Option<i64> {
    input.get(key).and_then(Value::as_i64)
}

/// Match `JSON.stringify` of the TS `RuntimeCompatVerdict` (absent optionals omitted).
fn runtime_verdict_to_json(verdict: RuntimeCompatVerdict) -> Value {
    match verdict {
        RuntimeCompatVerdict::Ok {
            client_protocol_version,
            server_protocol_version,
        } => json!({
            "kind": "ok",
            "clientProtocolVersion": client_protocol_version,
            "serverProtocolVersion": server_protocol_version,
        }),
        RuntimeCompatVerdict::Blocked {
            reason,
            client_protocol_version,
            server_protocol_version,
            required_client_protocol_version,
            required_server_protocol_version,
        } => {
            let mut map = Map::new();
            map.insert("kind".to_string(), Value::from("blocked"));
            map.insert(
                "reason".to_string(),
                Value::from(match reason {
                    RuntimeBlockReason::ClientTooOld => "client-too-old",
                    RuntimeBlockReason::ServerTooOld => "server-too-old",
                }),
            );
            map.insert(
                "clientProtocolVersion".to_string(),
                Value::from(client_protocol_version),
            );
            map.insert(
                "serverProtocolVersion".to_string(),
                Value::from(server_protocol_version),
            );
            if let Some(v) = required_client_protocol_version {
                map.insert("requiredClientProtocolVersion".to_string(), Value::from(v));
            }
            if let Some(v) = required_server_protocol_version {
                map.insert("requiredServerProtocolVersion".to_string(), Value::from(v));
            }
            Value::Object(map)
        }
    }
}

/// Match `JSON.stringify` of the TS `CompatVerdict` (absent optionals omitted).
fn compat_verdict_to_json(verdict: CompatVerdict) -> Value {
    match verdict {
        CompatVerdict::Ok => json!({ "kind": "ok" }),
        CompatVerdict::Blocked {
            reason,
            desktop_version,
            required_mobile_version,
            required_desktop_version,
        } => {
            let mut map = Map::new();
            map.insert("kind".to_string(), Value::from("blocked"));
            map.insert(
                "reason".to_string(),
                Value::from(match reason {
                    CompatBlockReason::MobileTooOld => "mobile-too-old",
                    CompatBlockReason::DesktopTooOld => "desktop-too-old",
                }),
            );
            map.insert("desktopVersion".to_string(), Value::from(desktop_version));
            if let Some(v) = required_mobile_version {
                map.insert("requiredMobileVersion".to_string(), Value::from(v));
            }
            if let Some(v) = required_desktop_version {
                map.insert("requiredDesktopVersion".to_string(), Value::from(v));
            }
            Value::Object(map)
        }
    }
}

/// Reconstruct the verdict from its `JSON.stringify` shape so the describe text
/// is computed over the same input the TS function receives.
fn parse_runtime_verdict(input: &Value) -> RuntimeCompatVerdict {
    if input.get("kind").and_then(Value::as_str) == Some("ok") {
        return RuntimeCompatVerdict::Ok {
            client_protocol_version: opt_i64(input, "clientProtocolVersion").unwrap_or(0),
            server_protocol_version: opt_i64(input, "serverProtocolVersion").unwrap_or(0),
        };
    }
    let reason = match input.get("reason").and_then(Value::as_str) {
        Some("client-too-old") => RuntimeBlockReason::ClientTooOld,
        _ => RuntimeBlockReason::ServerTooOld,
    };
    RuntimeCompatVerdict::Blocked {
        reason,
        client_protocol_version: opt_i64(input, "clientProtocolVersion").unwrap_or(0),
        server_protocol_version: opt_i64(input, "serverProtocolVersion").unwrap_or(0),
        required_client_protocol_version: opt_i64(input, "requiredClientProtocolVersion"),
        required_server_protocol_version: opt_i64(input, "requiredServerProtocolVersion"),
    }
}
