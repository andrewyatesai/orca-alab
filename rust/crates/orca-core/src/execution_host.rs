//! Execution-host id normalization, ported from `src/shared/execution-host.ts`.
//!
//! Currently the `normalizeExecutionHostId` / `parseExecutionHostId` slice that
//! project-group normalization consumes: canonicalize a persisted host id to
//! `local`, `ssh:<encoded>`, or `runtime:<encoded>`, or `None` when blank or a
//! malformed reference. SSH/runtime ids keep their ORIGINAL encoded suffix — the
//! TS validates decodability but returns `${prefix}${encoded}`, not re-encoded.

use crate::js_string::trim_js;
use crate::uri_component::try_decode_uri_component;

pub const LOCAL_EXECUTION_HOST_ID: &str = "local";

/// `normalizeExecutionHostId`: the canonical id, or `None` when the value is
/// blank or a malformed `ssh:`/`runtime:` reference (empty or non-decodable
/// suffix). Mirrors `parseExecutionHostId(value)?.id ?? null`.
pub fn normalize_execution_host_id(value: &str) -> Option<String> {
    // Why: JS `.trim()` (ECMAScript WhiteSpace: trims U+FEFF, keeps U+0085),
    // unlike Rust `str::trim` — mirror it via trim_js.
    let normalized = trim_js(value);
    if normalized.is_empty() {
        return None;
    }
    if normalized == LOCAL_EXECUTION_HOST_ID {
        return Some(LOCAL_EXECUTION_HOST_ID.to_string());
    }
    for prefix in ["ssh:", "runtime:"] {
        if let Some(encoded) = normalized.strip_prefix(prefix) {
            if encoded.is_empty() {
                return None;
            }
            // Validate decodability + non-empty decode (the TS `targetId ?` /
            // `environmentId ?` guard), but keep the ORIGINAL encoded suffix.
            return match try_decode_uri_component(encoded) {
                Some(decoded) if !decoded.is_empty() => Some(format!("{prefix}{encoded}")),
                _ => None,
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_local_and_trims_surrounding_whitespace() {
        assert_eq!(normalize_execution_host_id("local").as_deref(), Some("local"));
        assert_eq!(normalize_execution_host_id("  local  ").as_deref(), Some("local"));
    }

    #[test]
    fn rejects_blank_and_unknown_kinds() {
        assert_eq!(normalize_execution_host_id(""), None);
        assert_eq!(normalize_execution_host_id("   "), None);
        assert_eq!(normalize_execution_host_id("bogus"), None);
    }

    #[test]
    fn normalizes_ssh_and_runtime_keeping_the_original_encoded_suffix() {
        assert_eq!(normalize_execution_host_id("ssh:foo").as_deref(), Some("ssh:foo"));
        assert_eq!(normalize_execution_host_id("runtime:env-1").as_deref(), Some("runtime:env-1"));
        // Decodes to a non-empty string, so it survives — suffix kept as-is.
        assert_eq!(normalize_execution_host_id("ssh:foo%20bar").as_deref(), Some("ssh:foo%20bar"));
    }

    #[test]
    fn rejects_empty_or_non_decodable_suffixes() {
        assert_eq!(normalize_execution_host_id("ssh:"), None);
        assert_eq!(normalize_execution_host_id("runtime:"), None);
        // Malformed percent-escape → decodeURIComponent throws → None.
        assert_eq!(normalize_execution_host_id("ssh:%ZZ"), None);
    }

    #[test]
    fn trims_js_whitespace_before_matching() {
        // U+FEFF (BOM) is JS whitespace — trimmed so the "local" match holds.
        assert_eq!(normalize_execution_host_id("\u{FEFF}local").as_deref(), Some("local"));
    }
}
