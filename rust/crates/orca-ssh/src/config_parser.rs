//! OpenSSH config parsing, ported from `parseSshConfig` in
//! `src/main/ssh/ssh-config-parser.ts`. Pure: `~` expansion is parameterized on
//! `home` (the TS reads `os.homedir()`), so it's fully testable.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SshConfigHost {
    pub host: String,
    pub hostname: Option<String>,
    pub port: Option<u32>,
    pub user: Option<String>,
    pub identity_file: Option<String>,
    pub identity_agent: Option<String>,
    pub identities_only: Option<bool>,
    pub proxy_command: Option<String>,
    pub proxy_use_fdpass: Option<bool>,
    pub proxy_jump: Option<String>,
}

/// Parse an OpenSSH config file into structured host entries. Host blocks with
/// multiple patterns yield one entry per concrete alias; wildcard/negated/
/// pattern-only `Host` lines are skipped. `~` paths expand against `home`.
pub fn parse_ssh_config(content: &str, home: &str) -> Vec<SshConfigHost> {
    let mut hosts: Vec<SshConfigHost> = Vec::new();
    let mut current: Vec<SshConfigHost> = Vec::new();

    for raw_line in content.split('\n') {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = parse_config_directive(line) else {
            continue;
        };

        if key == "host" {
            if !current.is_empty() {
                hosts.append(&mut current);
            }
            let concrete: Vec<String> = split_openssh_arguments(&raw_value)
                .into_iter()
                .filter(|p| !p.starts_with('!') && !p.contains('*') && !p.contains('?'))
                .collect();
            current = concrete
                .into_iter()
                .map(|host| SshConfigHost { host, ..Default::default() })
                .collect();
            continue;
        }
        if key == "match" {
            if !current.is_empty() {
                hosts.append(&mut current);
            }
            continue;
        }
        if current.is_empty() {
            continue;
        }

        let value = parse_scalar_config_value(&raw_value);
        match key.as_str() {
            "hostname" => set_all(&mut current, |h| h.hostname = Some(value.clone())),
            "port" => {
                // TS: `parseInt(value, 10) || 22` — invalid or 0 falls back to 22.
                let port = value.parse::<u32>().ok().filter(|&p| p > 0).unwrap_or(22);
                set_all(&mut current, |h| h.port = Some(port));
            }
            "user" => set_all(&mut current, |h| h.user = Some(value.clone())),
            "identityfile" => {
                let resolved = resolve_ssh_config_home_path(&value, home);
                set_all(&mut current, |h| h.identity_file = Some(resolved.clone()));
            }
            "identityagent" => {
                let resolved = resolve_ssh_config_home_path(&value, home);
                set_all(&mut current, |h| h.identity_agent = Some(resolved.clone()));
            }
            "identitiesonly" => {
                let yes = value.eq_ignore_ascii_case("yes");
                set_all(&mut current, |h| h.identities_only = Some(yes));
            }
            // OpenSSH preserves the rest of the line for ProxyCommand (a shell snippet).
            "proxycommand" => {
                let command = raw_value.trim().to_string();
                set_all(&mut current, |h| h.proxy_command = Some(command.clone()));
            }
            "proxyusefdpass" => {
                let yes = value.eq_ignore_ascii_case("yes");
                set_all(&mut current, |h| h.proxy_use_fdpass = Some(yes));
            }
            "proxyjump" => set_all(&mut current, |h| h.proxy_jump = Some(value.clone())),
            _ => {}
        }
    }

    if !current.is_empty() {
        hosts.append(&mut current);
    }
    hosts
}

fn set_all(hosts: &mut [SshConfigHost], mut apply: impl FnMut(&mut SshConfigHost)) {
    for host in hosts {
        apply(host);
    }
}

/// `^([^=\s]+)(?:\s*=\s*|\s+)(.*)$` — key (lowercased) + value (trimmed).
fn parse_config_directive(line: &str) -> Option<(String, String)> {
    let key_end = line.find(|c: char| c == '=' || c.is_whitespace())?;
    if key_end == 0 {
        return None;
    }
    let key = line[..key_end].to_lowercase();
    let after_key = line[key_end..].trim_start();
    let value = after_key.strip_prefix('=').map(str::trim_start).unwrap_or(after_key);
    Some((key, value.trim().to_string()))
}

fn parse_scalar_config_value(input: &str) -> String {
    split_openssh_arguments(input).into_iter().next().unwrap_or_default()
}

