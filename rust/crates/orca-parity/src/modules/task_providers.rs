//! Parity dispatch for `orca_core::task_providers` vs
//! `src/shared/task-providers.ts`.

use orca_core::task_providers::{
    normalize_task_provider_settings, normalize_visible_task_providers,
    resolve_visible_task_provider, restore_available_default_task_provider, TaskProvider,
    TaskProviderAvailability,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isTaskProvider" => {
            Value::Bool(input.as_str().and_then(TaskProvider::from_id).is_some())
        }
        "normalizeVisibleTaskProviders" => {
            let list = str_list(Some(input));
            providers_to_json(&normalize_visible_task_providers(list.as_deref()))
        }
        "normalizeTaskProviderSettings" => {
            let visible = str_list(input.get("visibleTaskProviders"));
            let default = input.get("defaultTaskSource").and_then(Value::as_str);
            let result = normalize_task_provider_settings(visible.as_deref(), default);
            // Match `JSON.stringify` of the TS settings record (id strings).
            json!({
                "visibleTaskProviders": providers_to_json(&result.visible_task_providers),
                "defaultTaskSource": result.default_task_source.as_id(),
            })
        }
        "restoreAvailableDefaultTaskProvider" => {
            let visible = providers_from_json(input.get("visibleProviders"));
            let availability = availability_from_json(input.get("availability"));
            let preferred = input.get("preferredProvider").and_then(Value::as_str);
            providers_to_json(&restore_available_default_task_provider(
                &visible,
                &availability,
                preferred,
            ))
        }
        "resolveVisibleTaskProvider" => {
            let preferred = input
                .get("preferred")
                .and_then(Value::as_str)
                .and_then(TaskProvider::from_id);
            let visible = providers_from_json(input.get("visibleProviders"));
            Value::String(resolve_visible_task_provider(preferred, &visible).as_id().to_string())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// `Option<&[&str]>` for the normalize-family args: `Some` for a JSON array
/// (non-string elements dropped, mirroring the TS `Set.has` filter), `None`
/// for any non-array so the port takes its all-providers fallback.
fn str_list(value: Option<&Value>) -> Option<Vec<&str>> {
    value
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
}

/// Decode a TS `TaskProvider[]` arg into typed providers, dropping unknown ids.
fn providers_from_json(value: Option<&Value>) -> Vec<TaskProvider> {
    value
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).filter_map(TaskProvider::from_id).collect())
        .unwrap_or_default()
}

fn availability_from_json(value: Option<&Value>) -> TaskProviderAvailability {
    let field = |name: &str| {
        value.and_then(|v| v.get(name)).and_then(Value::as_bool).unwrap_or(false)
    };
    TaskProviderAvailability {
        gitlab_installed: field("gitlabInstalled"),
        linear_connected: field("linearConnected"),
    }
}

/// Serialize providers to the TS array-of-id-strings image.
fn providers_to_json(providers: &[TaskProvider]) -> Value {
    Value::Array(providers.iter().map(|p| Value::String(p.as_id().to_string())).collect())
}
