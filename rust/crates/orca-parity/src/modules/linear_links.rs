//! Parity dispatch for `orca_core::linear_links` vs
//! `src/shared/linear-links.ts`.

use orca_core::linear_links::{
    build_linear_personal_api_key_settings_url, build_linear_team_url,
    build_linear_workspace_api_settings_url, get_linear_organization_url_key_from_issue_url,
};
use serde_json::{json, Value};

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
