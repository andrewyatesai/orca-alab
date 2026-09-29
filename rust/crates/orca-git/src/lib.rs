//! `orca-git` — git operations for Orca.
//!
//! Logic is generic over the [`runner::GitRunner`] boundary so it runs against
//! local worktrees, SSH worktrees, or a mock in tests. Modules are faithful
//! ports of `src/main/git/*`, each carrying its original test cases.

// Trust contracts: `trustc` sets `cfg(trust_verify)` itself whenever verification
// runs (`targo trust`); the cfg is off under `targo --unverified` and under the
// stock wasm32 lane (orca-git-wasm), where this gating keeps the crate building.
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
pub mod line_count;
pub mod numstat;
pub mod status;
pub mod status_parse;
pub mod status_result;
pub mod status_stream;
pub mod upstream;
pub mod worktree;


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod git_history_types;
pub mod git_history_log_parser;
pub mod git_history_graph;
pub mod git_history_boundary_rows;
pub mod git_history;
pub mod source_control_ai;
