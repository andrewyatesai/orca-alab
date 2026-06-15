// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Allowlist enforcement for Safety mode.
//!
//! Safety mode restricts MCP tools, plugins, network targets, and shell
//! commands to an explicit allowlist. This module provides:
//!
//! - [`AllowlistConfig`] — the allowlist data, parsed from TOML
//! - [`init_allowlist`] — one-shot initialization (mirrors [`super::init_mode`])
//! - `is_*_allowed()` — per-subsystem checks called from gate sites
//!
//! **Fail-closed:** if [`init_allowlist`] is never called, all `is_*_allowed()`
//! functions return `false` for `Allowlist`/`Restricted` capability levels.
//! The launcher must explicitly provide an allowlist for Safety mode to
//! permit anything.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use crate::ContainmentPolicy;

/// Global allowlist config, set once at startup after [`super::init_mode`].
static ALLOWLIST: OnceLock<AllowlistConfig> = OnceLock::new();

/// Allowlist configuration for Safety mode enforcement.
///
/// Each field lists the identifiers permitted for that subsystem.
/// Empty lists mean "deny all" — this is intentional fail-closed behavior.
#[derive(Debug, Clone, Default)]
pub struct AllowlistConfig {
    /// MCP tool names permitted in Safety mode (e.g. `["read_file", "write_file"]`).
    pub mcp_tools: Vec<String>,
    /// Plugin manifest IDs permitted in Safety mode.
    pub plugins: Vec<String>,
    /// Network targets permitted in Safety mode.
    /// Format: `"host:port"`, `"[ipv6]:port"`, `"host:*"`, `"[ipv6]:*"`,
    /// `"unix:/path"`.
    pub network: Vec<String>,
    /// Process/command executable paths permitted in Safety mode.
    ///
    /// Rules must resolve to absolute filesystem paths. Runtime command names
    /// are resolved through `$PATH` and canonicalized before comparison.
    pub processes: Vec<String>,
}

/// Initialize the global allowlist. Called once at startup, after [`super::init_mode`].
///
/// If not called, Safety mode gates deny all allowlisted operations (fail-closed).
///
/// # Errors
///
/// Returns [`AllowlistError::AlreadyInitialized`] if called more than once.
pub fn init_allowlist(config: AllowlistConfig) -> Result<(), AllowlistError> {
    ALLOWLIST
        .set(config)
        .map_err(|_| AllowlistError::AlreadyInitialized)
}

/// Check if an MCP tool is allowed under the current containment mode.
///
/// Returns `true` for `Full` capability, checks the allowlist for `Allowlist`,
/// and `false` for `Disabled` or unknown variants.
#[must_use]
pub fn is_mcp_allowed(tool_name: &str) -> bool {
    let mode = crate::mode_or_containment();
    match ContainmentPolicy::mcp(mode) {
        crate::McpCapability::Full => true,
        crate::McpCapability::Allowlist => match ALLOWLIST.get() {
            Some(cfg) => cfg.mcp_tools.iter().any(|t| t == tool_name),
            None => false, // fail-closed
        },
        _ => false, // Disabled or unknown
    }
}

/// Check if a plugin is allowed under the current containment mode.
#[must_use]
pub fn is_plugin_allowed(plugin_id: &str) -> bool {
    let mode = crate::mode_or_containment();
    match ContainmentPolicy::plugins(mode) {
        crate::PluginCapability::Full => true,
        crate::PluginCapability::Allowlist => match ALLOWLIST.get() {
            Some(cfg) => cfg.plugins.iter().any(|p| p == plugin_id),
            None => false,
        },
        _ => false,
    }
}

/// Check if a network target is allowed under the current containment mode.
///
/// Supports exact match and `host:*` wildcard (any port on that host).
#[must_use]
pub fn is_network_allowed(target: &str) -> bool {
    let mode = crate::mode_or_containment();
    match ContainmentPolicy::network(mode) {
        crate::NetworkCapability::Full => true,
        crate::NetworkCapability::Allowlist => match ALLOWLIST.get() {
            Some(cfg) => cfg.network.iter().any(|rule| network_matches(rule, target)),
            None => false,
        },
        _ => false, // None or unknown
    }
}

