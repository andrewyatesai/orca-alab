//! Parity dispatch for `orca_ssh::parse_ssh_g_output` vs the TS `parseSshGOutput`
//! in `src/main/ssh/ssh-g-config-resolution.ts`.
//!
//! The TS reads `os.homedir()` inside its `~`-expansion; the input pins `home`
//! so the result is reproducible and machine-independent (the Rust port takes
//! `home` as a parameter). Running `ssh -G <host>` itself is the IO edge and
//! stays in TS; only its stdout parsing lives here. `None` optional fields are
//! omitted, matching `JSON.stringify` dropping `undefined`; `port` is always
//! present (`null` for a non-numeric port, mirroring `JSON.stringify(NaN)`).

use orca_ssh::{parse_ssh_g_output, SshResolvedConfig};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "parseSshGOutput" => {
            let stdout = input.get("stdout").and_then(Value::as_str).unwrap_or_default();
            let home = input.get("home").and_then(Value::as_str).unwrap_or_default();
            resolved_config_to_json(&parse_ssh_g_output(stdout, home))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn resolved_config_to_json(config: &SshResolvedConfig) -> Value {
    let mut map = Map::new();
    map.insert("hostname".into(), Value::from(config.hostname.clone()));
    if let Some(v) = &config.user {
        map.insert("user".into(), Value::from(v.clone()));
    }
    // Always present (TS field is `number`); a NaN parseInt serializes to null.
    map.insert("port".into(), config.port.map_or(Value::Null, Value::from));
    map.insert("identityFile".into(), json!(config.identity_file));
    if let Some(v) = &config.identity_agent {
        map.insert("identityAgent".into(), Value::from(v.clone()));
    }
    map.insert("identitiesOnly".into(), Value::from(config.identities_only));
    map.insert("forwardAgent".into(), Value::from(config.forward_agent));
    map.insert("gssapiAuthentication".into(), Value::from(config.gssapi_authentication));
    if let Some(v) = &config.proxy_command {
        map.insert("proxyCommand".into(), Value::from(v.clone()));
    }
    map.insert("proxyUseFdpass".into(), Value::from(config.proxy_use_fdpass));
    if let Some(v) = &config.proxy_jump {
        map.insert("proxyJump".into(), Value::from(v.clone()));
    }
    map.insert("controlMaster".into(), Value::from(config.control_master.clone()));
    if let Some(v) = &config.control_path {
        map.insert("controlPath".into(), Value::from(v.clone()));
    }
    map.insert("controlPersist".into(), Value::from(config.control_persist.clone()));
    Value::Object(map)
}
