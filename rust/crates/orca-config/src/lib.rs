//! `orca-config` — configuration inspection/parsing for Orca.
//!
//! JSON-backed config handling (over vendored `serde_json`), starting with MCP
//! server config inspection ported from `src/main`/`src/shared` `mcp-config.ts`.

pub mod feature_interactions;
pub mod mcp;
pub mod pi_overlay_ui_settings;
pub mod project_groups;
pub mod repo_icon;
pub mod setup_script_package_manager;
pub mod workspace_statuses;

pub use feature_interactions::{
    has_feature_interaction, is_feature_interaction_id, normalize_feature_interactions,
    FeatureInteractionDefinition, FeatureInteractionId, FeatureInteractionRecord,
    FeatureInteractionState, FEATURE_INTERACTIONS,
};
pub use mcp::{
    inspect_mcp_config_content, McpConfigInspection, McpServerStatus, McpServerSummary,
    McpServerTransport,
};
pub use pi_overlay_ui_settings::merge_pi_overlay_ui_settings;
pub use repo_icon::{
    favicon_url_from_website, github_avatar_icon, sanitize_repo_icon, RepoIcon, RepoIconImageSource,
    RepoIconSanitizeResult, MAX_REPO_ICON_DATA_URL_LENGTH, MAX_REPO_ICON_UPLOAD_BYTES,
};
pub use setup_script_package_manager::{
    inspect_package_manager_setup_candidate, SetupScriptImportCandidate,
};
