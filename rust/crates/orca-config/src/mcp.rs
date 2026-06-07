//! MCP server config inspection, ported from `src/shared/mcp-config.ts`
//! (`inspectMcpConfigContent` + `summarizeMcpServer`). Parses a config's JSON,
//! extracts the servers object at the candidate's path, and summarizes each
//! server's transport/status, masking sensitive env via `orca-text`.

use orca_text::mcp_env::mask_mcp_env;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpServerTransport {
    Stdio,
    Http,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpServerStatus {
    Enabled,
    Disabled,
    Invalid,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpServerSummary {
    pub name: String,
    pub transport: Option<McpServerTransport>,
    pub status: Option<McpServerStatus>,
    pub command: Option<String>,
    pub url: Option<String>,
    pub env: Option<Vec<(String, String)>>,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpConfigInspection {
    pub exists: bool,
    /// "missing" | "valid" | "invalid"
    pub status: String,
    pub servers: Vec<McpServerSummary>,
    pub error: Option<String>,
}

/// Inspect a config file's content (`None` = file absent). `servers_path` is the
/// candidate's path to the servers object (e.g. `["mcpServers"]`).
pub fn inspect_mcp_config_content(content: Option<&str>, servers_path: &[&str]) -> McpConfigInspection {
    let Some(content) = content else {
        return McpConfigInspection { exists: false, status: "missing".into(), servers: Vec::new(), error: None };
    };
    let parsed: Value = match serde_json::from_str(content) {
        Ok(value) => value,
        // Don't expose file contents; just note it failed to parse.
        Err(error) => {
            return McpConfigInspection {
                exists: true,
                status: "invalid".into(),
                servers: Vec::new(),
                error: Some(format!("Invalid JSON: {error}")),
            };
        }
    };

    let Some(servers) = extract_object_at_path(&parsed, servers_path) else {
        return McpConfigInspection { exists: true, status: "valid".into(), servers: Vec::new(), error: None };
    };

    let servers = servers
        .iter()
        .map(|(name, entry)| summarize_mcp_server(name, entry))
        .collect();
    McpConfigInspection { exists: true, status: "valid".into(), servers, error: None }
}

fn extract_object_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a serde_json::Map<String, Value>> {
    let mut current = value;
    for segment in path {
        current = current.as_object()?.get(*segment)?;
    }
    current.as_object()
}

fn summarize_mcp_server(name: &str, entry: &Value) -> McpServerSummary {
    let Some(raw) = entry.as_object() else {
        return McpServerSummary {
            name: name.to_string(),
            transport: Some(McpServerTransport::Unknown),
            status: Some(McpServerStatus::Invalid),
            issue: Some("Server entry must be an object.".into()),
            ..Default::default()
        };
    };

    let command = read_command(raw);
    let url = read_url(raw);
    let transport = resolve_transport(raw, &command, &url);
    let enabled = raw.get("enabled") != Some(&Value::Bool(false))
        && raw.get("disabled") != Some(&Value::Bool(true));
    let env = mask_env(raw.get("env"));

    let invalid = |issue: &str, env: Option<Vec<(String, String)>>| McpServerSummary {
        name: name.to_string(),
        transport: Some(transport),
        status: Some(McpServerStatus::Invalid),
        env,
        issue: Some(issue.to_string()),
        ..Default::default()
    };

    match transport {
        McpServerTransport::Unknown => invalid("Missing command or URL.", env),
        McpServerTransport::Http if url.is_none() => invalid("Missing URL.", env),
        McpServerTransport::Stdio if command.is_none() => invalid("Missing command.", env),
        _ => McpServerSummary {
            name: name.to_string(),
            transport: Some(transport),
            status: Some(if enabled { McpServerStatus::Enabled } else { McpServerStatus::Disabled }),
            command,
            url,
            env,
            issue: None,
        },
    }
}

fn read_command(raw: &serde_json::Map<String, Value>) -> Option<String> {
    match raw.get("command") {
        Some(Value::String(command)) => Some(command.clone()),
        Some(Value::Array(items)) => items.first().and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn read_url(raw: &serde_json::Map<String, Value>) -> Option<String> {
    raw.get("url")
        .and_then(Value::as_str)
        .or_else(|| raw.get("httpUrl").and_then(Value::as_str))
        .map(str::to_string)
}

fn resolve_transport(
    raw: &serde_json::Map<String, Value>,
    command: &Option<String>,
    url: &Option<String>,
) -> McpServerTransport {
    let type_field = raw.get("type").and_then(Value::as_str);
    if matches!(type_field, Some("http") | Some("remote")) || url.is_some() {
        McpServerTransport::Http
    } else if type_field == Some("local") || command.is_some() {
        McpServerTransport::Stdio
    } else {
        McpServerTransport::Unknown
    }
}

/// Convert a JSON `env` object to masked string pairs (preserving order).
fn mask_env(env: Option<&Value>) -> Option<Vec<(String, String)>> {
    let object = env?.as_object()?;
    let pairs: Vec<(String, String)> = object
        .iter()
        .map(|(key, value)| {
            let text = value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string());
            (key.clone(), text)
        })
        .collect();
    let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    mask_mcp_env(Some(&refs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        name: &str,
        transport: McpServerTransport,
        status: McpServerStatus,
        command: Option<&str>,
        url: Option<&str>,
        env: Option<Vec<(&str, &str)>>,
        issue: Option<&str>,
    ) -> McpServerSummary {
        McpServerSummary {
            name: name.to_string(),
            transport: Some(transport),
            status: Some(status),
            command: command.map(str::to_string),
            url: url.map(str::to_string),
            env: env.map(|e| e.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
            issue: issue.map(str::to_string),
        }
    }

    #[test]
    fn reports_missing_config() {
        let result = inspect_mcp_config_content(None, &["mcpServers"]);
        assert!(!result.exists);
        assert_eq!(result.status, "missing");
        assert!(result.servers.is_empty());
    }

    #[test]
    fn reports_invalid_json_without_exposing_contents() {
        let result = inspect_mcp_config_content(Some("{"), &["mcpServers"]);
        assert_eq!(result.status, "invalid");
        assert!(result.error.as_deref().unwrap().contains("JSON"));
        assert!(result.servers.is_empty());
    }

    #[test]
    fn summarizes_stdio_http_disabled_and_invalid_servers() {
        let content = r#"{
            "mcpServers": {
                "filesystem": { "command": "npx", "args": ["-y", "x"], "env": { "NODE_ENV": "production", "API_TOKEN": "secret-token" } },
                "docs": { "type": "http", "url": "https://example.com/mcp" },
                "old": { "command": "node", "enabled": false },
                "broken": { "args": ["missing-command"] }
            }
        }"#;
        let result = inspect_mcp_config_content(Some(content), &["mcpServers"]);
        assert_eq!(result.status, "valid");
        assert_eq!(
            result.servers,
            vec![
                summary(
                    "filesystem",
                    McpServerTransport::Stdio,
                    McpServerStatus::Enabled,
                    Some("npx"),
                    None,
                    Some(vec![("NODE_ENV", "production"), ("API_TOKEN", "••••••••")]),
                    None,
                ),
                summary("docs", McpServerTransport::Http, McpServerStatus::Enabled, None, Some("https://example.com/mcp"), None, None),
                summary("old", McpServerTransport::Stdio, McpServerStatus::Disabled, Some("node"), None, None, None),
                summary("broken", McpServerTransport::Unknown, McpServerStatus::Invalid, None, None, None, Some("Missing command or URL.")),
            ]
        );
    }

    #[test]
    fn http_without_url_is_invalid() {
        let content = r#"{"mcpServers": {"x": {"type": "http"}}}"#;
        let result = inspect_mcp_config_content(Some(content), &["mcpServers"]);
        assert_eq!(result.servers[0].status, Some(McpServerStatus::Invalid));
        assert_eq!(result.servers[0].issue.as_deref(), Some("Missing URL."));
    }

    #[test]
    fn missing_servers_object_is_valid_empty() {
        let result = inspect_mcp_config_content(Some(r#"{"other": 1}"#), &["mcpServers"]);
        assert_eq!(result.status, "valid");
        assert!(result.servers.is_empty());
    }
}
