//! Parity dispatch for `orca_ssh::parse_ssh_config` vs the TS `parseSshConfig`
//! in `src/main/ssh/ssh-config-parser.ts`.
//!
//! The TS reads `os.homedir()` inside its `~`-expansion; the vector pins `home`
//! so the goldens are reproducible and machine-independent (the Rust port takes
//! `home` as a parameter). `None` optional fields are omitted, matching
//! `JSON.stringify` dropping `undefined`.

use orca_ssh::{parse_ssh_config, SshConfigHost};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "parseSshConfig" => {
            let content = input.get("content").and_then(Value::as_str).unwrap_or_default();
            let home = input.get("home").and_then(Value::as_str).unwrap_or_default();
            Value::Array(parse_ssh_config(content, home).iter().map(host_to_json).collect())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn host_to_json(host: &SshConfigHost) -> Value {
    let mut map = Map::new();
    map.insert("host".into(), Value::from(host.host.clone()));
    if let Some(v) = &host.hostname {
        map.insert("hostname".into(), Value::from(v.clone()));
    }
    if let Some(v) = host.port {
        map.insert("port".into(), Value::from(v));
    }
    if let Some(v) = &host.user {
        map.insert("user".into(), Value::from(v.clone()));
    }
    if let Some(v) = &host.identity_file {
        map.insert("identityFile".into(), Value::from(v.clone()));
    }
    if let Some(v) = &host.identity_agent {
        map.insert("identityAgent".into(), Value::from(v.clone()));
    }
    if let Some(v) = host.identities_only {
        map.insert("identitiesOnly".into(), Value::from(v));
    }
    if let Some(v) = &host.proxy_command {
        map.insert("proxyCommand".into(), Value::from(v.clone()));
    }
    if let Some(v) = host.proxy_use_fdpass {
        map.insert("proxyUseFdpass".into(), Value::from(v));
    }
    if let Some(v) = &host.proxy_jump {
        map.insert("proxyJump".into(), Value::from(v.clone()));
    }
    Value::Object(map)
}
