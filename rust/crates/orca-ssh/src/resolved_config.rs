//! `ssh -G` resolved-config parsing, ported from `parseSshGOutput` in
//! `src/main/ssh/ssh-g-config-resolution.ts`. Pure: `~` expansion is
//! parameterized on `home` (the TS reads `os.homedir()`). The IO — running
//! `ssh -G <host>` — stays in TS; only the stdout parsing lives here.

use crate::config_parser::{js_parse_int_base10, resolve_ssh_config_home_path};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SshResolvedConfig {
    pub hostname: String,
    pub user: Option<String>,
    // `Number.parseInt(port ?? "22", 10)`: `None` models the NaN a non-numeric
    // port would yield (JSON.stringify(NaN) === null). i64 mirrors parseInt's
    // unbounded JS number, matching the config parser's port field.
    pub port: Option<i64>,
    pub identity_file: Vec<String>,
    pub identity_agent: Option<String>,
    pub identities_only: bool,
    pub forward_agent: bool,
    pub gssapi_authentication: bool,
    pub proxy_command: Option<String>,
    pub proxy_use_fdpass: bool,
    pub proxy_jump: Option<String>,
    pub control_master: String,
    pub control_path: Option<String>,
    pub control_persist: String,
}

/// Parse `ssh -G <host>` stdout (one `key value` per line, first space splits,
/// keys lowercased). `identityfile` accumulates (ssh emits it once per key);
/// every other key is last-write-wins. `~` paths in identity/agent/control
/// values expand against `home`.
pub fn parse_ssh_g_output(stdout: &str, home: &str) -> SshResolvedConfig {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut identity_files: Vec<String> = Vec::new();

    for line in stdout.split('\n') {
        let Some(space_idx) = line.find(' ') else {
            continue;
        };
        let key = line[..space_idx].to_lowercase();
        // ssh -G values are ASCII (hosts/users/paths/numbers), so str::trim
        // matches JS `.trim()` for every reachable value.
        let value = line[space_idx + 1..].trim();
        if key == "identityfile" {
            identity_files.push(resolve_ssh_config_home_path(value, home));
        } else {
            map.insert(key, value.to_string());
        }
    }

    build_ssh_resolved_config(&map, identity_files, home)
}

