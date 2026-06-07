//! Source Control AI settings: defaults, legacy migration/merge, defensive
//! normalization, host-scoped model selection, and per-operation precedence —
//! ported from `src/shared/source-control-ai.ts` (+ the type shapes from
//! `src/shared/source-control-ai-types.ts`).
//!
//! The defaults, migration-compatibility, and operation-resolution rules live
//! together so the commit-message / pull-request / branch-name precedence cannot
//! drift across the global, repo, local, and SSH paths. Untrusted persisted
//! blobs arrive as `serde_json::Value` and are normalized into typed structs;
//! the agent/model catalog comes from `orca_agents`. The JS proto-pollution
//! guards (`__proto__`/`constructor`/`prototype`) are not memory-safety issues
//! in Rust, but the *key filtering* they imply is observable and preserved.

use orca_agents::{
    get_commit_message_agent_spec, get_commit_message_model, is_custom_agent_id,
    resolve_commit_message_agent_choice, CommitMessageModel, CUSTOM_AGENT_ID,
};
use orca_core::commit_message_host_key::LOCAL_COMMIT_MESSAGE_HOST_KEY;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// A model/thinking capability cached from a discovery probe. Same shape as a
/// catalog model, so resolution can unify across spec/discovered/derived models.
pub type CommitMessageAiModelCapability = CommitMessageModel;

/// The three Source Control AI generation operations, in their canonical order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceControlAiOperation {
    CommitMessage,
    PullRequest,
    BranchName,
}

impl SourceControlAiOperation {
    pub const ALL: [SourceControlAiOperation; 3] = [
        SourceControlAiOperation::CommitMessage,
        SourceControlAiOperation::PullRequest,
        SourceControlAiOperation::BranchName,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceControlAiOperation::CommitMessage => "commitMessage",
            SourceControlAiOperation::PullRequest => "pullRequest",
            SourceControlAiOperation::BranchName => "branchName",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SourceControlAiOperation::CommitMessage => "commit messages",
            SourceControlAiOperation::PullRequest => "pull request details",
            SourceControlAiOperation::BranchName => "branch names",
        }
    }
}

/// Per-operation model/thinking override (`SourceControlAiModelChoice`). A field
/// is `None` when absent, matching the optional records in the TS shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceControlAiModelChoice {
    pub selected_model_by_agent: Option<BTreeMap<String, String>>,
    pub selected_model_by_agent_by_host: Option<BTreeMap<String, BTreeMap<String, String>>>,
    pub selected_thinking_by_model: Option<BTreeMap<String, String>>,
}

impl SourceControlAiModelChoice {
    fn is_empty(&self) -> bool {
        self.selected_model_by_agent.is_none()
            && self.selected_model_by_agent_by_host.is_none()
            && self.selected_thinking_by_model.is_none()
    }
}

/// Global PR-creation defaults; each field absent (`None`) inherits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceControlAiPrCreationDefaults {
    pub draft: Option<bool>,
    pub use_template: Option<bool>,
    pub generate_details_on_open: Option<bool>,
    pub open_after_create: Option<bool>,
}

/// Fully-resolved PR-creation defaults (the TS `Required<…>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPrCreationDefaults {
    pub draft: bool,
    pub use_template: bool,
    pub generate_details_on_open: bool,
    pub open_after_create: bool,
}

const DEFAULT_PR_CREATION_DEFAULTS: ResolvedPrCreationDefaults = ResolvedPrCreationDefaults {
    draft: false,
    use_template: false,
    generate_details_on_open: false,
    open_after_create: false,
};

/// Legacy commit-message-only settings (`CommitMessageAiSettings`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitMessageAiSettings {
    pub enabled: bool,
    /// A TuiAgent id, the literal `"custom"`, or `None` (≙ TS `null`).
    pub agent_id: Option<String>,
    pub selected_model_by_agent: BTreeMap<String, String>,
    pub selected_model_by_agent_by_host: Option<BTreeMap<String, BTreeMap<String, String>>>,
    pub discovered_models_by_agent: Option<BTreeMap<String, Vec<CommitMessageAiModelCapability>>>,
    pub discovered_models_by_agent_by_host:
        Option<BTreeMap<String, BTreeMap<String, Vec<CommitMessageAiModelCapability>>>>,
    pub selected_thinking_by_model: BTreeMap<String, String>,
    pub custom_prompt: String,
    pub custom_agent_command: String,
}

/// The split Source Control AI settings (`SourceControlAiSettings`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceControlAiSettings {
    pub enabled: bool,
    pub agent_id: Option<String>,
    pub selected_model_by_agent: BTreeMap<String, String>,
    pub selected_model_by_agent_by_host: Option<BTreeMap<String, BTreeMap<String, String>>>,
    pub discovered_models_by_agent: Option<BTreeMap<String, Vec<CommitMessageAiModelCapability>>>,
    pub discovered_models_by_agent_by_host:
        Option<BTreeMap<String, BTreeMap<String, Vec<CommitMessageAiModelCapability>>>>,
    pub selected_thinking_by_model: BTreeMap<String, String>,
    pub custom_agent_command: String,
    pub instructions_by_operation: BTreeMap<SourceControlAiOperation, String>,
    pub model_overrides_by_operation:
        Option<BTreeMap<SourceControlAiOperation, SourceControlAiModelChoice>>,
    pub pr_creation_defaults: Option<SourceControlAiPrCreationDefaults>,
}

/// Repo-level tri-state PR-creation override: `None` outer = absent (inherit),
/// `Some(None)` = explicit null (inherit), `Some(Some(b))` = explicit boolean.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoPrCreationDefaults {
    pub draft: Option<Option<bool>>,
    pub use_template: Option<Option<bool>>,
    pub generate_details_on_open: Option<Option<bool>>,
    pub open_after_create: Option<Option<bool>>,
}

/// Repo-scoped overrides (`RepoSourceControlAiOverrides`). Instructions are
/// `Option<String>` (`Some` = string replacement, `None` = explicit null).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoSourceControlAiOverrides {
    pub model_overrides_by_operation:
        Option<BTreeMap<SourceControlAiOperation, SourceControlAiModelChoice>>,
    pub instructions_by_operation: Option<BTreeMap<SourceControlAiOperation, Option<String>>>,
    pub pr_creation_defaults: Option<RepoPrCreationDefaults>,
}

// ---------------------------------------------------------------------------
// Defensive normalization over untrusted `serde_json::Value`.
// ---------------------------------------------------------------------------

static NULL: Value = Value::Null;

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&NULL)
}

/// Drop empty and prototype-chain keys. Not a memory-safety guard in Rust, but
/// the resulting key filtering is observable, so it is preserved.
fn is_safe_record_key(key: &str) -> bool {
    !key.is_empty() && key != "__proto__" && key != "constructor" && key != "prototype"
}