/// Check if a shell/command is allowed under the current containment mode.
///
/// # TOCTOU caveat
///
/// This function uses `canonicalize()` to resolve symlinks at check time.
/// Between this check and the actual `exec()`, the file could be swapped
/// (symlink or rename race). For security-critical callers, use
/// [`verify_executable_fd`] after opening the file descriptor to confirm
/// the resolved path still matches the allowlist at exec time.
#[must_use]
pub fn is_process_allowed(command: &str) -> bool {
    let mode = crate::mode_or_containment();
    match ContainmentPolicy::process(mode) {
        crate::ProcessCapability::Full => true,
        crate::ProcessCapability::Restricted => match ALLOWLIST.get() {
            Some(cfg) => process_allowed_by_config(cfg, command),
            None => false,
        },
        _ => false, // NoFork or unknown
    }
}

/// Verify that an already-opened file descriptor points to an allowlisted
/// executable.
///
/// This closes the TOCTOU window between `is_process_allowed` (which uses
/// `canonicalize()` at check time) and `exec()`. By resolving the path
/// from the open fd, we verify what will actually be executed rather than
/// what was at the path when we checked.
///
/// On macOS, reads from `/dev/fd/{fd}` via `fcntl(F_GETPATH)`.
/// On Linux, reads from `/proc/self/fd/{fd}`.
/// On other platforms, falls back to `false` (fail-closed).
///
/// # Arguments
///
/// * `fd` - An open file descriptor for the executable (opened with `O_RDONLY`).
///
/// Returns `true` if the fd resolves to an allowlisted executable path.
#[cfg(unix)]
#[must_use]
pub fn verify_executable_fd(fd: std::os::unix::io::RawFd) -> bool {
    let mode = crate::mode_or_containment();
    match ContainmentPolicy::process(mode) {
        crate::ProcessCapability::Full => true,
        crate::ProcessCapability::Restricted => match ALLOWLIST.get() {
            Some(cfg) => {
                let Some(path) = fd_to_path(fd) else {
                    return false; // fail-closed
                };
                cfg.processes
                    .iter()
                    .filter_map(|rule| normalize_process_rule(rule))
                    .any(|rule| rule == path)
            }
            None => false,
        },
        _ => false,
    }
}

/// Resolve an open file descriptor to its filesystem path.
///
/// Uses platform-specific mechanisms:
/// - macOS: `fcntl(fd, F_GETPATH)`
/// - Linux: `readlink("/proc/self/fd/{fd}")`
#[cfg(unix)]
fn fd_to_path(fd: std::os::unix::io::RawFd) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut buf = vec![0u8; libc::PATH_MAX as usize];
        // SAFETY: buf is a valid mutable buffer of PATH_MAX bytes, fd is a
        // valid file descriptor. F_GETPATH writes a null-terminated path
        // into the buffer.
        let ret = unsafe { libc::fcntl(fd, libc::F_GETPATH, buf.as_mut_ptr()) };
        if ret == -1 {
            return None;
        }
        let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(nul_pos);
        let os_str = std::ffi::OsString::from_vec(buf);
        Some(PathBuf::from(os_str))
    }

    #[cfg(target_os = "linux")]
    {
        let link = format!("/proc/self/fd/{fd}");
        std::fs::read_link(link).ok()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = fd;
        None // fail-closed on unsupported platforms
    }
}

/// Match a network target against an allowlist rule.
///
/// Rules:
/// - Exact match: `"localhost:8080"` matches `"localhost:8080"`
/// - Wildcard port: `"localhost:*"` matches `"localhost:8080"`, `"localhost:443"`
/// - Unix sockets: `"unix:/tmp/foo.sock"` matches exactly
fn network_matches(rule: &str, target: &str) -> bool {
    if rule == target {
        return true;
    }
    let Some(rule) = parse_network_rule(rule) else {
        return false;
    };
    let Some(target) = parse_network_target(target) else {
        return false;
    };
    match (rule, target) {
        (
            NetworkRule::Socket {
                host: rule_host,
                port: rule_port,
            },
            NetworkTarget::Socket {
                host: target_host,
                port: target_port,
            },
        ) => rule_host == target_host && rule_port.matches(target_port),
        (NetworkRule::Unix(rule_path), NetworkTarget::Unix(target_path)) => {
            rule_path == target_path
        }
        _ => false,
    }
}

fn process_allowed_by_config(cfg: &AllowlistConfig, command: &str) -> bool {
    let Some(command_path) = normalize_process_command(command) else {
        return false;
    };
    cfg.processes
        .iter()
        .filter_map(|rule| normalize_process_rule(rule))
        .any(|rule| rule == command_path)
}

fn normalize_process_rule(rule: &str) -> Option<PathBuf> {
    let path = Path::new(rule);
    path.is_absolute().then_some(())?;
    path.canonicalize().ok()
}

fn normalize_process_command(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return path.canonicalize().ok();
    }
    if !is_bare_command(path) {
        return None;
    }
    resolve_command_from_path(path)
}

