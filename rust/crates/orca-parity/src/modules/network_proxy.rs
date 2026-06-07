//! Parity dispatch for `orca_net::network_proxy` vs
//! `src/shared/network-proxy.ts`.

use std::collections::BTreeMap;

use orca_net::{
    build_configured_proxy_env, normalize_proxy_bypass_rules, normalize_proxy_url,
    redact_proxy_url, NetworkProxySettings, ProxyUrlValidation,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Non-string inputs map to `None`, matching the TS `typeof value !== 'string'` branch.
        "normalizeProxyUrl" => validation_to_json(&normalize_proxy_url(input.as_str())),
        "normalizeProxyBypassRules" => {
            Value::String(normalize_proxy_bypass_rules(input.as_str()))
        }
        "buildConfiguredProxyEnv" => {
            // A JSON `null` settings object (or absent keys) reads back as `None`,
            // matching the TS `settings?.httpProxyUrl` optional-chaining behavior.
            let settings = NetworkProxySettings {
                http_proxy_url: input.get("httpProxyUrl").and_then(Value::as_str),
                http_proxy_bypass_rules: input.get("httpProxyBypassRules").and_then(Value::as_str),
            };
            env_to_json(build_configured_proxy_env(&settings))
        }
        "redactProxyUrl" => Value::String(redact_proxy_url(input.as_str().unwrap_or(""))),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `ProxyUrlValidationResult`: `message` is only
/// present on the not-ok branch (TS `message?: undefined` is omitted, not null).
fn validation_to_json(result: &ProxyUrlValidation) -> Value {
    let mut map = Map::new();
    map.insert("ok".to_string(), Value::Bool(result.ok));
    map.insert("value".to_string(), Value::String(result.value.clone()));
    if let Some(message) = &result.message {
        map.insert("message".to_string(), Value::String(message.clone()));
    }
    Value::Object(map)
}

/// Match `JSON.stringify` of the TS `Record<string, string>` env map.
fn env_to_json(env: BTreeMap<String, String>) -> Value {
    let mut map = Map::new();
    for (key, value) in env {
        map.insert(key, Value::String(value));
    }
    Value::Object(map)
}
