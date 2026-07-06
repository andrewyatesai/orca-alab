//! `orca-git-wasm` — the addon-less SSH relay's git-parsing substrate.
//!
//! The relay runs on the remote host as pure JS with NO native addon, so it
//! historically re-implemented git-output parsing in TypeScript. Those TS
//! parsers could (and did) drift from the Rust `orca-git` port that the main
//! process runs via napi. This crate compiles the SAME pure `orca-git` /
//! `orca-core` / `orca-text` functions to `wasm32-unknown-unknown`, so the relay
//! parses git output through the identical code — one source of truth.
//!
//! Scope is deliberately the PURE parsers/validators (git output in -> data out),
//! which need no git runner. Multi-round operations that must actually run git
//! (effective-upstream resolution, rebase-source, branch-cleanup) stay as async
//! orchestration in the relay's JS — wasm is single-threaded and cannot block a
//! synchronous Rust runner on the relay's async git executor — and they call
//! these parsers underneath.
//!
//! Each export mirrors the matching `native/orca-node` napi function body so the
//! relay's output is byte-identical to the main process.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use orca_text::git_remote_error::GitRemoteOperation;

/// One-shot status scan (the relay's `parseStatusOutput` replacement): the cap is
/// applied DURING the scan, so `entries` is bounded by `limit`. Returns the
/// status-parse-result JSON.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "parseStatusPorcelain"))]
pub fn parse_status_porcelain(stdout: &[u8], limit: u32) -> String {
    let result = orca_git::status_stream::parse_status_porcelain(stdout, limit as usize);
    orca_git::status_result::status_parse_result_to_json(&result).to_string()
}

/// `git diff --numstat` (text or `-z`) parsed to `{path: {added?, removed?}}` JSON.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "parseNumstat"))]
pub fn parse_numstat(stdout: &[u8]) -> String {
    let entries = orca_git::numstat::parse_numstat(stdout);
    orca_git::status_result::numstat_to_json(&entries).to_string()
}

/// `git worktree list --porcelain` (or the `-z` NUL form) parsed to the
/// `GitWorktreeInfo[]` JSON the TS `parseWorktreeList` produced.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "parseWorktreeList"))]
pub fn parse_worktree_list(output: &str, nul_delimited: bool) -> String {
    let worktrees = orca_git::worktree::parse_worktree_list(output, nul_delimited);
    orca_git::worktree::worktree_list_to_json(&worktrees).to_string()
}

/// NUL-delimited `git log` (in `GIT_HISTORY_COMMIT_FORMAT`) parsed to the
/// `GitHistoryItem[]` JSON the TS `parseGitHistoryLog` produced.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "parseGitHistoryLog"))]
pub fn parse_git_history_log(stdout: &str) -> String {
    let items = orca_git::git_history_log_parser::parse_git_history_log(stdout);
    orca_git::git_history_log_parser::git_history_log_to_json(&items).to_string()
}

/// Count additions for an untracked file's contents: `undefined` for binary, 0 for
/// empty, else the trailing-newline-aware line count.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "countAdditionsInBuffer"))]
pub fn count_additions_in_buffer(bytes: &[u8]) -> Option<u32> {
    orca_git::line_count::count_additions_in_buffer(bytes)
}

/// Validate a persisted push target's *value* rules (path-traversal safety for a
/// remote name / branch name / optional GitHub URL). Returns the TS-identical
/// error message, or `undefined` when valid. The `unknown`->typed guards (the
/// "Invalid PR push target …" messages) stay in JS.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "validateGitPushTargetRules"))]
pub fn validate_git_push_target_rules(
    remote_name: &str,
    branch_name: &str,
    remote_url: Option<String>,
) -> Option<String> {
    orca_core::git_push_target::validate_git_push_target(
        remote_name,
        branch_name,
        remote_url.as_deref(),
    )
    .err()
}

/// Decode a git C-quoted (octal-escaped) path. Raw (unquoted) input passes through.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "decodeGitCQuotedPath"))]
pub fn decode_git_cquoted_path(value: &str) -> String {
    orca_core::git_cquoted_path::decode_git_cquoted_path(value)
}

/// Normalise a git remote-operation error into a user-facing message. `message`
/// is `undefined` for a non-Error throw (returns the fixed fallback). `operation`
/// is `"push" | "pull" | "fetch" | "upstream"` (or `undefined`); an unrecognised
/// value maps to `None`, matching the TS default-parameter behaviour.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "normalizeGitErrorMessage"))]
pub fn normalize_git_error_message(message: Option<String>, operation: Option<String>) -> String {
    let operation = match operation.as_deref() {
        Some("push") => Some(GitRemoteOperation::Push),
        Some("pull") => Some(GitRemoteOperation::Pull),
        Some("fetch") => Some(GitRemoteOperation::Fetch),
        Some("upstream") => Some(GitRemoteOperation::Upstream),
        _ => None,
    };
    orca_text::git_remote_error::normalize_git_error_message(message.as_deref(), operation)
}

/// Scrub credentials embedded in a git URL within `message` (keeps SSH user-info;
/// strips `user:password@` on any scheme + HTTP(S) token-only `user@`).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "stripCredentialsFromMessage"))]
pub fn strip_credentials_from_message(message: &str) -> String {
    orca_text::git_remote_error::strip_credentials_from_message(message)
}

/// True only for clearly-no-upstream signals (an expected state, gated on a
/// `fatal:` prefix). `undefined` message -> false (a non-Error throw in TS).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "isNoUpstreamError"))]
pub fn is_no_upstream_error(message: Option<String>) -> bool {
    orca_text::git_remote_error::is_no_upstream_error(message.as_deref())
}