fn is_bare_command(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn resolve_command_from_path(command: &Path) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in candidate_paths(&dir.join(command)) {
            if is_executable_file(&candidate) {
                return candidate.canonicalize().ok();
            }
        }
    }
    None
}

#[cfg(windows)]
fn candidate_paths(base: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let has_extension = base.extension().is_some();
    if has_extension {
        candidates.push(base.to_path_buf());
        return candidates;
    }
    let pathext = std::env::var_os("PATHEXT")
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
        .to_string_lossy()
        .into_owned();
    for ext in pathext.split(';').filter(|ext| !ext.is_empty()) {
        let ext = ext.trim_start_matches('.');
        candidates.push(base.with_extension(ext));
    }
    candidates
}

#[cfg(not(windows))]
fn candidate_paths(base: &Path) -> Vec<PathBuf> {
    vec![base.to_path_buf()]
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetworkRule {
    Socket { host: String, port: PortMatcher },
    Unix(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetworkTarget {
    Socket { host: String, port: u16 },
    Unix(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortMatcher {
    Exact(u16),
    Any,
}

impl PortMatcher {
    fn matches(self, port: u16) -> bool {
        match self {
            Self::Exact(expected) => expected == port,
            Self::Any => true,
        }
    }
}

fn parse_network_rule(rule: &str) -> Option<NetworkRule> {
    let decoded = percent_decode(rule);
    let rule = decoded.as_str();
    if let Some(path) = rule.strip_prefix("unix:") {
        return Some(NetworkRule::Unix(normalize_unix_path(path)));
    }
    if let Some(host) = rule.strip_suffix(":*") {
        return Some(NetworkRule::Socket {
            host: normalize_host(host),
            port: PortMatcher::Any,
        });
    }
    let (host, port) = parse_host_port(rule)?;
    Some(NetworkRule::Socket {
        host,
        port: PortMatcher::Exact(port),
    })
}

fn parse_network_target(target: &str) -> Option<NetworkTarget> {
    let decoded = percent_decode(target);
    let target = decoded.as_str();
    if let Some(path) = target.strip_prefix("unix:") {
        return Some(NetworkTarget::Unix(normalize_unix_path(path)));
    }
    if target.contains("://") {
        return parse_url_target(target);
    }
    let (host, port) = parse_host_port(target)?;
    Some(NetworkTarget::Socket { host, port })
}

fn parse_url_target(target: &str) -> Option<NetworkTarget> {
    let (scheme, remainder) = target.split_once("://")?;
    let authority = remainder
        .split(['/', '?', '#'])
        .next()
        .filter(|authority| !authority.is_empty())?;
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, authority)| authority);

    let (host, port) = if authority.starts_with('[') {
        parse_bracketed_host_port(authority)
            .or_else(|| Some((normalize_host(authority), default_port_for_scheme(scheme)?)))?
    } else if let Some((host, port_str)) = authority.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            (normalize_host(host), port)
        } else {
            (normalize_host(authority), default_port_for_scheme(scheme)?)
        }
    } else {
        (normalize_host(authority), default_port_for_scheme(scheme)?)
    };

    Some(NetworkTarget::Socket { host, port })
}

fn default_port_for_scheme(scheme: &str) -> Option<u16> {
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

fn parse_host_port(value: &str) -> Option<(String, u16)> {
    if value.starts_with('[') {
        return parse_bracketed_host_port(value);
    }
    let (host, port) = value.rsplit_once(':')?;
    Some((normalize_host(host), port.parse().ok()?))
}

fn parse_bracketed_host_port(value: &str) -> Option<(String, u16)> {
    let (host, rest) = value.strip_prefix('[')?.split_once(']')?;
    let port = rest.strip_prefix(':')?.parse().ok()?;
    Some((normalize_host(host), port))
}

fn normalize_host(host: &str) -> String {
    let stripped = host.trim_matches(['[', ']']);
    let decoded = percent_decode(stripped);
    let lowered = decoded.to_ascii_lowercase();
    normalize_ip(&lowered)
}

/// Decode percent-encoded (`%XX`) sequences in a string.
///
/// Invalid sequences (non-hex digits, truncated `%` at end) are passed through
/// verbatim so that malformed input does not silently disappear.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(decoded) = decode_hex_pair(bytes[i + 1], bytes[i + 2])
        {
            out.push(decoded);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Decode a pair of ASCII hex digits into a byte. Returns `None` if either
/// character is not a valid hex digit.
fn decode_hex_pair(hi: u8, lo: u8) -> Option<u8> {
    let h = hex_digit(hi)?;
    let l = hex_digit(lo)?;
    Some(h << 4 | l)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Canonicalize an IP address string so that equivalent representations compare
/// equal. Handles:
///
/// - Leading-zero stripping (e.g. `0177.0.0.01` -> `127.0.0.1`)
/// - Hex IPv4 (e.g. `0x7f000001` -> `127.0.0.1`)
/// - Per-octet hex (e.g. `0x7f.0.0.1` -> `127.0.0.1`)
/// - IPv4-mapped IPv6 (e.g. `::ffff:127.0.0.1` -> `127.0.0.1`)
/// - IPv6 canonicalization via `std::net::Ipv6Addr`
///
/// If the input is not a recognized IP format, it is returned unchanged.
fn normalize_ip(host: &str) -> String {
    // Try hex-encoded single-integer IPv4 (0x7f000001)
    if let Some(ip) = try_parse_hex_ipv4(host) {
        return ip;
    }

    // Try dotted IPv4 with possible octal/hex octets (0177.0.0.01, 0x7f.0.0.1)
    if let Some(ip) = try_parse_mixed_ipv4(host) {
        return ip;
    }

    // Try standard IPv6 parsing (handles ::ffff:x.x.x.x mapped addresses)
    if let Ok(v6) = host.parse::<std::net::Ipv6Addr>() {
        // Convert IPv4-mapped IPv6 to plain IPv4
        if let Some(v4) = v6.to_ipv4_mapped() {
            return v4.to_string();
        }
        // Canonicalize IPv6 (collapses zeros, lowercase)
        return v6.to_string();
    }

    // Try standard IPv4 (std already strips leading zeros on output)
    if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        return v4.to_string();
    }

    host.to_owned()
}

/// Parse a single hex integer like `0x7f000001` as an IPv4 address.
fn try_parse_hex_ipv4(host: &str) -> Option<String> {
    let hex_str = host
        .strip_prefix("0x")
        .or_else(|| host.strip_prefix("0X"))?;
    if hex_str.is_empty() || hex_str.len() > 8 {
        return None;
    }
    // Must be all hex digits (no dots)
    if hex_str.contains('.') {
        return None;
    }
    let val = u32::from_str_radix(hex_str, 16).ok()?;
    let ip = std::net::Ipv4Addr::from(val);
    Some(ip.to_string())
}

/// Parse dotted-quad IPv4 where each octet may be decimal, octal (0-prefixed),
/// or hex (0x-prefixed). E.g. `0177.0.0.01` or `0x7f.0.0.0x01`.
fn try_parse_mixed_ipv4(host: &str) -> Option<String> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    // Only try mixed parsing if at least one octet looks non-decimal
    // (starts with 0 and has more digits, or starts with 0x)
    let needs_mixed = parts.iter().any(|p| {
        (p.len() > 1 && p.starts_with('0') && !p.starts_with("0x") && !p.starts_with("0X"))
            || p.starts_with("0x")
            || p.starts_with("0X")
    });
    if !needs_mixed {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = parse_octet(part)?;
    }
    Some(std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]).to_string())
}

