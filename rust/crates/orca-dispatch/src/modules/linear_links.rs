//! Parity dispatch for `orca_core::linear_links` vs
//! `src/shared/linear-links.ts`.

use orca_core::linear_links::{
    build_linear_personal_api_key_settings_url, build_linear_team_url,
    build_linear_workspace_api_settings_url, get_linear_organization_url_key_from_issue_url,
    parse_linear_issue_input,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Object input: { organizationUrlKey?, teamKey? }. Returns `string | null`.
        "buildLinearTeamUrl" => {
            let organization_url_key = input.get("organizationUrlKey").and_then(Value::as_str);
            let team_key = input.get("teamKey").and_then(Value::as_str);
            optional_string(build_linear_team_url(organization_url_key, team_key))
        }
        // Single-arg functions: input is the raw `organizationUrlKey` / issue URL
        // value (string or null/absent, matching the optional TS arg).
        "buildLinearPersonalApiKeySettingsUrl" => {
            Value::String(build_linear_personal_api_key_settings_url(input.as_str()))
        }
        "buildLinearWorkspaceApiSettingsUrl" => {
            Value::String(build_linear_workspace_api_settings_url(input.as_str()))
        }
        "getLinearOrganizationUrlKeyFromIssueUrl" => {
            optional_string(get_linear_organization_url_key_from_issue_url(input.as_str()))
        }
        // Input is the raw string arg. Returns `{ identifier, organizationUrlKey? }`
        // (org key omitted when absent) or `null`, matching `JSON.stringify`.
        "parseLinearIssueInput" => match parse_linear_issue_input(input.as_str().unwrap_or("")) {
            Some(parsed) => {
                let mut obj = Map::new();
                obj.insert("identifier".to_string(), Value::String(parsed.identifier));
                if let Some(key) = parsed.organization_url_key {
                    obj.insert("organizationUrlKey".to_string(), Value::String(key));
                }
                Value::Object(obj)
            }
            None => Value::Null,
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of a TS `string | null` return: `Some` to the string,
/// `None` to `null` (not an omitted key).
fn optional_string(value: Option<String>) -> Value {
    match value {
        Some(s) => Value::String(s),
        None => Value::Null,
    }
}
