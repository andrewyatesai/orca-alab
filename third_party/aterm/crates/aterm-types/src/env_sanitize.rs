// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Environment variable sanitization for PTY spawn paths.
//!
//! AI development tools (AI Assistant, Copilot, Cursor, etc.) set env vars
//! that are meaningless — and potentially confusing — inside user terminal
//! sessions. This module contains the canonical deny-prefix list used by
//! all PTY spawn paths (Swift, Rust aterm-pty, aterm-core, Alacritty bridge),
//! and public callers reach it through `aterm_types::domain`.
//!
//! Part of #5400.

/// Prefixes for environment variables that should not leak into child shells.
///
/// All PTY spawn paths must filter these before exec.
pub const ENV_DENY_PREFIXES: &[&str] = &[
    "CLAUDE",     // CLAUDECODE, CLAUDE_CODE_*, CLAUDE_*
    "ANTHROPIC_", // ANTHROPIC_MODEL, ANTHROPIC_API_KEY, etc.
    "COPILOT_",   // GitHub Copilot
    "CODEX_",     // OpenAI Codex
    "CURSOR_",    // Cursor editor
    "AI_",        // AI development tool infrastructure vars
    "_DEVTOOL_",  // Internal development tool runtime vars
];

/// Exact env vars that should not leak into child shells.
///
/// These are denied by exact name because other `ATERM_*` variables are
/// required for shell integration inside the child shell.
pub const ENV_DENY_VARS: &[&str] = &["ATERM_CONTAINMENT_MODE", "ATERM_CONTAINMENT_ALLOWLIST"];

/// Returns `true` if `key` matches a deny-listed AI or containment env var.
#[must_use]
pub fn is_ai_env_var(key: &str) -> bool {
    ENV_DENY_VARS.contains(&key)
        || ENV_DENY_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ai_env_var_matches_deny_prefixes() {
        assert!(is_ai_env_var("CLAUDECODE"));
        assert!(is_ai_env_var("CLAUDE_CODE_ENTRYPOINT"));
        assert!(is_ai_env_var("CLAUDE_API_KEY"));
        assert!(is_ai_env_var("ANTHROPIC_MODEL"));
        assert!(is_ai_env_var("COPILOT_TOKEN"));
        assert!(is_ai_env_var("CODEX_SESSION"));
        assert!(is_ai_env_var("CURSOR_SETTINGS"));
        let ai_role = ["AI", "ROLE"].join("_");
        let ai_worker_id = ["AI", "WORKER", "ID"].join("_");
        assert!(is_ai_env_var(&ai_role));
        assert!(is_ai_env_var(&ai_worker_id));
        assert!(is_ai_env_var("_DEVTOOL_CARGO_LOCK"));
    }

    #[test]
    fn test_is_ai_env_var_strips_containment_vars_but_preserves_shell_integration_vars() {
        assert!(is_ai_env_var("ATERM_CONTAINMENT_MODE"));
        assert!(is_ai_env_var("ATERM_CONTAINMENT_ALLOWLIST"));
        assert!(!is_ai_env_var("ATERM_SHELL_INTEGRATION_DIR"));
        assert!(!is_ai_env_var("ATERM_ORIGINAL_ZDOTDIR"));
        assert!(!is_ai_env_var("ATERM_UNSET_ZDOTDIR"));
    }

    /// OSC 133/633 capability nonce (#7937 F01-2, #7960, #8006) must survive
    /// environment sanitization so the shell-integration preamble can emit
    /// `id=<hex>` on every 133/633 sequence.
    #[test]
    fn test_aterm_shell_nonce_survives_sanitization() {
        assert!(!is_ai_env_var("ATERM_SHELL_NONCE"));
    }

    #[test]
    fn test_is_ai_env_var_preserves_standard_vars() {
        assert!(!is_ai_env_var("PATH"));
        assert!(!is_ai_env_var("HOME"));
        assert!(!is_ai_env_var("USER"));
        assert!(!is_ai_env_var("SHELL"));
        assert!(!is_ai_env_var("TERM"));
        assert!(!is_ai_env_var("LANG"));
        assert!(!is_ai_env_var("EDITOR"));
        assert!(!is_ai_env_var("SSH_AUTH_SOCK"));
        assert!(!is_ai_env_var("HOMEBREW_PREFIX"));
        assert!(!is_ai_env_var("XDG_CONFIG_HOME"));
    }
}