/// Parse a single IPv4 octet that may be decimal, octal (0-prefixed), or hex
/// (0x-prefixed). Returns `None` if the value exceeds 255 or the format is
/// invalid.
fn parse_octet(s: &str) -> Option<u8> {
    if s.is_empty() {
        return None;
    }
    let val = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).ok()?
    } else if s.len() > 1 && s.starts_with('0') {
        // Octal
        u16::from_str_radix(s, 8).ok()?
    } else {
        s.parse::<u16>().ok()?
    };
    u8::try_from(val).ok()
}

/// Normalize a Unix socket path by collapsing redundant separators, `.`
/// components, and resolving `..` components without filesystem access.
fn normalize_unix_path(path: &str) -> String {
    let p = Path::new(path);
    let mut normalized = PathBuf::new();
    for component in p.components() {
        match component {
            Component::CurDir => {} // skip `.`
            Component::ParentDir => {
                normalized.pop(); // apply `..` by removing last component
            }
            other => normalized.push(other),
        }
    }
    normalized.to_string_lossy().into_owned()
}

#[cfg(feature = "allowlist-toml")]
impl AllowlistConfig {
    /// Parse an [`AllowlistConfig`] from a TOML string.
    ///
    /// Requires the `allowlist-toml` feature. Gated so `aterm-core`'s default
    /// build tree does not pull `toml` + `serde` through this crate (#7729).
    ///
    /// # Errors
    ///
    /// Returns [`AllowlistError::Parse`] if the TOML is malformed.
    pub(crate) fn from_toml_str(s: &str) -> Result<Self, AllowlistError> {
        let table: toml::Table = s.parse().map_err(AllowlistError::Parse)?;
        Ok(Self {
            mcp_tools: extract_string_array(&table, "mcp", "allowed"),
            plugins: extract_string_array(&table, "plugins", "allowed"),
            network: extract_string_array(&table, "network", "allowed"),
            processes: extract_string_array(&table, "process", "allowed"),
        })
    }

