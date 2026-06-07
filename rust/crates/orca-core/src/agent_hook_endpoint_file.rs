//! Agent-hook endpoint file parsing, ported from `src/shared/agent-hook-endpoint-file.ts`.
//!
//! The hook handshake writes `endpoint.env` (POSIX `KEY=value`) or `endpoint.cmd`
//! (Windows `set KEY=value`); this parses either into the connection fields.
//! Pure: the `set ` prefix and whitespace handling are hand-rolled (no regex).

use std::collections::HashMap;

pub const AGENT_HOOK_ENDPOINT_FILE_NAMES: [&str; 2] = ["endpoint.env", "endpoint.cmd"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentHookEndpoint {
    pub port: String,
    pub token: String,
    pub env: String,
    pub version: String,
}

pub fn is_agent_hook_endpoint_file_name(name: &str) -> bool {
    AGENT_HOOK_ENDPOINT_FILE_NAMES.contains(&name)
}

/// Strip a leading case-insensitive `set` + whitespace (the Windows `.cmd`
/// form); lines without that prefix are returned unchanged.
fn strip_set_prefix(line: &str) -> &str {
    if line.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("set")) {
        let rest = &line[3..];
        let trimmed = rest.trim_start();
        // Only strip when `set` was actually followed by whitespace.
        if trimmed.len() != rest.len() {
            return trimmed;
        }
    }
    line
}

pub fn parse_agent_hook_endpoint_file(contents: &str) -> Result<AgentHookEndpoint, String> {
    let mut values: HashMap<String, String> = HashMap::new();
    for raw_line in contents.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        let normalized = strip_set_prefix(line);
        // key = before the first `=`, value = the rest (equals signs preserved).
        let (key, value) = normalized.split_once('=').unwrap_or((normalized, ""));
        values.insert(key.to_string(), value.to_string());
    }

    let field = |key: &str| values.get(key).filter(|value| !value.is_empty());
    match (
        field("ORCA_AGENT_HOOK_PORT"),
        field("ORCA_AGENT_HOOK_TOKEN"),
        field("ORCA_AGENT_HOOK_ENV"),
        field("ORCA_AGENT_HOOK_VERSION"),
    ) {
        (Some(port), Some(token), Some(env), Some(version)) => Ok(AgentHookEndpoint {
            port: port.clone(),
            token: token.clone(),
            env: env.clone(),
            version: version.clone(),
        }),
        _ => Err("Agent hook endpoint file is missing required fields".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_posix_and_windows_endpoint_file_names() {
        assert!(is_agent_hook_endpoint_file_name("endpoint.env"));
        assert!(is_agent_hook_endpoint_file_name("endpoint.cmd"));
        assert!(!is_agent_hook_endpoint_file_name("endpoint.ps1"));
    }

    #[test]
    fn parses_posix_endpoint_env_contents() {
        let contents = [
            "ORCA_AGENT_HOOK_PORT=12345",
            "ORCA_AGENT_HOOK_TOKEN=token-123",
            "ORCA_AGENT_HOOK_ENV=production",
            "ORCA_AGENT_HOOK_VERSION=1",
        ]
        .join("\n");
        assert_eq!(
            parse_agent_hook_endpoint_file(&contents),
            Ok(AgentHookEndpoint {
                port: "12345".to_string(),
                token: "token-123".to_string(),
                env: "production".to_string(),
                version: "1".to_string(),
            })
        );
    }

    #[test]
    fn parses_windows_endpoint_cmd_contents() {
        let contents = [
            "set ORCA_AGENT_HOOK_PORT=54321",
            "set ORCA_AGENT_HOOK_TOKEN=token-abc",
            "set ORCA_AGENT_HOOK_ENV=development",
            "set ORCA_AGENT_HOOK_VERSION=1",
        ]
        .join("\r\n");
        assert_eq!(
            parse_agent_hook_endpoint_file(&contents),
            Ok(AgentHookEndpoint {
                port: "54321".to_string(),
                token: "token-abc".to_string(),
                env: "development".to_string(),
                version: "1".to_string(),
            })
        );
    }

    #[test]
    fn preserves_equals_signs_in_endpoint_values() {
        let contents = [
            "ORCA_AGENT_HOOK_PORT=12345",
            "ORCA_AGENT_HOOK_TOKEN=token=with=equals",
            "ORCA_AGENT_HOOK_ENV=production",
            "ORCA_AGENT_HOOK_VERSION=1",
        ]
        .join("\n");
        assert_eq!(parse_agent_hook_endpoint_file(&contents).unwrap().token, "token=with=equals");
    }

    #[test]
    fn errors_when_required_endpoint_fields_are_missing() {
        assert_eq!(
            parse_agent_hook_endpoint_file("ORCA_AGENT_HOOK_PORT=12345"),
            Err("Agent hook endpoint file is missing required fields".to_string())
        );
    }
}
