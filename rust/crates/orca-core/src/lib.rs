//! `orca-core` — pure cross-cutting logic for Orca.
//!
//! Each module here is a faithful port of a `src/shared/*` module that contains
//! no IO, no Electron, and no platform calls. The original TypeScript test cases
//! are translated verbatim so behavioural fidelity is verifiable with
//! `targo --unverified test -p orca-core`. Anything that touches the filesystem,
//! network, processes, or an OS API lives in a higher tier crate, not here.
//!
//! Written verifier-friendly for Trust (`#![forbid(unsafe_code)]`, panic-free):
//! the pure-logic surface is the first target for `targo trust check -p orca-core`.

// Trust contracts: `trustc` sets `cfg(trust_verify)` itself whenever verification
// runs (`targo trust`); the cfg is off under `targo --unverified` and under the
// stock wasm32 lane (orca-git-wasm), where this gating keeps the crate building.
#![cfg_attr(trust_verify, feature(register_tool))]
#![cfg_attr(trust_verify, register_tool(trust))]

/// Ceiling on a `with_capacity` hint derived from untrusted input length.
/// Capacity is only a hint, so clamping it cannot change any result — it caps a
/// pre-allocation an attacker-sized string would otherwise dictate, and gives
/// Trust the dominating check its `unbounded_allocation` obligation asks for.
/// 1 MiB is far above any path/URL/id this crate handles.
pub(crate) const MAX_PREALLOC_HINT: usize = 1 << 20;

pub mod agent_kind;
pub mod agent_notification_id;
pub mod agent_recognition;
pub mod agent_scratch_worktrees;
pub mod base_ref_search_result;
pub mod branch_name_from_work;
pub mod browser_search;
pub mod commit_message_host_key;
pub mod cross_platform_path;
pub mod execution_host;
pub mod external_worktree_inbox;
pub mod feature_wall_tour_depth;
pub mod fleet_exceptions;
pub mod git_cquoted_path;
pub mod git_push_target;
pub mod gitlab_pipeline_checks;
pub mod gitlab_projects;
pub mod git_upstream_status;
pub mod hook_command_source_policy;
pub mod hosted_remote_url;
pub mod hosted_review_refs;
pub mod js_string;
pub mod linear_links;
pub mod github_pr_merge_methods;
pub mod marine_creatures;
pub mod native_file_drop;
pub mod nested_repo_telemetry;
pub mod open_in_applications;
pub mod opencode_terminal_title;
pub mod protocol_compat;
pub mod protocol_version;
pub mod pty_env;
pub mod quick_open_filter;
pub mod repo_badge_color;
pub mod setup_runner_command;
pub mod setup_script_telemetry;
pub mod stable_pane_id;
pub mod synthetic_agent_title;
pub mod tab_title_resolution;
pub mod tailnet_address;
pub mod task_providers;
pub mod task_query;
pub mod terminal_fonts;
pub mod terminal_surface_id;
pub mod terminal_tab_id;
pub mod terminal_title_agent_type;
pub mod unicode_nfc;
mod unicode_nfc_data;
pub mod uri_component;
pub mod workspace_cleanup;
pub mod worktree_base_ref;
pub mod worktree_id;
pub mod worktree_ownership;
pub mod wsl_paths;


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod browser_grab_types;
pub mod browser_viewport_presets;