    /// Parse an [`AllowlistConfig`] from a TOML file.
    ///
    /// Requires the `allowlist-toml` feature. See [`Self::from_toml_str`].
    ///
    /// # Errors
    ///
    /// Returns [`AllowlistError::Io`] if the file cannot be read, or
    /// [`AllowlistError::Parse`] if the TOML is malformed.
    pub fn from_toml_file(path: impl AsRef<std::path::Path>) -> Result<Self, AllowlistError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .map_err(|e| AllowlistError::Io(path.display().to_string(), e))?;
        Self::from_toml_str(&content)
    }
}

/// Extract a string array from a TOML table at `[section].key`.
/// Returns an empty vec if the section or key is missing.
#[cfg(feature = "allowlist-toml")]
fn extract_string_array(table: &toml::Table, section: &str, key: &str) -> Vec<String> {
    table
        .get(section)
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Errors from allowlist operations.
#[derive(Debug, aterm_error::Error)]
#[non_exhaustive]
pub enum AllowlistError {
    /// Failed to read allowlist file.
    #[error("failed to read allowlist from {0}: {1}")]
    Io(String, #[source] std::io::Error),
    /// Failed to parse TOML.
    ///
    /// Only produced when the `allowlist-toml` feature is enabled (#7729).
    #[cfg(feature = "allowlist-toml")]
    #[error("failed to parse allowlist TOML: {0}")]
    Parse(#[source] toml::de::Error),
    /// Allowlist was already initialized.
    #[error("allowlist already initialized")]
    AlreadyInitialized,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[cfg(feature = "allowlist-toml")]
    #[test]
    fn parse_valid_toml() {
        let toml = r#"
[mcp]
allowed = ["read_file", "write_file"]

[plugins]
allowed = ["spell-check"]

[network]
allowed = ["localhost:*", "unix:/tmp/aterm.sock"]

[process]
allowed = ["/bin/bash", "/bin/zsh"]
"#;
        let config = AllowlistConfig::from_toml_str(toml).unwrap();
        assert_eq!(config.mcp_tools, vec!["read_file", "write_file"]);
        assert_eq!(config.plugins, vec!["spell-check"]);
        assert_eq!(config.network, vec!["localhost:*", "unix:/tmp/aterm.sock"]);
        assert_eq!(config.processes, vec!["/bin/bash", "/bin/zsh"]);
    }

    #[cfg(feature = "allowlist-toml")]
    #[test]
    fn parse_empty_toml() {
        let config = AllowlistConfig::from_toml_str("").unwrap();
        assert!(config.mcp_tools.is_empty());
        assert!(config.plugins.is_empty());
        assert!(config.network.is_empty());
        assert!(config.processes.is_empty());
    }

    #[cfg(feature = "allowlist-toml")]
    #[test]
    fn parse_partial_toml() {
        let toml = r#"
[mcp]
allowed = ["read_file"]
"#;
        let config = AllowlistConfig::from_toml_str(toml).unwrap();
        assert_eq!(config.mcp_tools, vec!["read_file"]);
        assert!(config.plugins.is_empty());
        assert!(config.network.is_empty());
        assert!(config.processes.is_empty());
    }

    #[cfg(feature = "allowlist-toml")]
    #[test]
    fn parse_invalid_toml() {
        let result = AllowlistConfig::from_toml_str("not valid [[[toml");
        assert!(result.is_err());
    }

    #[test]
    fn network_exact_match() {
        assert!(network_matches("localhost:8080", "localhost:8080"));
        assert!(!network_matches("localhost:8080", "localhost:9090"));
    }

    #[test]
    fn network_wildcard_port() {
        assert!(network_matches("localhost:*", "localhost:8080"));
        assert!(network_matches("localhost:*", "localhost:443"));
        assert!(!network_matches("localhost:*", "example.com:80"));
    }

    #[test]
    fn network_wildcard_matches_bracketed_ipv6() {
        assert!(network_matches("::1:*", "[::1]:8080"));
        assert!(network_matches("[::1]:*", "[::1]:443"));
        assert!(!network_matches("[::1]:*", "[::2]:443"));
    }

    #[test]
    fn network_matches_https_url_with_default_port() {
        assert!(network_matches(
            "example.com:443",
            "https://example.com/login"
        ));
        assert!(network_matches(
            "example.com:*",
            "https://example.com/login"
        ));
        assert!(!network_matches(
            "example.com:80",
            "https://example.com/login"
        ));
    }

    #[test]
    fn network_matches_bracketed_ipv6_url() {
        assert!(network_matches("::1:443", "https://[::1]/oauth"));
        assert!(network_matches("::1:*", "https://[::1]:8443/oauth"));
        assert!(!network_matches("::2:*", "https://[::1]:8443/oauth"));
    }

    #[test]
    fn network_unix_socket_exact() {
        assert!(network_matches(
            "unix:/tmp/aterm.sock",
            "unix:/tmp/aterm.sock"
        ));
        assert!(!network_matches(
            "unix:/tmp/aterm.sock",
            "unix:/tmp/other.sock"
        ));
    }

    #[test]
    fn default_config_denies_all() {
        let config = AllowlistConfig::default();
        assert!(config.mcp_tools.is_empty());
        assert!(config.plugins.is_empty());
        assert!(config.network.is_empty());
        assert!(config.processes.is_empty());
    }

    #[test]
    fn process_relative_path_is_rejected() {
        assert!(normalize_process_command("./bash").is_none());
        assert!(normalize_process_command("../bin/bash").is_none());
    }

    #[test]
    fn process_relative_rule_is_rejected() {
        assert!(normalize_process_rule("bash").is_none());
        assert!(normalize_process_rule("./bash").is_none());
    }

    #[test]
    fn process_canonicalizes_parent_segments_before_match() {
        let command = std::env::current_exe().unwrap();
        let canonical = command.canonicalize().unwrap();
        let variant = canonical
            .parent()
            .unwrap()
            .join("..")
            .join(canonical.parent().unwrap().file_name().unwrap())
            .join(canonical.file_name().unwrap());
        let config = AllowlistConfig {
            processes: vec![canonical.display().to_string()],
            ..AllowlistConfig::default()
        };
        assert!(process_allowed_by_config(
            &config,
            variant.to_str().unwrap()
        ));
    }

    #[test]
    fn process_canonicalizes_absolute_command_before_match() {
        let command = std::env::current_exe().unwrap();
        let canonical = command.canonicalize().unwrap();
        let variant = canonical
            .parent()
            .unwrap()
            .join(".")
            .join(canonical.file_name().unwrap());
        let config = AllowlistConfig {
            processes: vec![canonical.display().to_string()],
            ..AllowlistConfig::default()
        };
        assert!(process_allowed_by_config(
            &config,
            variant.to_str().unwrap()
        ));
    }

    // --- Percent-decode tests ---

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("localhost"), "localhost");
        assert_eq!(percent_decode("%6c%6f%63%61%6c%68%6f%73%74"), "localhost");
        assert_eq!(percent_decode("exam%70le.com"), "example.com");
    }

    #[test]
    fn percent_decode_uppercase_hex() {
        assert_eq!(percent_decode("%4A%4B"), "JK");
        assert_eq!(percent_decode("%4a%4b"), "JK");
    }

    #[test]
    fn percent_decode_passthrough_invalid() {
        assert_eq!(percent_decode("foo%2"), "foo%2");
        assert_eq!(percent_decode("foo%zz"), "foo%zz");
        assert_eq!(percent_decode("foo%"), "foo%");
    }

    // --- IP normalization tests ---

    #[test]
    fn normalize_ip_hex_ipv4() {
        assert_eq!(normalize_ip("0x7f000001"), "127.0.0.1");
        assert_eq!(normalize_ip("0X7F000001"), "127.0.0.1");
        assert_eq!(normalize_ip("0x00000000"), "0.0.0.0");
        assert_eq!(normalize_ip("0xffffffff"), "255.255.255.255");
    }

    #[test]
    fn normalize_ip_octal_dotted() {
        assert_eq!(normalize_ip("0177.0.0.01"), "127.0.0.1");
        assert_eq!(normalize_ip("0300.0250.0.01"), "192.168.0.1");
    }

    #[test]
    fn normalize_ip_hex_dotted() {
        assert_eq!(normalize_ip("0x7f.0.0.0x01"), "127.0.0.1");
    }

    #[test]
    fn normalize_ip_standard_ipv4_passthrough() {
        assert_eq!(normalize_ip("127.0.0.1"), "127.0.0.1");
        assert_eq!(normalize_ip("192.168.1.1"), "192.168.1.1");
    }

    #[test]
    fn normalize_ip_ipv4_mapped_ipv6() {
        assert_eq!(normalize_ip("::ffff:127.0.0.1"), "127.0.0.1");
        assert_eq!(normalize_ip("::ffff:192.168.1.1"), "192.168.1.1");
        assert_eq!(normalize_ip("::ffff:7f00:1"), "127.0.0.1");
    }

    #[test]
    fn normalize_ip_standard_ipv6() {
        assert_eq!(normalize_ip("::1"), "::1");
        assert_eq!(
            normalize_ip("0000:0000:0000:0000:0000:0000:0000:0001"),
            "::1"
        );
    }

    #[test]
    fn normalize_ip_non_ip_passthrough() {
        assert_eq!(normalize_ip("localhost"), "localhost");
        assert_eq!(normalize_ip("example.com"), "example.com");
    }

    // --- Unix path normalization tests ---

    #[test]
    fn normalize_unix_path_double_slash() {
        assert_eq!(normalize_unix_path("/tmp//aterm.sock"), "/tmp/aterm.sock");
        assert_eq!(normalize_unix_path("//tmp///aterm.sock"), "/tmp/aterm.sock");
    }

    #[test]
    fn normalize_unix_path_dot_segments() {
        assert_eq!(normalize_unix_path("/tmp/./aterm.sock"), "/tmp/aterm.sock");
        assert_eq!(
            normalize_unix_path("/tmp/./././aterm.sock"),
            "/tmp/aterm.sock"
        );
    }

    #[test]
    fn normalize_unix_path_combined() {
        assert_eq!(
            normalize_unix_path("/tmp/./foo//aterm.sock"),
            "/tmp/foo/aterm.sock"
        );
    }

    #[test]
    fn normalize_unix_path_clean() {
        assert_eq!(normalize_unix_path("/tmp/aterm.sock"), "/tmp/aterm.sock");
    }

    // --- Network matching bypass vector tests ---

    #[test]
    fn network_bypass_octal_ip() {
        assert!(network_matches("127.0.0.1:8080", "0177.0.0.01:8080"));
        assert!(network_matches("127.0.0.1:*", "0177.0.0.01:8080"));
        assert!(network_matches("0177.0.0.01:8080", "127.0.0.1:8080"));
    }

    #[test]
    fn network_bypass_hex_ip() {
        assert!(network_matches("127.0.0.1:8080", "0x7f000001:8080"));
        assert!(network_matches("127.0.0.1:*", "0x7f000001:443"));
        assert!(network_matches("0x7f000001:8080", "127.0.0.1:8080"));
    }

    #[test]
    fn network_bypass_hex_dotted_ip() {
        assert!(network_matches("127.0.0.1:8080", "0x7f.0.0.0x01:8080"));
    }

    #[test]
    fn network_bypass_percent_encoded_host() {
        assert!(network_matches(
            "localhost:8080",
            "%6c%6f%63%61%6c%68%6f%73%74:8080"
        ));
        assert!(network_matches(
            "localhost:*",
            "%6c%6f%63%61%6c%68%6f%73%74:9090"
        ));
    }

    #[test]
    fn network_bypass_percent_encoded_in_url() {
        assert!(network_matches(
            "example.com:443",
            "https://exam%70le.com/path"
        ));
    }

    #[test]
    fn network_bypass_ipv4_mapped_ipv6() {
        assert!(network_matches("127.0.0.1:8080", "[::ffff:127.0.0.1]:8080"));
        assert!(network_matches("127.0.0.1:*", "[::ffff:127.0.0.1]:9090"));
        assert!(network_matches("[::ffff:127.0.0.1]:8080", "127.0.0.1:8080"));
    }

    #[test]
    fn network_bypass_ipv6_expanded_vs_compressed() {
        assert!(network_matches(
            "::1:8080",
            "[0000:0000:0000:0000:0000:0000:0000:0001]:8080"
        ));
    }

    #[test]
    fn network_bypass_unix_double_slash() {
        assert!(network_matches(
            "unix:/tmp/aterm.sock",
            "unix:/tmp//aterm.sock"
        ));
        assert!(network_matches(
            "unix:/tmp//aterm.sock",
            "unix:/tmp/aterm.sock"
        ));
    }

    #[test]
    fn network_bypass_unix_dot_segment() {
        assert!(network_matches(
            "unix:/tmp/aterm.sock",
            "unix:/tmp/./aterm.sock"
        ));
        assert!(network_matches(
            "unix:/tmp/./aterm.sock",
            "unix:/tmp/aterm.sock"
        ));
    }

    #[test]
    fn network_bypass_nonmatch_still_denied() {
        assert!(!network_matches("127.0.0.1:8080", "127.0.0.2:8080"));
        assert!(!network_matches("127.0.0.1:*", "0x7f000002:8080"));
        assert!(!network_matches("localhost:*", "evil.com:80"));
        assert!(!network_matches(
            "unix:/tmp/aterm.sock",
            "unix:/var/evil.sock"
        ));
    }

    // Note: is_*_allowed() functions depend on global OnceLock state
    // (MODE and ALLOWLIST), so full integration tests are in
    // tests/allowlist_integration.rs using separate test binaries.

    // --- TOCTOU documentation tests (#7591) ---

    /// Document that `canonicalize()` resolves symlinks at check time,
    /// creating a TOCTOU window before exec. The `verify_executable_fd`
    /// function closes this gap by resolving from an open fd.
    #[cfg(unix)]
    #[test]
    fn toctou_symlink_swap_between_canonicalize_and_exec() {
        use std::os::unix::fs::symlink;

        let dir = aterm_tempfile::tempdir().unwrap();
        let real_bin = dir.path().join("real_bin");
        let evil_bin = dir.path().join("evil_bin");
        let link = dir.path().join("link_bin");

        // Create two "executables"
        fs::write(&real_bin, "#!/bin/sh\necho safe").unwrap();
        fs::write(&evil_bin, "#!/bin/sh\necho pwned").unwrap();

        // Symlink initially points to the safe binary
        symlink(&real_bin, &link).unwrap();

        // canonicalize() at check time resolves to real_bin
        let check_time_path = link.canonicalize().unwrap();
        assert_eq!(check_time_path, real_bin.canonicalize().unwrap());

        // --- TOCTOU window: attacker swaps the symlink ---
        fs::remove_file(&link).unwrap();
        symlink(&evil_bin, &link).unwrap();

        // At exec time, the symlink now points to evil_bin
        let exec_time_path = link.canonicalize().unwrap();
        assert_eq!(exec_time_path, evil_bin.canonicalize().unwrap());

        // The two paths differ -- this IS the TOCTOU bug.
        assert_ne!(
            check_time_path, exec_time_path,
            "TOCTOU: path changed between check and exec"
        );

        // The fix: verify_executable_fd resolves from the fd, not the path.
        // (Full integration of fd-based verification requires the exec
        //  callsite to open + verify, which is tested in the PTY layer.)
    }

    /// Verify that `fd_to_path` correctly resolves an open fd back to
    /// its filesystem path, which is the foundation of the TOCTOU fix.
    #[cfg(unix)]
    #[test]
    fn fd_to_path_resolves_open_file() {
        use std::os::unix::io::AsRawFd;

        let dir = aterm_tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test_exec");
        fs::write(&file_path, "#!/bin/sh").unwrap();

        let file = fs::File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();

        let resolved = super::fd_to_path(fd);
        assert!(
            resolved.is_some(),
            "fd_to_path should resolve an open file descriptor"
        );
        let resolved = resolved.unwrap();
        assert_eq!(
            resolved,
            file_path.canonicalize().unwrap(),
            "resolved path should match canonical path"
        );
    }

    /// Verify that `fd_to_path` detects symlink swap after open: if we
    /// open the real file and then swap the symlink, the fd still
    /// resolves to the original (real) file.
    #[cfg(unix)]
    #[test]
    fn fd_to_path_stable_after_symlink_swap() {
        use std::os::unix::fs::symlink;
        use std::os::unix::io::AsRawFd;

        let dir = aterm_tempfile::tempdir().unwrap();
        let real_bin = dir.path().join("real");
        let evil_bin = dir.path().join("evil");
        let link = dir.path().join("cmd");

        fs::write(&real_bin, "safe").unwrap();
        fs::write(&evil_bin, "evil").unwrap();
        symlink(&real_bin, &link).unwrap();

        // Open via the symlink -- fd points to real_bin
        let file = fs::File::open(&link).unwrap();
        let fd = file.as_raw_fd();

        // Swap the symlink to evil_bin
        fs::remove_file(&link).unwrap();
        symlink(&evil_bin, &link).unwrap();

        // fd_to_path still resolves to the original real_bin
        let resolved = super::fd_to_path(fd).unwrap();
        assert_eq!(
            resolved,
            real_bin.canonicalize().unwrap(),
            "fd should still point to original file after symlink swap"
        );
    }
}