fn build_ssh_resolved_config(
    map: &HashMap<String, String>,
    identity_files: Vec<String>,
    home: &str,
) -> SshResolvedConfig {
    // `ssh -G` prints `proxycommand none` / `proxyjump none` / `controlpath none`
    // when unset; the TS `raw && raw !== 'none'` guard drops both the empty and
    // the literal "none" (treating "none" as real would spawn bad commands).
    let non_none = |key: &str| {
        map.get(key).filter(|value| !value.is_empty() && value.as_str() != "none").cloned()
    };

    SshResolvedConfig {
        hostname: map.get("hostname").cloned().unwrap_or_default(),
        // `map.get('user') || undefined` — empty string is falsy → None.
        user: map.get("user").filter(|value| !value.is_empty()).cloned(),
        port: js_parse_int_base10(map.get("port").map(String::as_str).unwrap_or("22")),
        identity_file: identity_files,
        // `rawIdentityAgent ? resolve(rawIdentityAgent) : undefined`.
        identity_agent: map
            .get("identityagent")
            .filter(|value| !value.is_empty())
            .map(|value| resolve_ssh_config_home_path(value, home)),
        identities_only: map.get("identitiesonly").map(String::as_str) == Some("yes"),
        forward_agent: map.get("forwardagent").map(String::as_str) == Some("yes"),
        gssapi_authentication: map.get("gssapiauthentication").map(String::as_str) == Some("yes"),
        proxy_command: non_none("proxycommand"),
        proxy_use_fdpass: map.get("proxyusefdpass").map(String::as_str) == Some("yes"),
        proxy_jump: non_none("proxyjump"),
        control_master: map.get("controlmaster").cloned().unwrap_or_else(|| "no".to_string()),
        control_path: non_none("controlpath").map(|value| resolve_ssh_config_home_path(&value, home)),
        control_persist: map.get("controlpersist").cloned().unwrap_or_else(|| "no".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/testuser";

    fn parse(stdout: &str) -> SshResolvedConfig {
        parse_ssh_g_output(stdout, HOME)
    }

    #[test]
    fn parses_a_typical_resolved_config() {
        let stdout = "host myserver\nhostname 192.168.1.100\nuser deploy\nport 2222\nidentityfile ~/.ssh/id_ed25519\nidentitiesonly yes\nforwardagent no\nproxycommand none\nproxyjump none\ncontrolmaster auto\ncontrolpath ~/.ssh/cm-%r@%h:%p\ncontrolpersist 600\n";
        let config = parse(stdout);
        assert_eq!(config.hostname, "192.168.1.100");
        assert_eq!(config.user.as_deref(), Some("deploy"));
        assert_eq!(config.port, Some(2222));
        assert_eq!(config.identity_file, vec!["/home/testuser/.ssh/id_ed25519".to_string()]);
        assert!(config.identities_only);
        assert!(!config.forward_agent);
        assert_eq!(config.proxy_command, None);
        assert_eq!(config.proxy_jump, None);
        assert_eq!(config.control_master, "auto");
        assert_eq!(config.control_path.as_deref(), Some("/home/testuser/.ssh/cm-%r@%h:%p"));
        assert_eq!(config.control_persist, "600");
    }

    #[test]
    fn parses_gssapi_authentication_flag() {
        // `ssh -G` prints `gssapiauthentication yes|no`; mirrors `=== 'yes'`.
        assert!(parse("gssapiauthentication yes\n").gssapi_authentication);
        assert!(!parse("gssapiauthentication no\n").gssapi_authentication);
        // Absent → false (TS `map.get(...) === 'yes'` is false for undefined).
        assert!(!parse("hostname h\n").gssapi_authentication);
    }

    #[test]
    fn defaults_missing_keys_like_the_ts() {
        // Empty stdout → hostname '', port 22, controlmaster/persist 'no', flags false.
        let config = parse("");
        assert_eq!(config.hostname, "");
        assert_eq!(config.user, None);
        assert_eq!(config.port, Some(22));
        assert!(config.identity_file.is_empty());
        assert_eq!(config.identity_agent, None);
        assert!(!config.identities_only);
        assert_eq!(config.control_master, "no");
        assert_eq!(config.control_persist, "no");
    }

    #[test]
    fn collects_multiple_identity_files_in_order() {
        let config = parse("identityfile ~/.ssh/a\nidentityfile /etc/ssh/b\nidentityfile ~/c\n");
        assert_eq!(
            config.identity_file,
            vec![
                "/home/testuser/.ssh/a".to_string(),
                "/etc/ssh/b".to_string(),
                "/home/testuser/c".to_string()
            ]
        );
    }

    #[test]
    fn keeps_real_proxy_command_and_expands_control_path() {
        let config = parse("proxycommand ssh -W %h:%p bastion\ncontrolpath none\n");
        assert_eq!(config.proxy_command.as_deref(), Some("ssh -W %h:%p bastion"));
        assert_eq!(config.control_path, None);
    }

    #[test]
    fn skips_lines_without_a_space_and_lowercases_keys() {
        let config = parse("HostName Example.Com\nblanklinewithoutspace\nUser Root\n");
        assert_eq!(config.hostname, "Example.Com");
        assert_eq!(config.user.as_deref(), Some("Root"));
    }

    #[test]
    fn empty_user_is_dropped_and_last_value_wins() {
        // `map.get('user') || undefined`: empty → None; duplicate host key → last wins.
        let config = parse("user \nhostname first\nhostname second\n");
        assert_eq!(config.user, None);
        assert_eq!(config.hostname, "second");
    }
}
