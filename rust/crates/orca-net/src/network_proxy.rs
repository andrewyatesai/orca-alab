//! Network proxy settings, ported from `src/shared/network-proxy.ts`.
//!
//! Normalizes/validates a configured proxy URL (dropping path/query/fragment),
//! reads proxy settings from the environment with standard precedence, builds
//! the child-process proxy env, and redacts credentials for diagnostics.
//!
//! The WHATWG `URL` parse is replaced by a targeted proxy-URL parser: proxy
//! URLs are `scheme://[user[:pass]@]host[:port]` and the only output is
//! `scheme://[auth@]host`, so the full URL spec (paths, default-port dropping,
//! IDNA) is not needed here.

use std::collections::BTreeMap;

const PROXY_URL_MAX_LENGTH: usize = 2048;
const PROXY_BYPASS_RULES_MAX_LENGTH: usize = 4096;
const PROXY_PROTOCOLS: [&str; 5] = ["http:", "https:", "socks:", "socks4:", "socks5:"];
const PROXY_ENV_KEYS: [&str; 6] =
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"];
const NO_PROXY_ENV_KEYS: [&str; 2] = ["NO_PROXY", "no_proxy"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkProxySettings<'a> {
    pub http_proxy_url: Option<&'a str>,
    pub http_proxy_bypass_rules: Option<&'a str>,
}

/// Result of validating a proxy URL: `ok` with a normalized `value`, or not-`ok`
/// with an empty `value` and a user-facing `message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyUrlValidation {
    pub ok: bool,
    pub value: String,
    pub message: Option<String>,
}

fn ok_value(value: &str) -> ProxyUrlValidation {
    ProxyUrlValidation { ok: true, value: value.to_string(), message: None }
}

fn invalid(message: &str) -> ProxyUrlValidation {
    ProxyUrlValidation { ok: false, value: String::new(), message: Some(message.to_string()) }
}

#[derive(Clone)]
struct ParsedProxyUrl {
    protocol: String,
    username: String,
    password: String,
    hostname: String,
    host: String,
}

fn parse_proxy_url(input: &str) -> Option<ParsedProxyUrl> {
    let (scheme, rest) = input.split_once("://")?;
    if scheme.is_empty()
        || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((user, host)) => (user, host),
        None => ("", authority),
    };
    let (username, password) = match userinfo.split_once(':') {
        Some((user, pass)) => (user.to_string(), pass.to_string()),
        None => (userinfo.to_string(), String::new()),
    };
    Some(ParsedProxyUrl {
        protocol: format!("{}:", scheme.to_ascii_lowercase()),
        username,
        password,
        hostname: hostport.split(':').next().unwrap_or(hostport).to_string(),
        host: hostport.to_string(),
    })
}

fn format_proxy_url(url: &ParsedProxyUrl) -> String {
    let auth = if url.username.is_empty() && url.password.is_empty() {
        String::new()
    } else if url.password.is_empty() {
        format!("{}@", url.username)
    } else {
        format!("{}:{}@", url.username, url.password)
    };
    format!("{}//{}{}", url.protocol, auth, url.host)
}

pub fn normalize_proxy_url(value: Option<&str>) -> ProxyUrlValidation {
    let Some(value) = value else {
        return ok_value("");
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return ok_value("");
    }
    if trimmed.chars().count() > PROXY_URL_MAX_LENGTH {
        return invalid("Proxy URL is too long.");
    }
    let Some(parsed) = parse_proxy_url(trimmed) else {
        return invalid("Enter a valid proxy URL.");
    };
    if !PROXY_PROTOCOLS.iter().any(|protocol| *protocol == parsed.protocol) {
        return invalid("Use an http, https, socks, socks4, or socks5 proxy URL.");
    }
    if parsed.hostname.is_empty() {
        return invalid("Proxy URL must include a host.");
    }
    ok_value(&format_proxy_url(&parsed))
}

