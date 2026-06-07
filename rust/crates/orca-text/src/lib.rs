//! `orca-text` — text/parsing logic ported from `src/shared` that needs a regex
//! engine. Still pure (no IO); separated from `orca-core` only because it pulls
//! in the (vendored, stripped) `regex` crate.

pub mod agent_tab_title;
pub mod git_remote_error;
pub mod mcp_env;
pub mod pi_agent_kind;
pub mod skill_metadata;
pub mod workspace_name;