fn normalize_string_record(value: &Value) -> Option<BTreeMap<String, String>> {
    let obj = value.as_object()?;
    let mut normalized = BTreeMap::new();
    for (key, item) in obj {
        if is_safe_record_key(key) {
            if let Some(text) = item.as_str() {
                normalized.insert(key.clone(), text.to_string());
            }
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_host_agent_model_record(
    value: &Value,
) -> Option<BTreeMap<String, BTreeMap<String, String>>> {
    let obj = value.as_object()?;
    let mut normalized = BTreeMap::new();
    for (host_key, host_models) in obj {
        if !is_safe_record_key(host_key) {
            continue;
        }
        if let Some(models) = normalize_string_record(host_models) {
            normalized.insert(host_key.clone(), models);
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_source_control_ai_model_choice(value: &Value) -> Option<SourceControlAiModelChoice> {
    if !value.is_object() {
        return None;
    }
    let choice = SourceControlAiModelChoice {
        selected_model_by_agent: normalize_string_record(get(value, "selectedModelByAgent")),
        selected_model_by_agent_by_host: normalize_host_agent_model_record(get(
            value,
            "selectedModelByAgentByHost",
        )),
        selected_thinking_by_model: normalize_string_record(get(value, "selectedThinkingByModel")),
    };
    if choice.is_empty() {
        None
    } else {
        Some(choice)
    }
}

fn normalize_operation_record<T>(
    value: &Value,
    normalize_value: fn(&Value) -> Option<T>,
) -> Option<BTreeMap<SourceControlAiOperation, T>> {
    let obj = value.as_object()?;
    let mut normalized = BTreeMap::new();
    for operation in SourceControlAiOperation::ALL {
        // `Object.prototype.hasOwnProperty.call(value, operation)`: only own keys.
        if let Some(item) = obj.get(operation.as_str()) {
            if let Some(parsed) = normalize_value(item) {
                normalized.insert(operation, parsed);
            }
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// `string | null | undefined`: a present-but-defined value (string or null) is
/// kept (outer `Some`); anything else is dropped (outer `None`).
fn normalize_repo_instruction(value: &Value) -> Option<Option<String>> {
    if let Some(text) = value.as_str() {
        Some(Some(text.to_string()))
    } else if value.is_null() {
        Some(None)
    } else {
        None
    }
}

fn parse_bool_or_null(value: &Value) -> Option<Option<bool>> {
    if let Some(flag) = value.as_bool() {
        Some(Some(flag))
    } else if value.is_null() {
        Some(None)
    } else {
        None
    }
}

fn normalize_repo_pr_creation_defaults(value: &Value) -> Option<RepoPrCreationDefaults> {
    let obj = value.as_object()?;
    let mut normalized = RepoPrCreationDefaults::default();
    let mut any = false;
    if let Some(item) = obj.get("draft").and_then(parse_bool_or_null) {
        normalized.draft = Some(item);
        any = true;
    }
    if let Some(item) = obj.get("useTemplate").and_then(parse_bool_or_null) {
        normalized.use_template = Some(item);
        any = true;
    }
    if let Some(item) = obj.get("generateDetailsOnOpen").and_then(parse_bool_or_null) {
        normalized.generate_details_on_open = Some(item);
        any = true;
    }
    if let Some(item) = obj.get("openAfterCreate").and_then(parse_bool_or_null) {
        normalized.open_after_create = Some(item);
        any = true;
    }
    if any {
        Some(normalized)
    } else {
        None
    }
}

pub fn normalize_repo_source_control_ai_overrides(
    value: &Value,
) -> Option<RepoSourceControlAiOverrides> {
    if !value.is_object() {
        return None;
    }
    Some(RepoSourceControlAiOverrides {
        model_overrides_by_operation: normalize_operation_record(
            get(value, "modelOverridesByOperation"),
            normalize_source_control_ai_model_choice,
        ),
        instructions_by_operation: normalize_operation_record(
            get(value, "instructionsByOperation"),
            normalize_repo_instruction,
        ),
        pr_creation_defaults: normalize_repo_pr_creation_defaults(get(value, "prCreationDefaults")),
    })
}

// ---------------------------------------------------------------------------
// Defaults + legacy migration.
// ---------------------------------------------------------------------------

pub fn get_default_source_control_ai_settings() -> SourceControlAiSettings {
    let mut instructions = BTreeMap::new();
    instructions.insert(SourceControlAiOperation::CommitMessage, String::new());
    instructions.insert(SourceControlAiOperation::PullRequest, String::new());
    instructions.insert(SourceControlAiOperation::BranchName, String::new());
    SourceControlAiSettings {
        enabled: true,
        agent_id: None,
        selected_model_by_agent: BTreeMap::new(),
        selected_model_by_agent_by_host: Some(BTreeMap::new()),
        discovered_models_by_agent: Some(BTreeMap::new()),
        discovered_models_by_agent_by_host: Some(BTreeMap::new()),
        selected_thinking_by_model: BTreeMap::new(),
        custom_agent_command: String::new(),
        instructions_by_operation: instructions,
        model_overrides_by_operation: None,
        pr_creation_defaults: Some(SourceControlAiPrCreationDefaults {
            draft: Some(false),
            use_template: Some(false),
            generate_details_on_open: Some(false),
            open_after_create: Some(false),
        }),
    }
}

pub fn source_control_ai_settings_from_legacy(
    legacy: Option<&CommitMessageAiSettings>,
) -> SourceControlAiSettings {
    let defaults = get_default_source_control_ai_settings();
    let legacy = match legacy {
        Some(legacy) => legacy,
        None => return defaults,
    };
    let mut instructions = BTreeMap::new();
    // Why: the legacy prompt covered commit generation and branch auto-rename;
    // the first split must preserve that guidance for both released paths.
    instructions.insert(
        SourceControlAiOperation::CommitMessage,
        legacy.custom_prompt.clone(),
    );
    instructions.insert(SourceControlAiOperation::PullRequest, String::new());
    instructions.insert(
        SourceControlAiOperation::BranchName,
        legacy.custom_prompt.clone(),
    );
    SourceControlAiSettings {
        enabled: legacy.enabled,
        agent_id: legacy.agent_id.clone(),
        selected_model_by_agent: legacy.selected_model_by_agent.clone(),
        selected_model_by_agent_by_host: Some(
            legacy.selected_model_by_agent_by_host.clone().unwrap_or_default(),
        ),
        discovered_models_by_agent: Some(
            legacy.discovered_models_by_agent.clone().unwrap_or_default(),
        ),
        discovered_models_by_agent_by_host: Some(
            legacy.discovered_models_by_agent_by_host.clone().unwrap_or_default(),
        ),
        selected_thinking_by_model: legacy.selected_thinking_by_model.clone(),
        custom_agent_command: legacy.custom_agent_command.clone(),
        instructions_by_operation: instructions,
        model_overrides_by_operation: defaults.model_overrides_by_operation,
        pr_creation_defaults: defaults.pr_creation_defaults,
    }
}

fn merge_selected_model_by_agent_by_host(
    base: Option<&BTreeMap<String, BTreeMap<String, String>>>,
    override_: Option<&BTreeMap<String, BTreeMap<String, String>>>,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut merged = base.cloned().unwrap_or_default();
    if let Some(override_) = override_ {
        for (host_key, host_models) in override_ {
            let entry = merged.entry(host_key.clone()).or_default();
            for (agent, model) in host_models {
                entry.insert(agent.clone(), model.clone());
            }
        }
    }
    merged
}

/// Returns the merged record plus whether the delta changed anything. The
/// boolean replaces the TS reference-identity check (`result !== existing`),
/// which is exactly "did this delta produce a different object".
fn merge_legacy_model_selection_delta(
    existing: Option<&BTreeMap<String, String>>,
    legacy: Option<&BTreeMap<String, String>>,
    projected: Option<&BTreeMap<String, String>>,
) -> (Option<BTreeMap<String, String>>, bool) {
    let empty = BTreeMap::new();
    let legacy = legacy.unwrap_or(&empty);
    let projected = projected.unwrap_or(&empty);
    let mut merged = existing.cloned().unwrap_or_default();
    let mut changed = false;
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    keys.extend(legacy.keys());
    keys.extend(projected.keys());
    for key in keys {
        let legacy_value = legacy.get(key);
        // `JSON.stringify(projected[key]) === JSON.stringify(legacyValue)` over
        // string values is plain (UTF-8) equality, with `None == None`.
        if projected.get(key) == legacy_value {
            continue;
        }
        changed = true;
        match legacy_value {
            Some(value) => {
                merged.insert(key.clone(), value.clone());
            }
            None => {
                merged.remove(key);
            }
        }
    }
    if changed {
        (Some(merged), true)
    } else {
        (existing.cloned(), false)
    }
}

/// host id -> (operation -> model id): the SC-AI per-host model overrides.
type HostModelSelections = BTreeMap<String, BTreeMap<String, String>>;

fn merge_legacy_host_model_selection_delta(
    existing: Option<&HostModelSelections>,
    legacy: Option<&HostModelSelections>,
    projected: Option<&HostModelSelections>,
) -> (Option<HostModelSelections>, bool) {
    let empty = BTreeMap::new();
    let legacy = legacy.unwrap_or(&empty);
    let projected = projected.unwrap_or(&empty);
    let mut merged = existing.cloned().unwrap_or_default();
    let mut changed = false;
    let mut host_keys: BTreeSet<&String> = BTreeSet::new();
    host_keys.extend(legacy.keys());
    host_keys.extend(projected.keys());
    for host_key in host_keys {
        let current = merged.get(host_key).cloned();
        let (next_host_models, inner_changed) = merge_legacy_model_selection_delta(
            current.as_ref(),
            legacy.get(host_key),
            projected.get(host_key),
        );
        if inner_changed {
            changed = true;
        }
        match next_host_models {
            Some(models) if !models.is_empty() => {
                merged.insert(host_key.clone(), models);
            }
            _ => {
                merged.remove(host_key);
            }
        }
    }
    if changed {
        (Some(merged), true)
    } else {
        (existing.cloned(), false)
    }
}

fn has_entries<K, V>(map: Option<&BTreeMap<K, V>>) -> bool {
    matches!(map, Some(map) if !map.is_empty())
}

pub struct MergeLegacyOptions {
    pub pull_request_instructions_from_legacy: bool,
}

pub fn merge_legacy_commit_message_ai_into_source_control_ai(
    source_control_ai: Option<&SourceControlAiSettings>,
    legacy: Option<&CommitMessageAiSettings>,
    options: MergeLegacyOptions,
) -> SourceControlAiSettings {
    // Why: older runtimes and rollback builds still write commitMessageAi; merge
    // those writes into the new shape without wiping PR-only settings.
    let base = normalize_source_control_ai_settings(source_control_ai, legacy);
    let legacy = match legacy {
        Some(legacy) => legacy,
        None => return base,
    };

    let mut next = base.clone();
    next.enabled = legacy.enabled;
    next.agent_id = legacy.agent_id.clone();
    next.discovered_models_by_agent =
        Some(legacy.discovered_models_by_agent.clone().unwrap_or_default());
    next.discovered_models_by_agent_by_host =
        Some(legacy.discovered_models_by_agent_by_host.clone().unwrap_or_default());
    next.custom_agent_command = legacy.custom_agent_command.clone();

    if source_control_ai.is_some() {
        let existing_commit_choice = base
            .model_overrides_by_operation
            .as_ref()
            .and_then(|overrides| overrides.get(&SourceControlAiOperation::CommitMessage));
        let projected_legacy = project_source_control_ai_to_legacy_commit_message_ai(&base, None);
        let (selected_model_by_agent, changed_models) = merge_legacy_model_selection_delta(
            existing_commit_choice.and_then(|choice| choice.selected_model_by_agent.as_ref()),
            Some(&legacy.selected_model_by_agent),
            Some(&projected_legacy.selected_model_by_agent),
        );
        let (selected_model_by_agent_by_host, changed_host_models) =
            merge_legacy_host_model_selection_delta(
                existing_commit_choice
                    .and_then(|choice| choice.selected_model_by_agent_by_host.as_ref()),
                legacy.selected_model_by_agent_by_host.as_ref(),
                projected_legacy.selected_model_by_agent_by_host.as_ref(),
            );
        let (selected_thinking_by_model, changed_thinking) = merge_legacy_model_selection_delta(
            existing_commit_choice.and_then(|choice| choice.selected_thinking_by_model.as_ref()),
            Some(&legacy.selected_thinking_by_model),
            Some(&projected_legacy.selected_thinking_by_model),
        );
        let should_merge = changed_models || changed_host_models || changed_thinking;
        let mut next_overrides = base.model_overrides_by_operation.clone().unwrap_or_default();
        if should_merge {
            let mut next_choice = SourceControlAiModelChoice::default();
            if has_entries(selected_model_by_agent.as_ref()) {
                next_choice.selected_model_by_agent = selected_model_by_agent;
            }
            if has_entries(selected_model_by_agent_by_host.as_ref()) {
                next_choice.selected_model_by_agent_by_host = selected_model_by_agent_by_host;
            }
            if has_entries(selected_thinking_by_model.as_ref()) {
                next_choice.selected_thinking_by_model = selected_thinking_by_model;
            }
            if next_choice.is_empty() {
                next_overrides.remove(&SourceControlAiOperation::CommitMessage);
            } else {
                next_overrides.insert(SourceControlAiOperation::CommitMessage, next_choice);
            }
        }
        // Why: keep model choices scoped to commit-message generation so PR
        // defaults cannot drift on reload across rollback/new builds.
        apply_legacy_instructions(&mut next, &base, legacy, &options);
        next.model_overrides_by_operation = Some(next_overrides);
        return normalize_source_control_ai_settings(Some(&next), Some(legacy));
    }

    next.selected_model_by_agent = legacy.selected_model_by_agent.clone();
    next.selected_model_by_agent_by_host =
        Some(legacy.selected_model_by_agent_by_host.clone().unwrap_or_default());
    next.selected_thinking_by_model = legacy.selected_thinking_by_model.clone();
    apply_legacy_instructions(&mut next, &base, legacy, &options);
    normalize_source_control_ai_settings(Some(&next), Some(legacy))
}

fn apply_legacy_instructions(
    next: &mut SourceControlAiSettings,
    base: &SourceControlAiSettings,
    legacy: &CommitMessageAiSettings,
    options: &MergeLegacyOptions,
) {
    let mut instructions = base.instructions_by_operation.clone();
    instructions.insert(
        SourceControlAiOperation::CommitMessage,
        legacy.custom_prompt.clone(),
    );
    instructions.insert(
        SourceControlAiOperation::BranchName,
        legacy.custom_prompt.clone(),
    );
    if options.pull_request_instructions_from_legacy {
        instructions.insert(
            SourceControlAiOperation::PullRequest,
            legacy.custom_prompt.clone(),
        );
    }
    next.instructions_by_operation = instructions;
}

pub fn normalize_source_control_ai_settings(
    value: Option<&SourceControlAiSettings>,
    legacy: Option<&CommitMessageAiSettings>,
) -> SourceControlAiSettings {
    let base_owned;
    let base: &SourceControlAiSettings = match value {
        Some(value) => value,
        None => {
            base_owned = source_control_ai_settings_from_legacy(legacy);
            &base_owned
        }
    };
    let defaults = get_default_source_control_ai_settings();

    let mut selected_model_by_agent = defaults.selected_model_by_agent.clone();
    for (key, value) in &base.selected_model_by_agent {
        selected_model_by_agent.insert(key.clone(), value.clone());
    }
    let mut selected_thinking_by_model = defaults.selected_thinking_by_model.clone();
    for (key, value) in &base.selected_thinking_by_model {
        selected_thinking_by_model.insert(key.clone(), value.clone());
    }
    let mut instructions_by_operation = defaults.instructions_by_operation.clone();
    for (key, value) in &base.instructions_by_operation {
        instructions_by_operation.insert(*key, value.clone());
    }

    SourceControlAiSettings {
        enabled: base.enabled,
        agent_id: base.agent_id.clone(),
        selected_model_by_agent,
        selected_model_by_agent_by_host: base
            .selected_model_by_agent_by_host
            .clone()
            .or(defaults.selected_model_by_agent_by_host),
        discovered_models_by_agent: base
            .discovered_models_by_agent
            .clone()
            .or(defaults.discovered_models_by_agent),
        discovered_models_by_agent_by_host: base
            .discovered_models_by_agent_by_host
            .clone()
            .or(defaults.discovered_models_by_agent_by_host),
        selected_thinking_by_model,
        custom_agent_command: base.custom_agent_command.clone(),
        instructions_by_operation,
        model_overrides_by_operation: base.model_overrides_by_operation.clone(),
        pr_creation_defaults: Some(merge_pr_defaults(base.pr_creation_defaults.as_ref())),
    }
}

fn merge_pr_defaults(
    base: Option<&SourceControlAiPrCreationDefaults>,
) -> SourceControlAiPrCreationDefaults {
    let mut out = SourceControlAiPrCreationDefaults {
        draft: Some(false),
        use_template: Some(false),
        generate_details_on_open: Some(false),
        open_after_create: Some(false),
    };
    if let Some(base) = base {
        if base.draft.is_some() {
            out.draft = base.draft;
        }
        if base.use_template.is_some() {
            out.use_template = base.use_template;
        }
        if base.generate_details_on_open.is_some() {
            out.generate_details_on_open = base.generate_details_on_open;
        }
        if base.open_after_create.is_some() {
            out.open_after_create = base.open_after_create;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Host-scoped model selection.
// ---------------------------------------------------------------------------

pub fn read_source_control_ai_model_choice_for_host(
    choice: Option<&SourceControlAiModelChoice>,
    host_key: &str,
    agent_id: &str,
) -> Option<String> {
    choice
        .and_then(|choice| choice.selected_model_by_agent_by_host.as_ref())
        .and_then(|by_host| by_host.get(host_key))
        .and_then(|by_agent| by_agent.get(agent_id))
        .cloned()
        .or_else(|| {
            if host_key == LOCAL_COMMIT_MESSAGE_HOST_KEY {
                choice
                    .and_then(|choice| choice.selected_model_by_agent.as_ref())
                    .and_then(|by_agent| by_agent.get(agent_id))
                    .cloned()
            } else {
                None
            }
        })
}

pub fn select_source_control_ai_model_choice_for_host(
    choice: Option<&SourceControlAiModelChoice>,
    host_key: &str,
    agent_id: &str,
    model_id: &str,
) -> SourceControlAiModelChoice {
    let mut result = choice.cloned().unwrap_or_default();
    if host_key == LOCAL_COMMIT_MESSAGE_HOST_KEY {
        let mut by_agent = choice
            .and_then(|choice| choice.selected_model_by_agent.clone())
            .unwrap_or_default();
        by_agent.insert(agent_id.to_string(), model_id.to_string());
        result.selected_model_by_agent = Some(by_agent);
    } else {
        result.selected_model_by_agent =
            choice.and_then(|choice| choice.selected_model_by_agent.clone());
    }

    let mut by_host = choice
        .and_then(|choice| choice.selected_model_by_agent_by_host.clone())
        .unwrap_or_default();
    let mut host_selected = choice
        .and_then(|choice| choice.selected_model_by_agent_by_host.as_ref())
        .and_then(|by_host| by_host.get(host_key))
        .cloned()
        .unwrap_or_default();
    host_selected.insert(agent_id.to_string(), model_id.to_string());
    by_host.insert(host_key.to_string(), host_selected);
    result.selected_model_by_agent_by_host = Some(by_host);
    result
}

pub fn clear_source_control_ai_model_choice_for_host(
    choice: Option<&SourceControlAiModelChoice>,
    host_key: &str,
    agent_id: &str,
) -> Option<SourceControlAiModelChoice> {
    let choice = choice?;
    // Why: model choices are host-scoped; clearing one "Use global" selector
    // must not erase a different SSH/runtime host's override.
    let mut selected_model_by_agent =
        choice.selected_model_by_agent.clone().unwrap_or_default();
    if host_key == LOCAL_COMMIT_MESSAGE_HOST_KEY {
        selected_model_by_agent.remove(agent_id);
    }

    let mut selected_model_by_agent_by_host =
        choice.selected_model_by_agent_by_host.clone().unwrap_or_default();
    let mut host_models = selected_model_by_agent_by_host
        .get(host_key)
        .cloned()
        .unwrap_or_default();
    host_models.remove(agent_id);
    if host_models.is_empty() {
        selected_model_by_agent_by_host.remove(host_key);
    } else {
        selected_model_by_agent_by_host.insert(host_key.to_string(), host_models);
    }

    let mut next_choice = SourceControlAiModelChoice::default();
    if !selected_model_by_agent.is_empty() {
        next_choice.selected_model_by_agent = Some(selected_model_by_agent);
    }
    if !selected_model_by_agent_by_host.is_empty() {
        next_choice.selected_model_by_agent_by_host = Some(selected_model_by_agent_by_host);
    }
    let has_model_selection = next_choice.selected_model_by_agent.is_some()
        || next_choice.selected_model_by_agent_by_host.is_some();
    if has_model_selection {
        if let Some(thinking) = choice.selected_thinking_by_model.as_ref() {
            if !thinking.is_empty() {
                next_choice.selected_thinking_by_model = Some(thinking.clone());
            }
        }
        Some(next_choice)
    } else {
        None
    }
}

pub fn project_source_control_ai_to_legacy_commit_message_ai(
    source_control_ai: &SourceControlAiSettings,
    previous_legacy: Option<&CommitMessageAiSettings>,
) -> CommitMessageAiSettings {
    let commit_choice = source_control_ai
        .model_overrides_by_operation
        .as_ref()
        .and_then(|overrides| overrides.get(&SourceControlAiOperation::CommitMessage));

    let mut selected_model_by_agent = source_control_ai.selected_model_by_agent.clone();
    if let Some(by_agent) = commit_choice.and_then(|choice| choice.selected_model_by_agent.as_ref())
    {
        for (key, value) in by_agent {
            selected_model_by_agent.insert(key.clone(), value.clone());
        }
    }
    let mut selected_thinking_by_model = source_control_ai.selected_thinking_by_model.clone();
    if let Some(by_model) =
        commit_choice.and_then(|choice| choice.selected_thinking_by_model.as_ref())
    {
        for (key, value) in by_model {
            selected_thinking_by_model.insert(key.clone(), value.clone());
        }
    }
    let custom_prompt = source_control_ai
        .instructions_by_operation
        .get(&SourceControlAiOperation::CommitMessage)
        .cloned()
        .or_else(|| previous_legacy.map(|legacy| legacy.custom_prompt.clone()))
        .unwrap_or_default();

    CommitMessageAiSettings {
        enabled: source_control_ai.enabled,
        agent_id: source_control_ai.agent_id.clone(),
        selected_model_by_agent,
        selected_model_by_agent_by_host: Some(merge_selected_model_by_agent_by_host(
            source_control_ai.selected_model_by_agent_by_host.as_ref(),
            commit_choice.and_then(|choice| choice.selected_model_by_agent_by_host.as_ref()),
        )),
        discovered_models_by_agent: Some(
            source_control_ai.discovered_models_by_agent.clone().unwrap_or_default(),
        ),
        discovered_models_by_agent_by_host: Some(
            source_control_ai.discovered_models_by_agent_by_host.clone().unwrap_or_default(),
        ),
        selected_thinking_by_model,
        custom_prompt,
        custom_agent_command: source_control_ai.custom_agent_command.clone(),
    }
}

// ---------------------------------------------------------------------------
// Per-operation resolution.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSourceControlAiGenerationParams {
    pub agent_id: String,
    pub model: String,
    pub thinking_level: Option<String>,
    pub custom_prompt: Option<String>,
    pub custom_agent_command: Option<String>,
    pub agent_command_override: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSourceControlAiOperation {
    pub enabled: bool,
    pub params: ResolvedSourceControlAiGenerationParams,
    pub pr_creation_defaults: ResolvedPrCreationDefaults,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveSourceControlAiResult {
    Ok(ResolvedSourceControlAiOperation),
    Err(String),
}

/// The slice of `GlobalSettings` the resolvers actually read.
#[derive(Clone, Debug, Default)]
pub struct GlobalSettingsSlice {
    pub default_tui_agent: Option<String>,
    pub agent_cmd_overrides: BTreeMap<String, String>,
    pub commit_message_ai: Option<CommitMessageAiSettings>,
    pub source_control_ai: Option<SourceControlAiSettings>,
    pub disabled_tui_agents: Vec<String>,
}

pub struct ResolveSourceControlAiInput<'a> {
    pub settings: &'a GlobalSettingsSlice,
    /// `repo?.sourceControlAi` as raw JSON (normalized defensively); `None` when
    /// no repo override is present.
    pub repo_source_control_ai: Option<&'a Value>,
    pub operation: SourceControlAiOperation,
    pub discovery_host_key: Option<&'a str>,
    pub pr_creation_product_defaults: Option<&'a SourceControlAiPrCreationDefaults>,
}

fn read_default_selected_model_id(
    source: &SourceControlAiSettings,
    host_key: &str,
    agent_id: &str,
) -> Option<String> {
    let choice = SourceControlAiModelChoice {
        selected_model_by_agent: Some(source.selected_model_by_agent.clone()),
        selected_model_by_agent_by_host: source.selected_model_by_agent_by_host.clone(),
        selected_thinking_by_model: None,
    };
    read_source_control_ai_model_choice_for_host(Some(&choice), host_key, agent_id)
}

fn get_discovered_models(
    source: &SourceControlAiSettings,
    legacy: Option<&CommitMessageAiSettings>,
    host_key: &str,
    agent_id: &str,
) -> Vec<CommitMessageAiModelCapability> {
    if let Some(models) = source
        .discovered_models_by_agent_by_host
        .as_ref()
        .and_then(|by_host| by_host.get(host_key))
        .and_then(|by_agent| by_agent.get(agent_id))
    {
        return models.clone();
    }
    if host_key == LOCAL_COMMIT_MESSAGE_HOST_KEY {
        if let Some(models) = source
            .discovered_models_by_agent
            .as_ref()
            .and_then(|by_agent| by_agent.get(agent_id))
        {
            return models.clone();
        }
        if let Some(legacy) = legacy {
            if let Some(models) = legacy
                .discovered_models_by_agent_by_host
                .as_ref()
                .and_then(|by_host| by_host.get(host_key))
                .and_then(|by_agent| by_agent.get(agent_id))
            {
                return models.clone();
            }
            if let Some(models) = legacy
                .discovered_models_by_agent
                .as_ref()
                .and_then(|by_agent| by_agent.get(agent_id))
            {
                return models.clone();
            }
        }
        Vec::new()
    } else {
        legacy
            .and_then(|legacy| legacy.discovered_models_by_agent_by_host.as_ref())
            .and_then(|by_host| by_host.get(host_key))
            .and_then(|by_agent| by_agent.get(agent_id))
            .cloned()
            .unwrap_or_default()
    }
}

#[allow(clippy::too_many_arguments)]
fn select_persisted_model_id(
    source: &SourceControlAiSettings,
    legacy: Option<&CommitMessageAiSettings>,
    repo_overrides: Option<&RepoSourceControlAiOverrides>,
    operation: SourceControlAiOperation,
    host_key: &str,
    agent_id: &str,
    default_model_id: &str,
) -> String {
    let repo_choice = repo_overrides
        .and_then(|overrides| overrides.model_overrides_by_operation.as_ref())
        .and_then(|by_op| by_op.get(&operation));
    let source_choice = source
        .model_overrides_by_operation
        .as_ref()
        .and_then(|by_op| by_op.get(&operation));
    read_source_control_ai_model_choice_for_host(repo_choice, host_key, agent_id)
        .or_else(|| read_source_control_ai_model_choice_for_host(source_choice, host_key, agent_id))
        .or_else(|| read_default_selected_model_id(source, host_key, agent_id))
        .or_else(|| {
            legacy
                .and_then(|legacy| legacy.selected_model_by_agent_by_host.as_ref())
                .and_then(|by_host| by_host.get(host_key))
                .and_then(|by_agent| by_agent.get(agent_id))
                .cloned()
        })
        .or_else(|| {
            if host_key == LOCAL_COMMIT_MESSAGE_HOST_KEY {
                legacy
                    .and_then(|legacy| legacy.selected_model_by_agent.get(agent_id))
                    .cloned()
            } else {
                None
            }
        })
        .unwrap_or_else(|| default_model_id.to_string())
}

fn resolve_thinking_level(
    model: &CommitMessageAiModelCapability,
    source: &SourceControlAiSettings,
    legacy: Option<&CommitMessageAiSettings>,
    repo_overrides: Option<&RepoSourceControlAiOverrides>,
    operation: SourceControlAiOperation,
) -> Option<String> {
    let levels = model.thinking_levels.as_ref().filter(|levels| !levels.is_empty())?;
    let persisted = repo_overrides
        .and_then(|overrides| overrides.model_overrides_by_operation.as_ref())
        .and_then(|by_op| by_op.get(&operation))
        .and_then(|choice| choice.selected_thinking_by_model.as_ref())
        .and_then(|by_model| by_model.get(&model.id))
        .or_else(|| {
            source
                .model_overrides_by_operation
                .as_ref()
                .and_then(|by_op| by_op.get(&operation))
                .and_then(|choice| choice.selected_thinking_by_model.as_ref())
                .and_then(|by_model| by_model.get(&model.id))
        })
        .or_else(|| source.selected_thinking_by_model.get(&model.id))
        .or_else(|| legacy.and_then(|legacy| legacy.selected_thinking_by_model.get(&model.id)));
    if levels.iter().any(|level| Some(&level.id) == persisted) {
        persisted.cloned()
    } else {
        model.default_thinking_level.clone()
    }
}

fn read_repo_instruction_override(
    overrides: Option<&RepoSourceControlAiOverrides>,
    operation: SourceControlAiOperation,
) -> Option<String> {
    let instructions = overrides?.instructions_by_operation.as_ref()?;
    // Present-and-string → the override; present-and-null or absent → inherit.
    match instructions.get(&operation) {
        Some(Some(instruction)) => Some(instruction.clone()),
        _ => None,
    }
}

#[cfg_attr(trust_verify, trust::ensures(|out: &String| out.trim().len() == out.len()))]
pub fn resolve_source_control_ai_instructions(
    settings: &GlobalSettingsSlice,
    repo_source_control_ai: Option<&Value>,
    operation: SourceControlAiOperation,
) -> String {
    let source = normalize_source_control_ai_settings(
        settings.source_control_ai.as_ref(),
        settings.commit_message_ai.as_ref(),
    );
    let repo_overrides =
        normalize_repo_source_control_ai_overrides(repo_source_control_ai.unwrap_or(&NULL));
    if let Some(instruction) = read_repo_instruction_override(repo_overrides.as_ref(), operation) {
        return instruction.trim().to_string();
    }
    if let Some(global) = source.instructions_by_operation.get(&operation) {
        return global.trim().to_string();
    }
    if operation == SourceControlAiOperation::CommitMessage {
        return settings
            .commit_message_ai
            .as_ref()
            .map(|legacy| legacy.custom_prompt.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
    }
    String::new()
}

pub fn has_configured_source_control_ai_instructions(
    settings: &GlobalSettingsSlice,
    repo_source_control_ai: Option<&Value>,
    operation: SourceControlAiOperation,
) -> bool {
    let repo_overrides =
        normalize_repo_source_control_ai_overrides(repo_source_control_ai.unwrap_or(&NULL));
    if read_repo_instruction_override(repo_overrides.as_ref(), operation).is_some() {
        return true;
    }
    !resolve_source_control_ai_instructions(settings, repo_source_control_ai, operation).is_empty()
}

fn resolve_pr_creation_defaults(
    source: &SourceControlAiSettings,
    repo_overrides: Option<&RepoSourceControlAiOverrides>,
    product_defaults: Option<&SourceControlAiPrCreationDefaults>,
) -> ResolvedPrCreationDefaults {
    let mut base = DEFAULT_PR_CREATION_DEFAULTS;
    if let Some(product) = product_defaults {
        overlay_pr_defaults(&mut base, product);
    }
    if let Some(source_defaults) = source.pr_creation_defaults.as_ref() {
        overlay_pr_defaults(&mut base, source_defaults);
    }
    let repo_defaults = match repo_overrides.and_then(|overrides| overrides.pr_creation_defaults.as_ref())
    {
        Some(repo_defaults) => repo_defaults,
        None => return base,
    };
    ResolvedPrCreationDefaults {
        draft: flatten_repo_default(repo_defaults.draft, base.draft),
        use_template: flatten_repo_default(repo_defaults.use_template, base.use_template),
        generate_details_on_open: flatten_repo_default(
            repo_defaults.generate_details_on_open,
            base.generate_details_on_open,
        ),
        open_after_create: flatten_repo_default(
            repo_defaults.open_after_create,
            base.open_after_create,
        ),
    }
}

fn overlay_pr_defaults(base: &mut ResolvedPrCreationDefaults, overlay: &SourceControlAiPrCreationDefaults) {
    if let Some(draft) = overlay.draft {
        base.draft = draft;
    }
    if let Some(use_template) = overlay.use_template {
        base.use_template = use_template;
    }
    if let Some(generate_details_on_open) = overlay.generate_details_on_open {
        base.generate_details_on_open = generate_details_on_open;
    }
    if let Some(open_after_create) = overlay.open_after_create {
        base.open_after_create = open_after_create;
    }
}

fn flatten_repo_default(field: Option<Option<bool>>, base: bool) -> bool {
    match field {
        Some(Some(value)) => value,
        _ => base,
    }
}

pub fn resolve_source_control_ai_pr_creation_defaults(
    settings: &GlobalSettingsSlice,
    repo_source_control_ai: Option<&Value>,
    product_defaults: Option<&SourceControlAiPrCreationDefaults>,
) -> ResolvedPrCreationDefaults {
    let source = normalize_source_control_ai_settings(
        settings.source_control_ai.as_ref(),
        settings.commit_message_ai.as_ref(),
    );
    let repo_overrides =
        normalize_repo_source_control_ai_overrides(repo_source_control_ai.unwrap_or(&NULL));
    resolve_pr_creation_defaults(&source, repo_overrides.as_ref(), product_defaults)
}

pub fn resolve_source_control_ai_for_operation(
    input: &ResolveSourceControlAiInput,
) -> ResolveSourceControlAiResult {
    let legacy = input.settings.commit_message_ai.as_ref();
    let source =
        normalize_source_control_ai_settings(input.settings.source_control_ai.as_ref(), legacy);
    if !source.enabled {
        return ResolveSourceControlAiResult::Err("Enable Git AI Author in Settings -> Git.".to_string());
    }

    // Why: a normalized null means "use the current default agent"; stale legacy
    // commitMessageAi should not make that choice sticky again.
    let disabled: Vec<&str> = input
        .settings
        .disabled_tui_agents
        .iter()
        .map(String::as_str)
        .collect();
    let agent_choice = match resolve_commit_message_agent_choice(
        source.agent_id.as_deref(),
        input.settings.default_tui_agent.as_deref(),
        &disabled,
    ) {
        Some(agent_choice) => agent_choice,
        None => {
            return ResolveSourceControlAiResult::Err(format!(
                "Default agent \"{}\" does not support Git AI Author. Choose Claude, Codex, or Custom in Settings -> Git -> Git AI Author.",
                input.settings.default_tui_agent.as_deref().unwrap_or("null")
            ));
        }
    };

    let repo_overrides = normalize_repo_source_control_ai_overrides(
        input.repo_source_control_ai.unwrap_or(&NULL),
    );
    let pr_creation_defaults = resolve_pr_creation_defaults(
        &source,
        repo_overrides.as_ref(),
        input.pr_creation_product_defaults,
    );

    if is_custom_agent_id(Some(agent_choice.as_str())) {
        let custom_agent_command = source.custom_agent_command.trim();
        if custom_agent_command.is_empty() {
            return ResolveSourceControlAiResult::Err(
                "Custom command is empty. Add one in Settings -> Git -> Git AI Author.".to_string(),
            );
        }
        return ResolveSourceControlAiResult::Ok(ResolvedSourceControlAiOperation {
            enabled: true,
            params: ResolvedSourceControlAiGenerationParams {
                agent_id: CUSTOM_AGENT_ID.to_string(),
                model: String::new(),
                thinking_level: None,
                custom_prompt: Some(resolve_source_control_ai_instructions(
                    input.settings,
                    input.repo_source_control_ai,
                    input.operation,
                )),
                custom_agent_command: Some(custom_agent_command.to_string()),
                agent_command_override: None,
            },
            pr_creation_defaults,
        });
    }

    let agent_id = agent_choice;
    let spec = match get_commit_message_agent_spec(&agent_id) {
        Some(spec) => spec,
        None => {
            return ResolveSourceControlAiResult::Err(format!(
                "Agent \"{}\" does not support Git AI Author {}.",
                agent_id,
                input.operation.label()
            ));
        }
    };

    let host_key = input.discovery_host_key.unwrap_or(LOCAL_COMMIT_MESSAGE_HOST_KEY);
    let persisted_model_id = select_persisted_model_id(
        &source,
        legacy,
        repo_overrides.as_ref(),
        input.operation,
        host_key,
        &agent_id,
        spec.default_model_id,
    );
    let discovered_models = get_discovered_models(&source, legacy, host_key, &agent_id);
    let model = spec
        .models
        .iter()
        .find(|candidate| candidate.id == persisted_model_id)
        .cloned()
        .or_else(|| {
            discovered_models
                .iter()
                .find(|candidate| candidate.id == persisted_model_id)
                .cloned()
        })
        .or_else(|| get_commit_message_model(&agent_id, spec.default_model_id));
    let model = match model {
        Some(model) => model,
        None => {
            return ResolveSourceControlAiResult::Err(format!(
                "No model is available for {}.",
                spec.label
            ));
        }
    };

    let thinking_level =
        resolve_thinking_level(&model, &source, legacy, repo_overrides.as_ref(), input.operation);
    let agent_command_override = input
        .settings
        .agent_cmd_overrides
        .get(&agent_id)
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty());

    ResolveSourceControlAiResult::Ok(ResolvedSourceControlAiOperation {
        enabled: true,
        params: ResolvedSourceControlAiGenerationParams {
            agent_id,
            model: model.id.clone(),
            thinking_level,
            custom_prompt: Some(resolve_source_control_ai_instructions(
                input.settings,
                input.repo_source_control_ai,
                input.operation,
            )),
            custom_agent_command: None,
            agent_command_override,
        },
        pr_creation_defaults,
    })
}

#[cfg(test)]
mod tests {
    use super::SourceControlAiOperation::{BranchName, CommitMessage, PullRequest};
    use super::*;
    use serde_json::json;

    fn smap(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
    }

    fn host_map(pairs: &[(&str, &[(&str, &str)])]) -> BTreeMap<String, BTreeMap<String, String>> {
        pairs.iter().map(|(host, models)| (host.to_string(), smap(models))).collect()
    }

    fn instr(pairs: &[(SourceControlAiOperation, &str)]) -> BTreeMap<SourceControlAiOperation, String> {
        pairs.iter().map(|(op, value)| (*op, value.to_string())).collect()
    }

    fn default_commit_message_ai() -> CommitMessageAiSettings {
        CommitMessageAiSettings {
            enabled: true,
            agent_id: None,
            selected_model_by_agent: BTreeMap::new(),
            selected_model_by_agent_by_host: Some(BTreeMap::new()),
            discovered_models_by_agent: Some(BTreeMap::new()),
            discovered_models_by_agent_by_host: Some(BTreeMap::new()),
            selected_thinking_by_model: BTreeMap::new(),
            custom_prompt: String::new(),
            custom_agent_command: String::new(),
        }
    }

    fn settings() -> GlobalSettingsSlice {
        let mut source = get_default_source_control_ai_settings();
        source.enabled = true;
        source.agent_id = Some("codex".to_string());
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.5")]);
        source.selected_thinking_by_model = smap(&[("gpt-5.5", "medium"), ("gpt-5.4", "high")]);
        source.instructions_by_operation = instr(&[
            (CommitMessage, "Global commit style"),
            (PullRequest, "Global PR style"),
            (BranchName, "Global branch style"),
        ]);
        GlobalSettingsSlice {
            default_tui_agent: Some("codex".to_string()),
            agent_cmd_overrides: BTreeMap::new(),
            commit_message_ai: Some(default_commit_message_ai()),
            source_control_ai: Some(source),
            disabled_tui_agents: Vec::new(),
        }
    }

    fn resolve(
        operation: SourceControlAiOperation,
        overrides: Option<&Value>,
    ) -> ResolvedSourceControlAiOperation {
        let settings = settings();
        let product = SourceControlAiPrCreationDefaults {
            draft: Some(false),
            use_template: Some(false),
            generate_details_on_open: Some(false),
            open_after_create: Some(false),
        };
        let input = ResolveSourceControlAiInput {
            settings: &settings,
            repo_source_control_ai: overrides,
            operation,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: Some(&product),
        };
        match resolve_source_control_ai_for_operation(&input) {
            ResolveSourceControlAiResult::Ok(value) => value,
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn uses_the_global_default_model_for_every_operation() {
        assert_eq!(resolve(CommitMessage, None).params.model, "gpt-5.5");
        assert_eq!(resolve(PullRequest, None).params.model, "gpt-5.5");
        assert_eq!(resolve(BranchName, None).params.model, "gpt-5.5");
    }

    #[test]
    fn resolves_pr_defaults_even_when_generation_is_disabled() {
        let mut base = settings();
        let mut source = base.source_control_ai.clone().unwrap();
        source.enabled = false;
        source.pr_creation_defaults = Some(SourceControlAiPrCreationDefaults {
            draft: Some(true),
            use_template: Some(true),
            generate_details_on_open: Some(false),
            open_after_create: Some(false),
        });
        base.source_control_ai = Some(source);

        let input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: PullRequest,
            discovery_host_key: None,
            pr_creation_product_defaults: None,
        };
        assert!(matches!(
            resolve_source_control_ai_for_operation(&input),
            ResolveSourceControlAiResult::Err(_)
        ));

        let repo = json!({
            "prCreationDefaults": { "draft": null, "generateDetailsOnOpen": true, "openAfterCreate": true }
        });
        let product = SourceControlAiPrCreationDefaults {
            draft: Some(false),
            use_template: Some(false),
            generate_details_on_open: Some(false),
            open_after_create: Some(false),
        };
        assert_eq!(
            resolve_source_control_ai_pr_creation_defaults(&base, Some(&repo), Some(&product)),
            ResolvedPrCreationDefaults {
                draft: true,
                use_template: true,
                generate_details_on_open: true,
                open_after_create: true,
            }
        );
    }

    #[test]
    fn resolves_pr_defaults_even_when_generation_config_is_invalid() {
        let mut base = settings();
        let mut source = base.source_control_ai.clone().unwrap();
        source.agent_id = Some("custom".to_string());
        source.custom_agent_command = String::new();
        source.pr_creation_defaults = Some(SourceControlAiPrCreationDefaults {
            draft: Some(false),
            use_template: Some(true),
            generate_details_on_open: Some(true),
            open_after_create: Some(false),
        });
        base.source_control_ai = Some(source);

        let input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: PullRequest,
            discovery_host_key: None,
            pr_creation_product_defaults: None,
        };
        assert!(matches!(
            resolve_source_control_ai_for_operation(&input),
            ResolveSourceControlAiResult::Err(_)
        ));

        let repo = json!({ "prCreationDefaults": { "draft": true } });
        assert_eq!(
            resolve_source_control_ai_pr_creation_defaults(&base, Some(&repo), None),
            ResolvedPrCreationDefaults {
                draft: true,
                use_template: true,
                generate_details_on_open: true,
                open_after_create: false,
            }
        );
    }

    #[test]
    fn treats_a_normalized_null_agent_as_default_instead_of_stale_legacy() {
        let mut base = settings();
        base.default_tui_agent = Some("codex".to_string());
        base.commit_message_ai = Some(CommitMessageAiSettings {
            agent_id: Some("claude".to_string()),
            selected_model_by_agent: smap(&[("claude", "opus")]),
            selected_thinking_by_model: smap(&[("opus", "max")]),
            ..default_commit_message_ai()
        });
        let mut source = base.source_control_ai.clone().unwrap();
        source.agent_id = None;
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.4")]);
        base.source_control_ai = Some(source);

        let input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: CommitMessage,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: None,
        };
        match resolve_source_control_ai_for_operation(&input) {
            ResolveSourceControlAiResult::Ok(value) => {
                assert_eq!(value.params.agent_id, "codex");
                assert_eq!(value.params.model, "gpt-5.4");
            }
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn lets_a_global_operation_model_override_win_over_the_global_default() {
        let mut base = settings();
        let mut source = base.source_control_ai.clone().unwrap();
        let mut overrides = BTreeMap::new();
        overrides.insert(
            PullRequest,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                ..Default::default()
            },
        );
        source.model_overrides_by_operation = Some(overrides);
        base.source_control_ai = Some(source);

        let input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: PullRequest,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: None,
        };
        match resolve_source_control_ai_for_operation(&input) {
            ResolveSourceControlAiResult::Ok(value) => assert_eq!(value.params.model, "gpt-5.4"),
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn lets_a_repo_operation_model_override_win_over_global_operation_override() {
        let mut base = settings();
        let mut source = base.source_control_ai.clone().unwrap();
        let mut overrides = BTreeMap::new();
        overrides.insert(
            CommitMessage,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                ..Default::default()
            },
        );
        source.model_overrides_by_operation = Some(overrides);
        base.source_control_ai = Some(source);

        let repo = json!({
            "modelOverridesByOperation": { "commitMessage": { "selectedModelByAgent": { "codex": "gpt-5.4-mini" } } }
        });
        let input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: Some(&repo),
            operation: CommitMessage,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: None,
        };
        match resolve_source_control_ai_for_operation(&input) {
            ResolveSourceControlAiResult::Ok(value) => assert_eq!(value.params.model, "gpt-5.4-mini"),
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn resolves_thinking_effort_with_override_precedence_and_model_default_fallback() {
        assert_eq!(resolve(CommitMessage, None).params.thinking_level.as_deref(), Some("medium"));

        let overrides = json!({
            "modelOverridesByOperation": {
                "commitMessage": {
                    "selectedModelByAgent": { "codex": "gpt-5.4" },
                    "selectedThinkingByModel": { "gpt-5.4": "xhigh" }
                }
            }
        });
        assert_eq!(resolve(CommitMessage, Some(&overrides)).params.thinking_level.as_deref(), Some("xhigh"));

        let mut base = settings();
        let mut source = base.source_control_ai.clone().unwrap();
        source.selected_thinking_by_model = smap(&[("gpt-5.5", "unsupported")]);
        base.source_control_ai = Some(source);
        let input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: CommitMessage,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: None,
        };
        match resolve_source_control_ai_for_operation(&input) {
            ResolveSourceControlAiResult::Ok(value) => {
                assert_eq!(value.params.thinking_level.as_deref(), Some("low"))
            }
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn resolves_repo_instructions_as_replacement_overrides_including_explicit_empty() {
        assert_eq!(resolve(CommitMessage, None).params.custom_prompt.as_deref(), Some("Global commit style"));
        let null_override = json!({ "instructionsByOperation": { "commitMessage": null } });
        assert_eq!(
            resolve(CommitMessage, Some(&null_override)).params.custom_prompt.as_deref(),
            Some("Global commit style")
        );
        let empty_override = json!({ "instructionsByOperation": { "commitMessage": "" } });
        assert_eq!(resolve(CommitMessage, Some(&empty_override)).params.custom_prompt.as_deref(), Some(""));
        let repo_override = json!({ "instructionsByOperation": { "commitMessage": "Repo commit style" } });
        assert_eq!(
            resolve(CommitMessage, Some(&repo_override)).params.custom_prompt.as_deref(),
            Some("Repo commit style")
        );
        assert_eq!(resolve(BranchName, None).params.custom_prompt.as_deref(), Some("Global branch style"));
        let branch_override = json!({ "instructionsByOperation": { "branchName": "Repo branch style" } });
        assert_eq!(
            resolve(BranchName, Some(&branch_override)).params.custom_prompt.as_deref(),
            Some("Repo branch style")
        );
    }

    #[test]
    fn does_not_treat_null_repo_instructions_as_configured_overrides() {
        let mut base = settings();
        let mut source = base.source_control_ai.clone().unwrap();
        source.instructions_by_operation =
            instr(&[(CommitMessage, ""), (PullRequest, ""), (BranchName, "")]);
        base.source_control_ai = Some(source);

        let null_override = json!({ "instructionsByOperation": { "commitMessage": null } });
        assert!(!has_configured_source_control_ai_instructions(
            &base,
            Some(&null_override),
            CommitMessage
        ));
        let empty_override = json!({ "instructionsByOperation": { "commitMessage": "" } });
        assert!(has_configured_source_control_ai_instructions(
            &base,
            Some(&empty_override),
            CommitMessage
        ));
    }

    #[test]
    fn resolves_repo_tri_state_pr_defaults_through_inherit_on_and_off() {
        assert!(!resolve(PullRequest, None).pr_creation_defaults.draft);
        let overrides = json!({ "prCreationDefaults": { "draft": true, "openAfterCreate": false } });
        let pr = resolve(PullRequest, Some(&overrides)).pr_creation_defaults;
        assert!(pr.draft);
        assert!(!pr.open_after_create);
    }

    #[test]
    fn maps_legacy_custom_prompt_to_released_split_instructions() {
        let migrated = source_control_ai_settings_from_legacy(Some(&CommitMessageAiSettings {
            enabled: true,
            agent_id: Some("codex".to_string()),
            selected_model_by_agent: smap(&[("codex", "gpt-5.5")]),
            selected_thinking_by_model: BTreeMap::new(),
            custom_prompt: "Legacy commit prompt".to_string(),
            custom_agent_command: String::new(),
            ..Default::default()
        }));
        assert_eq!(migrated.instructions_by_operation.get(&CommitMessage).unwrap(), "Legacy commit prompt");
        assert_eq!(migrated.instructions_by_operation.get(&PullRequest).unwrap(), "");
        assert_eq!(migrated.instructions_by_operation.get(&BranchName).unwrap(), "Legacy commit prompt");
    }

    #[test]
    fn merges_legacy_commit_message_updates_without_wiping_pr_only_settings() {
        let base = settings().source_control_ai.clone().unwrap();
        let merged = merge_legacy_commit_message_ai_into_source_control_ai(
            Some(&base),
            Some(&CommitMessageAiSettings {
                enabled: false,
                agent_id: Some("claude".to_string()),
                selected_model_by_agent: smap(&[("claude", "sonnet")]),
                selected_thinking_by_model: smap(&[("sonnet", "medium")]),
                custom_prompt: "Legacy commit prompt".to_string(),
                custom_agent_command: "claude".to_string(),
                ..Default::default()
            }),
            MergeLegacyOptions { pull_request_instructions_from_legacy: false },
        );

        assert!(!merged.enabled);
        assert_eq!(merged.agent_id.as_deref(), Some("claude"));
        assert_eq!(merged.selected_model_by_agent, smap(&[("codex", "gpt-5.5")]));
        assert_eq!(
            merged.selected_thinking_by_model,
            smap(&[("gpt-5.5", "medium"), ("gpt-5.4", "high")])
        );
        assert_eq!(merged.custom_agent_command, "claude");
        assert_eq!(merged.instructions_by_operation.get(&CommitMessage).unwrap(), "Legacy commit prompt");
        assert_eq!(merged.instructions_by_operation.get(&PullRequest).unwrap(), "Global PR style");
        assert_eq!(merged.instructions_by_operation.get(&BranchName).unwrap(), "Legacy commit prompt");
        assert_eq!(
            *merged
                .model_overrides_by_operation
                .as_ref()
                .unwrap()
                .get(&CommitMessage)
                .unwrap(),
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("claude", "sonnet")])),
                selected_model_by_agent_by_host: None,
                selected_thinking_by_model: Some(smap(&[("sonnet", "medium")])),
            }
        );
    }

    #[test]
    fn can_map_explicit_legacy_pr_generation_instructions_for_old_runtime_callers() {
        let merged = merge_legacy_commit_message_ai_into_source_control_ai(
            None,
            Some(&CommitMessageAiSettings {
                enabled: true,
                agent_id: Some("codex".to_string()),
                selected_model_by_agent: smap(&[("codex", "gpt-5.5")]),
                selected_thinking_by_model: BTreeMap::new(),
                custom_prompt: "Legacy PR prompt".to_string(),
                custom_agent_command: String::new(),
                ..Default::default()
            }),
            MergeLegacyOptions { pull_request_instructions_from_legacy: true },
        );

        assert_eq!(merged.instructions_by_operation.get(&PullRequest).unwrap(), "Legacy PR prompt");
    }

    #[test]
    fn projects_commit_message_operation_model_overrides_into_legacy_settings() {
        let mut source = settings().source_control_ai.clone().unwrap();
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.5"), ("claude", "sonnet")]);
        source.selected_model_by_agent_by_host = Some(host_map(&[
            ("local", &[("codex", "gpt-5.5")]),
            ("ssh:conn-1", &[("codex", "gpt-5.5"), ("claude", "sonnet")]),
        ]));
        source.selected_thinking_by_model = smap(&[("gpt-5.4", "high"), ("gpt-5.5", "medium")]);
        let mut overrides = BTreeMap::new();
        overrides.insert(
            CommitMessage,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                selected_model_by_agent_by_host: Some(host_map(&[
                    ("local", &[("codex", "gpt-5.4")]),
                    ("ssh:conn-1", &[("codex", "gpt-5.4-mini")]),
                ])),
                selected_thinking_by_model: Some(smap(&[
                    ("gpt-5.4", "xhigh"),
                    ("gpt-5.4-mini", "medium"),
                ])),
            },
        );
        overrides.insert(
            PullRequest,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.2")])),
                selected_model_by_agent_by_host: None,
                selected_thinking_by_model: Some(smap(&[("gpt-5.2", "low")])),
            },
        );
        source.model_overrides_by_operation = Some(overrides);

        let legacy = project_source_control_ai_to_legacy_commit_message_ai(&source, None);
        assert_eq!(legacy.selected_model_by_agent.get("codex").map(String::as_str), Some("gpt-5.4"));
        assert_eq!(legacy.selected_model_by_agent.get("claude").map(String::as_str), Some("sonnet"));
        let by_host = legacy.selected_model_by_agent_by_host.as_ref().unwrap();
        assert_eq!(by_host.get("local").unwrap().get("codex").map(String::as_str), Some("gpt-5.4"));
        assert_eq!(by_host.get("ssh:conn-1").unwrap().get("codex").map(String::as_str), Some("gpt-5.4-mini"));
        assert_eq!(by_host.get("ssh:conn-1").unwrap().get("claude").map(String::as_str), Some("sonnet"));
        assert_eq!(legacy.selected_thinking_by_model.get("gpt-5.4").map(String::as_str), Some("xhigh"));
        assert_eq!(legacy.selected_thinking_by_model.get("gpt-5.4-mini").map(String::as_str), Some("medium"));
        assert_eq!(legacy.selected_thinking_by_model.get("gpt-5.5").map(String::as_str), Some("medium"));
        assert_eq!(legacy.selected_thinking_by_model.get("gpt-5.2"), None);
    }

    #[test]
    fn merges_projected_legacy_commit_message_models_without_changing_pr_defaults() {
        let mut source = settings().source_control_ai.clone().unwrap();
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.5")]);
        source.selected_thinking_by_model = smap(&[("gpt-5.5", "medium")]);
        let mut overrides = BTreeMap::new();
        overrides.insert(
            CommitMessage,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                selected_model_by_agent_by_host: None,
                selected_thinking_by_model: Some(smap(&[("gpt-5.4", "high")])),
            },
        );
        source.model_overrides_by_operation = Some(overrides);

        let legacy = project_source_control_ai_to_legacy_commit_message_ai(&source, None);
        let merged = merge_legacy_commit_message_ai_into_source_control_ai(
            Some(&source),
            Some(&legacy),
            MergeLegacyOptions { pull_request_instructions_from_legacy: false },
        );

        assert_eq!(merged.selected_model_by_agent.get("codex").map(String::as_str), Some("gpt-5.5"));
        assert_eq!(
            merged
                .model_overrides_by_operation
                .as_ref()
                .and_then(|by_op| by_op.get(&CommitMessage))
                .and_then(|choice| choice.selected_model_by_agent.as_ref())
                .and_then(|by_agent| by_agent.get("codex"))
                .map(String::as_str),
            Some("gpt-5.4")
        );

        let mut base = settings();
        base.source_control_ai = Some(merged);
        let commit_input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: CommitMessage,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: None,
        };
        match resolve_source_control_ai_for_operation(&commit_input) {
            ResolveSourceControlAiResult::Ok(value) => assert_eq!(value.params.model, "gpt-5.4"),
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
        let pr_input = ResolveSourceControlAiInput {
            settings: &base,
            repo_source_control_ai: None,
            operation: PullRequest,
            discovery_host_key: Some("local"),
            pr_creation_product_defaults: None,
        };
        match resolve_source_control_ai_for_operation(&pr_input) {
            ResolveSourceControlAiResult::Ok(value) => assert_eq!(value.params.model, "gpt-5.5"),
            ResolveSourceControlAiResult::Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn does_not_synthesize_a_commit_message_override_from_projected_global_defaults() {
        let mut source = settings().source_control_ai.clone().unwrap();
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.5")]);
        source.selected_thinking_by_model = smap(&[("gpt-5.5", "medium")]);
        source.model_overrides_by_operation = None;

        let legacy = project_source_control_ai_to_legacy_commit_message_ai(&source, None);
        let merged = merge_legacy_commit_message_ai_into_source_control_ai(
            Some(&source),
            Some(&legacy),
            MergeLegacyOptions { pull_request_instructions_from_legacy: false },
        );

        assert_eq!(merged.selected_model_by_agent.get("codex").map(String::as_str), Some("gpt-5.5"));
        assert!(merged
            .model_overrides_by_operation
            .as_ref()
            .and_then(|by_op| by_op.get(&CommitMessage))
            .is_none());
    }

    #[test]
    fn merges_only_rollback_legacy_model_deltas_into_commit_message_overrides() {
        let mut source = settings().source_control_ai.clone().unwrap();
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.5"), ("claude", "sonnet")]);
        source.selected_model_by_agent_by_host =
            Some(host_map(&[("local", &[("codex", "gpt-5.5"), ("claude", "sonnet")])]));
        source.selected_thinking_by_model = smap(&[("gpt-5.5", "medium"), ("sonnet", "high")]);
        source.model_overrides_by_operation = None;

        let mut legacy = project_source_control_ai_to_legacy_commit_message_ai(&source, None);
        legacy.selected_model_by_agent.insert("codex".to_string(), "gpt-5.4".to_string());
        {
            let by_host = legacy.selected_model_by_agent_by_host.get_or_insert_with(BTreeMap::new);
            let local = by_host.entry("local".to_string()).or_default();
            local.insert("codex".to_string(), "gpt-5.4".to_string());
        }

        let merged = merge_legacy_commit_message_ai_into_source_control_ai(
            Some(&source),
            Some(&legacy),
            MergeLegacyOptions { pull_request_instructions_from_legacy: false },
        );

        assert_eq!(merged.selected_model_by_agent, smap(&[("codex", "gpt-5.5"), ("claude", "sonnet")]));
        assert_eq!(
            *merged
                .model_overrides_by_operation
                .as_ref()
                .unwrap()
                .get(&CommitMessage)
                .unwrap(),
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                selected_model_by_agent_by_host: Some(host_map(&[("local", &[("codex", "gpt-5.4")])])),
                selected_thinking_by_model: None,
            }
        );
    }

    #[test]
    fn removes_projected_commit_message_overrides_cleared_by_legacy_settings() {
        let mut source = settings().source_control_ai.clone().unwrap();
        source.selected_model_by_agent = smap(&[("codex", "gpt-5.5")]);
        source.selected_thinking_by_model = smap(&[("gpt-5.5", "medium")]);
        let mut overrides = BTreeMap::new();
        overrides.insert(
            CommitMessage,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                selected_model_by_agent_by_host: None,
                selected_thinking_by_model: Some(smap(&[("gpt-5.4", "high")])),
            },
        );
        source.model_overrides_by_operation = Some(overrides);

        let mut legacy = project_source_control_ai_to_legacy_commit_message_ai(&source, None);
        legacy.selected_model_by_agent.remove("codex");
        legacy.selected_thinking_by_model.remove("gpt-5.4");

        let merged = merge_legacy_commit_message_ai_into_source_control_ai(
            Some(&source),
            Some(&legacy),
            MergeLegacyOptions { pull_request_instructions_from_legacy: false },
        );

        assert!(merged
            .model_overrides_by_operation
            .as_ref()
            .and_then(|by_op| by_op.get(&CommitMessage))
            .is_none());
    }

    #[test]
    fn reads_and_selects_host_scoped_model_choices_with_local_fallback_rules() {
        let local_choice =
            select_source_control_ai_model_choice_for_host(None, "local", "codex", "gpt-5.4");
        assert_eq!(
            local_choice,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                selected_model_by_agent_by_host: Some(host_map(&[("local", &[("codex", "gpt-5.4")])])),
                selected_thinking_by_model: None,
            }
        );

        let remote_choice = select_source_control_ai_model_choice_for_host(
            Some(&local_choice),
            "ssh:conn-1",
            "codex",
            "remote-model",
        );
        assert_eq!(
            read_source_control_ai_model_choice_for_host(Some(&remote_choice), "local", "codex").as_deref(),
            Some("gpt-5.4")
        );
        assert_eq!(
            read_source_control_ai_model_choice_for_host(Some(&remote_choice), "ssh:conn-1", "codex")
                .as_deref(),
            Some("remote-model")
        );
        assert_eq!(
            read_source_control_ai_model_choice_for_host(Some(&remote_choice), "ssh:conn-2", "codex"),
            None
        );
        let global_only = SourceControlAiModelChoice {
            selected_model_by_agent: Some(smap(&[("codex", "global-model")])),
            ..Default::default()
        };
        assert_eq!(
            read_source_control_ai_model_choice_for_host(Some(&global_only), "local", "codex").as_deref(),
            Some("global-model")
        );
    }

    #[test]
    fn clears_only_the_selected_host_model_override_when_inheriting() {
        let choice = SourceControlAiModelChoice {
            selected_model_by_agent: Some(smap(&[("codex", "local-model")])),
            selected_model_by_agent_by_host: Some(host_map(&[
                ("local", &[("codex", "local-model")]),
                ("ssh:conn-1", &[("codex", "remote-model")]),
            ])),
            selected_thinking_by_model: Some(smap(&[("remote-model", "high")])),
        };
        let cleared = clear_source_control_ai_model_choice_for_host(Some(&choice), "local", "codex");
        assert_eq!(
            cleared,
            Some(SourceControlAiModelChoice {
                selected_model_by_agent: None,
                selected_model_by_agent_by_host: Some(host_map(&[("ssh:conn-1", &[("codex", "remote-model")])])),
                selected_thinking_by_model: Some(smap(&[("remote-model", "high")])),
            })
        );
    }

    #[test]
    fn normalizes_repo_overrides_defensively_and_preserves_explicit_inherit_sentinels() {
        let input = json!({
            "modelOverridesByOperation": {
                "commitMessage": {
                    "selectedModelByAgent": { "codex": "gpt-5.4", "claude": 42, "constructor": "polluted" },
                    "selectedModelByAgentByHost": {
                        "local": { "codex": "gpt-5.4" },
                        "ssh:conn-1": { "codex": "remote-model", "claude": false },
                        "malformed": "not-a-record",
                        "prototype": { "codex": "polluted" }
                    },
                    "selectedThinkingByModel": {
                        "gpt-5.4": "xhigh",
                        "remote-model": "high",
                        "bad": true,
                        "constructor": "polluted"
                    }
                },
                "pullRequest": { "selectedModelByAgent": [] },
                "branchName": { "selectedModelByAgent": { "codex": "gpt-5.4" } },
                "unknown": { "selectedModelByAgent": { "codex": "ignored" } }
            },
            "instructionsByOperation": {
                "commitMessage": null,
                "pullRequest": "",
                "branchName": "branch style",
                "unknown": "ignored"
            },
            "prCreationDefaults": {
                "draft": true,
                "useTemplate": null,
                "generateDetailsOnOpen": "yes",
                "openAfterCreate": false
            }
        });

        let normalized = normalize_repo_source_control_ai_overrides(&input).unwrap();

        let mut model_overrides = BTreeMap::new();
        model_overrides.insert(
            CommitMessage,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                selected_model_by_agent_by_host: Some(host_map(&[
                    ("local", &[("codex", "gpt-5.4")]),
                    ("ssh:conn-1", &[("codex", "remote-model")]),
                ])),
                selected_thinking_by_model: Some(smap(&[("gpt-5.4", "xhigh"), ("remote-model", "high")])),
            },
        );
        model_overrides.insert(
            BranchName,
            SourceControlAiModelChoice {
                selected_model_by_agent: Some(smap(&[("codex", "gpt-5.4")])),
                ..Default::default()
            },
        );
        let mut instructions = BTreeMap::new();
        instructions.insert(CommitMessage, None);
        instructions.insert(PullRequest, Some(String::new()));
        instructions.insert(BranchName, Some("branch style".to_string()));

        assert_eq!(
            normalized,
            RepoSourceControlAiOverrides {
                model_overrides_by_operation: Some(model_overrides),
                instructions_by_operation: Some(instructions),
                pr_creation_defaults: Some(RepoPrCreationDefaults {
                    draft: Some(Some(true)),
                    use_template: Some(None),
                    generate_details_on_open: None,
                    open_after_create: Some(Some(false)),
                }),
            }
        );
        assert!(normalize_repo_source_control_ai_overrides(&Value::Null).is_none());
        assert!(normalize_repo_source_control_ai_overrides(&json!([])).is_none());
    }
}
