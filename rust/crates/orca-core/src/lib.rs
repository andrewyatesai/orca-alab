//! `orca-core` — pure cross-cutting logic for Orca.
//!
//! Each module here is a faithful port of a `src/shared/*` module that contains
//! no IO, no Electron, and no platform calls. The original TypeScript test cases
//! are translated verbatim so behavioural fidelity is verifiable with
//! `cargo test`. Anything that touches the filesystem, network, processes, or an
//! OS API lives in a higher tier crate, not here.
//!
//! Written verifier-friendly for Trust (`#![forbid(unsafe_code)]`, panic-free):
//! the pure-logic surface is the first target for `tcargo trust check`.

pub mod agent_hook_endpoint_file;
pub mod agent_kind;
pub mod agent_notification_id;
pub mod agent_recognition;
pub mod base_ref_search_result;
pub mod branch_name_from_work;
pub mod browser_search;
pub mod commit_message_host_key;
pub mod cross_platform_path;
pub mod feature_wall_tour_depth;
pub mod git_cquoted_path;
pub mod git_push_target;
pub mod gitlab_pipeline_checks;
pub mod gitlab_projects;
pub mod git_upstream_status;
pub mod hook_command_source_policy;
pub mod hosted_remote_url;
pub mod hosted_review_queue;
pub mod hosted_review_refs;
pub mod linear_links;
pub mod github_pr_merge_methods;
pub mod marine_creatures;
pub mod native_file_drop;
pub mod nested_repo_telemetry;
pub mod open_in_applications;
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
pub mod uri_component;
pub mod workspace_cleanup;
pub mod worktree_base_ref;
pub mod worktree_id;
pub mod worktree_ownership;
pub mod wsl_paths;