pub fn normalize_proxy_bypass_rules(value: Option<&str>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let limited: String = value.chars().take(PROXY_BYPASS_RULES_MAX_LENGTH).collect();
    limited
        .split([';', ',', '\n'])
        .map(str::trim)
        .filter(|rule| !rule.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

fn lookup<'a>(env: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| *k == key).map(|(_, value)| *value).filter(|value| !value.is_empty())
}

pub fn get_proxy_url_from_environment(env: &[(&str, &str)]) -> ProxyUrlValidation {
    for key in PROXY_ENV_KEYS {
        if let Some(value) = lookup(env, key) {
            return normalize_proxy_url(Some(value));
        }
    }
    ok_value("")
}

pub fn get_proxy_bypass_rules_from_environment(env: &[(&str, &str)]) -> String {
    for key in NO_PROXY_ENV_KEYS {
        if let Some(value) = lookup(env, key) {
            return normalize_proxy_bypass_rules(Some(value));
        }
    }
    String::new()
}

pub fn build_configured_proxy_env(settings: &NetworkProxySettings) -> BTreeMap<String, String> {
    let proxy = normalize_proxy_url(settings.http_proxy_url);
    if !proxy.ok || proxy.value.is_empty() {
        return BTreeMap::new();
    }
    let mut env = BTreeMap::new();
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        env.insert(key.to_string(), proxy.value.clone());
    }
    let bypass_rules = normalize_proxy_bypass_rules(settings.http_proxy_bypass_rules);
    // Explicit Orca proxy settings must not inherit a parent shell's NO_PROXY;
    // the bypass field is the single source for child-process bypass behaviour.
    let no_proxy = if bypass_rules.is_empty() { String::new() } else { bypass_rules.replace(';', ",") };
    env.insert("NO_PROXY".to_string(), no_proxy.clone());
    env.insert("no_proxy".to_string(), no_proxy);
    env
}

pub fn redact_proxy_url(value: &str) -> String {
    let parsed = normalize_proxy_url(Some(value));
    if !parsed.ok || parsed.value.is_empty() {
        return parsed.value;
    }
    let Some(mut url) = parse_proxy_url(&parsed.value) else {
        return parsed.value;
    };
    if !url.username.is_empty() || !url.password.is_empty() {
        url.username = "***".to_string();
        url.password = if url.password.is_empty() { String::new() } else { "***".to_string() };
    }
    format_proxy_url(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_supported_proxy_urls_without_path_query_or_fragment() {
        assert_eq!(
            normalize_proxy_url(Some(" https://user:pass@proxy.example.com:8443/path?q=1#secret ")),
            ProxyUrlValidation {
                ok: true,
                value: "https://user:pass@proxy.example.com:8443".to_string(),
                message: None,
            }
        );
    }

    #[test]
    fn rejects_unsupported_or_malformed_proxy_urls() {
        assert!(!normalize_proxy_url(Some("file:///tmp/proxy")).ok);
        assert!(!normalize_proxy_url(Some("http://")).ok);
        assert!(!normalize_proxy_url(Some("not-a-url")).ok);
    }

    #[test]
    fn normalizes_bypass_rules_from_common_separator_styles() {
        assert_eq!(
            normalize_proxy_bypass_rules(Some("localhost, 127.0.0.1; *.internal\n<local>")),
            "localhost;127.0.0.1;*.internal;<local>"
        );
    }

    #[test]
    fn uses_standard_proxy_environment_precedence() {
        assert_eq!(
            get_proxy_url_from_environment(&[
                ("HTTP_PROXY", "http://plain.example:8080"),
                ("HTTPS_PROXY", "https://secure.example:8443"),
            ]),
            ProxyUrlValidation {
                ok: true,
                value: "https://secure.example:8443".to_string(),
                message: None,
            }
        );
        assert_eq!(
            get_proxy_bypass_rules_from_environment(&[("no_proxy", "localhost,*.internal")]),
            "localhost;*.internal"
        );
    }

    #[test]
    fn builds_local_pty_proxy_env_only_from_explicit_settings() {
        let env = build_configured_proxy_env(&NetworkProxySettings {
            http_proxy_url: Some("http://proxy.example:8080"),
            http_proxy_bypass_rules: Some("localhost;*.internal"),
        });
        let expected: BTreeMap<String, String> = [
            ("HTTP_PROXY", "http://proxy.example:8080"),
            ("HTTPS_PROXY", "http://proxy.example:8080"),
            ("ALL_PROXY", "http://proxy.example:8080"),
            ("http_proxy", "http://proxy.example:8080"),
            ("https_proxy", "http://proxy.example:8080"),
            ("all_proxy", "http://proxy.example:8080"),
            ("NO_PROXY", "localhost,*.internal"),
            ("no_proxy", "localhost,*.internal"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(env, expected);

        assert_eq!(
            build_configured_proxy_env(&NetworkProxySettings {
                http_proxy_url: Some(""),
                http_proxy_bypass_rules: None,
            }),
            BTreeMap::new()
        );
    }

    #[test]
    fn redacts_credentials_for_diagnostics() {
        assert_eq!(
            redact_proxy_url("http://user:pass@proxy.example:8080"),
            "http://***:***@proxy.example:8080"
        );
    }
}