/// OpenSSH argument splitting: quote- and escape-aware, stops at an unquoted `#`.
fn split_openssh_arguments(input: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if in_quotes && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if !in_quotes && ch == '#' {
            break;
        }
        if !in_quotes && ch.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Expand a leading `~` (with `/` or `\` separators) against `home`; other
/// values pass through. Separators are normalised to `/`.
fn resolve_ssh_config_home_path(value: &str, home: &str) -> String {
    let bytes = value.as_bytes();
    let tilde_path = bytes.first() == Some(&b'~')
        && (value.len() == 1 || matches!(bytes[1], b'/' | b'\\'));
    if !tilde_path {
        return value.to_string();
    }
    let rest = value[1..].replace('\\', "/");
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        home.to_string()
    } else {
        format!("{home}/{rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/testuser";

    fn host(name: &str) -> SshConfigHost {
        SshConfigHost { host: name.to_string(), ..Default::default() }
    }

    #[test]
    fn parses_basic_host_block() {
        let config = "Host myserver\n  HostName 192.168.1.100\n  User deploy\n  Port 2222\n";
        assert_eq!(
            parse_ssh_config(config, HOME),
            vec![SshConfigHost {
                host: "myserver".into(),
                hostname: Some("192.168.1.100".into()),
                user: Some("deploy".into()),
                port: Some(2222),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn parses_multiple_blocks() {
        let config = "Host staging\n  HostName staging.example.com\n  User admin\n\nHost production\n  HostName prod.example.com\n  User deploy\n  Port 2222\n";
        let hosts = parse_ssh_config(config, HOME);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host, "staging");
        assert_eq!(hosts[1].host, "production");
        assert_eq!(hosts[1].port, Some(2222));
    }

    #[test]
    fn skips_wildcard_and_pattern_only_hosts() {
        let config = "Host *\n  ServerAliveInterval 60\n\nHost *.example.com\n  User admin\n\nHost myserver\n  HostName 10.0.0.1\n";
        let hosts = parse_ssh_config(config, HOME);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "myserver");
    }

    #[test]
    fn expands_tilde_paths_posix_and_windows() {
        let posix = parse_ssh_config("Host s\n  IdentityFile ~/.ssh/id_ed25519\n", HOME);
        assert_eq!(posix[0].identity_file.as_deref(), Some("/home/testuser/.ssh/id_ed25519"));
        let windows = parse_ssh_config("Host s\n  IdentityFile ~\\.ssh\\id_ed25519\n", HOME);
        assert_eq!(windows[0].identity_file.as_deref(), Some("/home/testuser/.ssh/id_ed25519"));
    }

    #[test]
    fn parses_quoted_scalars_with_inline_comments() {
        let config = "Host quoted\n  HostName \"localhost\" # local test\n  User \"deploy\" # u\n  Port \"2202\" # p\n  IdentityFile \"~/.ssh/id with space\" # key\n  IdentityAgent \"~/.1password/agent sock\" # sock\n  IdentitiesOnly \"yes\" # limit\n  ProxyJump \"bastion\" # jump\n";
        assert_eq!(
            parse_ssh_config(config, HOME),
            vec![SshConfigHost {
                host: "quoted".into(),
                hostname: Some("localhost".into()),
                user: Some("deploy".into()),
                port: Some(2202),
                identity_file: Some("/home/testuser/.ssh/id with space".into()),
                identity_agent: Some("/home/testuser/.1password/agent sock".into()),
                identities_only: Some(true),
                proxy_jump: Some("bastion".into()),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn parses_equals_form_and_case_insensitive_keywords() {
        let config = "Host eq\n  HOSTNAME=eq.example.com\n  Port = 2200\n";
        let hosts = parse_ssh_config(config, HOME);
        assert_eq!(hosts[0].hostname.as_deref(), Some("eq.example.com"));
        assert_eq!(hosts[0].port, Some(2200));
    }

    #[test]
    fn preserves_proxycommand_rest_of_line() {
        let config = "Host p\n  ProxyCommand ssh -W %h:%p bastion # via\n";
        // ProxyCommand keeps the full line (a shell snippet), unlike scalars.
        assert_eq!(
            parse_ssh_config(config, HOME)[0].proxy_command.as_deref(),
            Some("ssh -W %h:%p bastion # via")
        );
    }

    #[test]
    fn match_directive_stops_the_current_block() {
        let config = "Host myserver\n  HostName example.com\n\nMatch host *.internal\n  User internal-admin\n\nHost other\n  HostName other.com\n";
        let hosts = parse_ssh_config(config, HOME);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host, "myserver");
        assert_eq!(hosts[1].host, "other");
    }

    #[test]
    fn one_host_per_concrete_alias_on_multi_pattern_line() {
        let config = "Host staging stage *.example.com\n  HostName staging.example.com\n";
        assert_eq!(
            parse_ssh_config(config, HOME),
            vec![
                SshConfigHost { host: "staging".into(), hostname: Some("staging.example.com".into()), ..Default::default() },
                SshConfigHost { host: "stage".into(), hostname: Some("staging.example.com".into()), ..Default::default() },
            ]
        );
    }

    #[test]
    fn empty_input_yields_no_hosts() {
        assert_eq!(parse_ssh_config("", HOME), Vec::new());
    }

    #[test]
    fn ignores_directives_before_any_host() {
        // Leading directives with no Host block are dropped (current empty).
        let config = "HostName orphan.example.com\nHost real\n  HostName real.example.com\n";
        let hosts = parse_ssh_config(config, HOME);
        assert_eq!(hosts, vec![host("real").with_hostname("real.example.com")]);
    }

    impl SshConfigHost {
        fn with_hostname(mut self, hostname: &str) -> Self {
            self.hostname = Some(hostname.to_string());
            self
        }
    }
}
