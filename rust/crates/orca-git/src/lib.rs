//! `orca-git` — git operations for Orca.
//!
//! Logic is generic over the [`runner::GitRunner`] boundary so it runs against
//! local worktrees, SSH worktrees, or a mock in tests. Modules are faithful
//! ports of `src/main/git/*`, each carrying its original test cases.

// Trust contracts: the `trust` tool namespace only exists under `trustc`
// (`--cfg trust_verify`); inert under stock cargo so the crate stays
// dual-buildable.
#![cfg_attr(trust_verify, feature(register_tool))]
#![cfg_attr(trust_verify, register_tool(trust))]

pub mod branch_cleanup;
pub mod branch_rename;
pub mod check_ignored_paths;
pub mod effective_upstream;
pub mod fetch_error_classification;
pub mod publish_target_status;
pub mod push_target;
pub mod rebase_source;
pub mod repo_clone_path;
pub mod remote;
pub mod runner;
pub mod status;
pub mod status_parse;
pub mod upstream;
pub mod worktree;


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod git_history_types;
pub mod git_history_log_parser;
pub mod git_history_graph;
pub mod git_history_boundary_rows;
pub mod git_history;
pub mod source_control_ai;
