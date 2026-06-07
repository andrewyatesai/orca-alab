//! Parity dispatch for `orca_config::mcp` vs `src/shared/mcp-config.ts`
//! (`inspectMcpConfigContent` + `summarizeMcpServer`). The TS reference echoes
//! the input `candidate` into its result, so we echo it here too; invalid-JSON
//! cases are intentionally absent from the vectors because V8 and serde produce
//! different parse-error text (only the masked/valid paths are JSON-equal).

use orca_config::{
    inspect_mcp_config_content, McpConfigInspection, McpServerStatus, McpServerSummary,
    McpServerTransport,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "inspectMcpConfigContent" => inspect(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn inspect(input: &Value) -> Value {
    let candidate = input.get("candidate").cloned().unwrap_or(Value::Null);
    let servers_path: Vec<String> = candidate
        .get("serversPath")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|segment| segment.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let servers_path_refs: Vec<&str> = servers_path.iter().map(String::as_str).collect();

    // TS `content: string | null`; JSON null (or absent) maps to the missing path.
    let content = input.get("content").and_then(Value::as_str);
    let inspection = inspect_mcp_config_content(content, &servers_path_refs);
    inspection_to_json(candidate, &inspection)
}

/// Match `JSON.stringify` of the TS `McpConfigInspection` (candidate echoed,
/// `error` omitted when absent).
fn inspection_to_json(candidate: Value, inspection: &McpConfigInspection) -> Value {
    let mut map = Map::new();
    map.insert("candidate".into(), candidate);
    map.insert("exists".into(), Value::Bool(inspection.exists));
    map.insert("status".into(), Value::String(inspection.status.clone()));
    map.insert(
        "servers".into(),
        Value::Array(inspection.servers.iter().map(server_to_json).collect()),
    );
    if let Some(error) = &inspection.error {
        map.insert("error".into(), Value::String(error.clone()));
    }
    Value::Object(map)
}

/// Match `JSON.stringify` of the TS `McpServerSummary`: enums become their TS
/// string ids, `None` optionals are omitted, env becomes an ordered object.
fn server_to_json(server: &McpServerSummary) -> Value {
    let mut map = Map::new();
    map.insert("name".into(), Value::String(server.name.clone()));
    if let Some(transport) = server.transport {
        map.insert("transport".into(), Value::String(transport_id(transport).into()));
    }
    if let Some(status) = server.status {
        map.insert("status".into(), Value::String(status_id(status).into()));
    }
    if let Some(command) = &server.command {
        map.insert("command".into(), Value::String(command.clone()));
    }
    if let Some(url) = &server.url {
        map.insert("url".into(), Value::String(url.clone()));
    }
    if let Some(env) = &server.env {
        let mut env_map = Map::new();
        for (key, value) in env {
            env_map.insert(key.clone(), Value::String(value.clone()));
        }
        map.insert("env".into(), Value::Object(env_map));
    }
    if let Some(issue) = &server.issue {
        map.insert("issue".into(), Value::String(issue.clone()));
    }
    Value::Object(map)
}

fn transport_id(transport: McpServerTransport) -> &'static str {
    match transport {
        McpServerTransport::Stdio => "stdio",
        McpServerTransport::Http => "http",
        McpServerTransport::Unknown => "unknown",
    }
}

fn status_id(status: McpServerStatus) -> &'static str {
    match status {
        McpServerStatus::Enabled => "enabled",
        McpServerStatus::Disabled => "disabled",
        McpServerStatus::Invalid => "invalid",
    }
}
