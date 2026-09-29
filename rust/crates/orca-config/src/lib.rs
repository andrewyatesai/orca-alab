//! `orca-config` — configuration inspection/parsing for Orca.
//!
//! JSON-backed config handling (over vendored `serde_json`), starting with MCP
//! server config inspection ported from `src/main`/`src/shared` `mcp-config.ts`.

// Trust contracts: `trustc` sets `cfg(trust_verify)` itself whenever verification
// runs (`targo trust`); the cfg is off under `targo --unverified` and under the
// stock wasm32 lane (orca-git-wasm), where this gating keeps the crate building.
#![cfg_attr(trust_verify, feature(register_tool))]
#![cfg_attr(trust_verify, register_tool(trust))]

pub mod feature_interaction_usage_buckets;
pub mod feature_interactions;
pub mod js_value_string;
pub mod json_lone_surrogate_escapes;
pub mod mcp;
pub mod project_groups;
pub mod repo_icon;
pub mod setup_script_package_manager;
pub mod workspace_statuses;

pub use feature_interaction_usage_buckets::{
    FeatureInteractionUsageBucket, FEATURE_INTERACTION_USAGE_BUCKETS,
};
pub use feature_interactions::{
    has_feature_interaction, is_feature_interaction_id,
    normalize_feature_interaction_telemetry_buckets, normalize_feature_interactions,
    FeatureInteractionDefinition, FeatureInteractionId, FeatureInteractionRecord,
    FeatureInteractionState, FeatureInteractionTelemetryBucketState, FEATURE_INTERACTIONS,
};
pub use js_value_string::{js_number_string, js_string};
pub use json_lone_surrogate_escapes::{
    parse_json_past_lone_surrogate_escapes, replace_lone_surrogate_escapes,
};
pub use mcp::{
    inspect_mcp_config_content, McpConfigInspection, McpServerStatus, McpServerSummary,
    McpServerTransport,
};
pub use repo_icon::{
    favicon_url_from_website, github_avatar_icon, sanitize_repo_icon, RepoIcon, RepoIconImageSource,
    RepoIconSanitizeResult, MAX_REPO_ICON_DATA_URL_LENGTH, MAX_REPO_ICON_UPLOAD_BYTES,
};
pub use setup_script_package_manager::{
    inspect_package_manager_setup_candidate, SetupScriptImportCandidate,
};


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod workspace_session_schema;
pub mod workspace_session_terminal_buffers;
pub mod feature_tips;
pub mod contextual_tours;
pub mod feature_education_telemetry;
pub mod setup_script_import_codex_environment;
pub mod setup_script_import_limits;
pub mod setup_script_imports;

pub use workspace_session_schema::{parse_workspace_session, ParsedWorkspaceSession, MAX_BROWSER_HISTORY_ENTRIES};
pub use workspace_session_terminal_buffers::{cap_terminal_scrollback_session_buffer, prune_local_terminal_scrollback_buffers, should_preserve_terminal_scrollback_buffers, RepoConnection, FLOATING_TERMINAL_WORKTREE_ID, TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT};
pub use feature_tips::{
    get_completed_feature_tip_ids, get_ordered_unseen_feature_tips, is_feature_tip_id,
    normalize_feature_tip_ids, CompletedFeatureTipState, FeatureTip, FeatureTipAction,
    FeatureTipId, FeatureTipPriority, FEATURE_TIPS,
};
pub use contextual_tours::{
    get_contextual_tour, is_contextual_tour_id, normalize_contextual_tour_ids, ContextualTour,
    ContextualTourId, ContextualTourStep, ContextualTourStepAction, ContextualTourStepActionKind,
    ContextualTourStepControl, ContextualTourStepControlKind, ContextualTourStepPlacement,
    CONTEXTUAL_TOURS, CONTEXTUAL_TOUR_IDS,
};
pub use feature_education_telemetry::{
    normalize_feature_education_source, normalize_setup_guide_source, CONTEXTUAL_TOUR_OUTCOMES,
    FEATURE_EDUCATION_CONTEXTUAL_TOUR_IDS, FEATURE_EDUCATION_SOURCES, SETUP_GUIDE_CLOSE_OUTCOMES,
    SETUP_GUIDE_SOURCES, TERMINAL_PANE_SPLIT_SOURCES,
};
pub use setup_script_imports::inspect_setup_script_import_candidates;
