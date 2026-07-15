//! `orca-git-wasm` — the addon-less SSH relay's git-parsing substrate.
//!
//! The relay runs on the remote host as pure JS with NO native addon, so it
//! historically re-implemented git-output parsing in TypeScript. Those TS
//! parsers could (and did) drift from the Rust `orca-git` port that the main
//! process runs via napi. This crate compiles the SAME pure `orca-git` /
//! `orca-core` / `orca-text` functions to `wasm32-unknown-unknown`, so the relay
//! parses git output through the identical code — one source of truth.
//!
//! Most exports are the PURE parsers/validators (git output in -> data out),
//! which need no git runner. Multi-round operations that must actually RUN git
//! (rebase-source, upstream status, branch-cleanup, push) are also driven in Rust
//! here, through the async `AsyncGitRunner` bridge at the bottom of this file: it
//! awaits the relay's JS git executor via `wasm_bindgen_futures` instead of
//! `block_on`-ing it (wasm is single-threaded — the napi "A bridge" runs a sync
//! runner on a worker thread and blocks, which would deadlock the wasm event
//! loop). Same orca-git logic, same classification, driven asynchronously.
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

/// Approximate added/removed line counts for a diff section; returns the
/// line-stats JSON, or `undefined` for the large-input guard (>500k combined
/// chars — splitting that in a React render would block the UI). This one is
/// consumed by the RENDERER (not the relay): the renderer has no napi access,
/// so it loads this same wasm.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "computeLineStats"))]
pub fn compute_line_stats(original: &str, modified: &str, status: &str) -> Option<String> {
    orca_git::line_count::compute_line_stats(original, modified, status)
        .map(|stats| orca_git::status_result::line_stats_to_json(Some(stats)).to_string())
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

/// The actionable nested-submodule rejection hidden behind a recursive-push
/// failure, or `undefined`. Consumed by the RENDERER (push-failure toasts) via
/// this same wasm.
#[cfg_attr(
    target_arch = "wasm32",
    wasm_bindgen(js_name = "formatSubmodulePushFailureDetail")
)]
pub fn format_submodule_push_failure_detail(message: &str) -> Option<String> {
    orca_text::git_remote_error::format_submodule_push_failure_detail(message)
}

/// Prepared Quick Open index for the RENDERER: the worktree file list crosses
/// the wasm boundary ONCE (NUL-joined — file names cannot contain NUL), then
/// each keystroke sends only the query and gets the top-N `{path, score}`
/// JSON back. Preparation (slash-normalize, lowercase, UTF-16 encode) happens
/// at construction, so the per-keystroke cost is only the subsequence scans.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct QuickOpenIndex {
    inner: orca_text::quick_open_rank::QuickOpenIndex,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl QuickOpenIndex {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(nul_joined_paths: &str) -> QuickOpenIndex {
        let paths = if nul_joined_paths.is_empty() {
            Vec::new()
        } else {
            nul_joined_paths.split('\0').collect()
        };
        QuickOpenIndex { inner: orca_text::quick_open_rank::QuickOpenIndex::new(paths) }
    }

    /// Rank against the prepared list; returns `[{path, score}, …]` JSON,
    /// best (lowest score) first, ties by original input order.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn rank(&self, query: &str, limit: usize) -> String {
        let results = self.inner.rank(query, limit);
        serde_json::Value::Array(
            results
                .into_iter()
                .map(|r| serde_json::json!({ "path": r.path, "score": r.score }))
                .collect(),
        )
        .to_string()
    }

    /// Exact-path and exact-basename matches for an already-lowercased query
    /// (the TS `findExistingFileMatches` passes), as
    /// `{"paths":[…],"basenames":[…]}` JSON in input order.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "exactMatches"))]
    pub fn exact_matches(&self, lower_query: &str) -> String {
        serde_json::json!({
            "paths": self.inner.exact_path_matches(lower_query),
            "basenames": self.inner.exact_basename_matches(lower_query),
        })
        .to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "fileCount"))]
    pub fn file_count(&self) -> usize {
        self.inner.len()
    }
}

/// Short generated tab title from a free-form agent prompt (first clause,
/// filler stripped, capped at a word boundary), or `undefined` when the prompt
/// has no usable title text. Consumed by the RENDERER terminal store.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "deriveGeneratedTabTitle"))]
pub fn derive_generated_tab_title(prompt: &str) -> Option<String> {
    orca_text::agent_tab_title::derive_generated_tab_title(prompt)
}

/// True when `git cherry <upstream> HEAD`-style mark output shows at least one
/// commit and every commit is patch-equivalent (`=`). The relay's
/// behind-commits-are-patch-equivalent probe.
#[cfg_attr(
    target_arch = "wasm32",
    wasm_bindgen(js_name = "upstreamOnlyCommitsArePatchEquivalent")
)]
pub fn upstream_only_commits_are_patch_equivalent(cherry_mark_output: &str) -> bool {
    orca_core::git_upstream_status::upstream_only_commits_are_patch_equivalent(cherry_mark_output)
}

/// Which Pi-compatible agent a launch command starts: `"omp"` for OMP
/// (`omp` / `omp.sh`), else `"pi"`. The relay uses this to target the managed
/// extension dir for the actual agent being launched.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "detectPiAgentKindFromCommand"))]
pub fn detect_pi_agent_kind_from_command(command: Option<String>) -> String {
    match orca_text::pi_agent_kind::detect_pi_agent_kind_from_command(command.as_deref()) {
        orca_text::pi_agent_kind::PiAgentKind::Omp => "omp".to_string(),
        orca_text::pi_agent_kind::PiAgentKind::Pi => "pi".to_string(),
    }
}

// --- RENDERER workspace-name seam ----------------------------------------
// The renderer's workspace-name/seed preview helpers (the shared TS impl was
// deleted). These are PREVIEW/seed derivations — the main process runs the
// authoritative worktree-name sanitizer at create time, and every consumer
// already falls back to a valid seed, so a null/empty during the wasm boot
// window degrades to a less-descriptive (never broken) name.

/// Slugify free text into a git-ref-safe workspace seed.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "slugifyForWorkspaceName"))]
pub fn slugify_for_workspace_name(input: &str) -> String {
    orca_text::workspace_name::slugify_for_workspace_name(input)
}

/// Title → slug suggestion for a linked work item (TS takes `{ title }`; the
/// wrapper passes `.title`).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "getLinkedWorkItemSuggestedName"))]
pub fn get_linked_work_item_suggested_name(title: &str) -> String {
    orca_text::workspace_name::get_linked_work_item_suggested_name(title)
}

/// Combined Linear identifier+title workspace seed (dedup-aware).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "getLinearIssueWorkspaceName"))]
pub fn get_linear_issue_workspace_name(identifier: &str, title: &str) -> String {
    orca_text::workspace_name::get_linear_issue_workspace_name(identifier, title)
}

/// Display+seed for a linked work item as `{displayName, seedName}` JSON, or
/// `undefined` when no git-safe seed derives. Input is the work item as JSON.
#[cfg_attr(
    target_arch = "wasm32",
    wasm_bindgen(js_name = "getLinkedWorkItemWorkspaceName")
)]
pub fn get_linked_work_item_workspace_name(item_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(item_json).ok()?;
    let item = work_item_from_value(&value)?;
    orca_text::workspace_name::get_linked_work_item_workspace_name(&item)
        .map(|name| intent_name_to_json(&name))
}

/// First-create intent display+seed as `{displayName, seedName}` JSON, or
/// `undefined`. Input is `{sourceText?, workItem?, fallbackName?}` JSON.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "getWorkspaceIntentName"))]
pub fn get_workspace_intent_name(args_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let args = orca_text::workspace_name::WorkspaceIntentArgs {
        source_text: value.get("sourceText").and_then(|v| v.as_str()).map(str::to_string),
        work_item: value.get("workItem").and_then(work_item_from_value),
        fallback_name: value.get("fallbackName").and_then(|v| v.as_str()).map(str::to_string),
    };
    orca_text::workspace_name::get_workspace_intent_name(&args).map(|name| intent_name_to_json(&name))
}

/// `JSON.stringify` of the TS `WorkspaceIntentName` (camelCase keys).
fn intent_name_to_json(name: &orca_text::workspace_name::WorkspaceIntentName) -> String {
    serde_json::json!({ "displayName": name.display_name, "seedName": name.seed_name }).to_string()
}

/// Rebuild the pure `WorkspaceIntentWorkItem` from its TS JSON shape — the same
/// field mapping the parity dispatch uses (`type`→kind, camelCase identifiers).
fn work_item_from_value(
    value: &serde_json::Value,
) -> Option<orca_text::workspace_name::WorkspaceIntentWorkItem> {
    use orca_text::workspace_name::{WorkItemType, WorkspaceIntentWorkItem};
    let object = value.as_object()?;
    let kind = match object.get("type").and_then(|v| v.as_str()) {
        Some("pr") => Some(WorkItemType::Pr),
        Some("mr") => Some(WorkItemType::Mr),
        Some("issue") => Some(WorkItemType::Issue),
        _ => None,
    };
    Some(WorkspaceIntentWorkItem {
        kind,
        number: object.get("number").and_then(|v| v.as_u64()).unwrap_or_default(),
        title: object.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        linear_identifier: object.get("linearIdentifier").and_then(|v| v.as_str()).map(str::to_string),
        jira_identifier: object.get("jiraIdentifier").and_then(|v| v.as_str()).map(str::to_string),
    })
}

// --- commit-message spawn planner (RENDERER diagnostic; main runs it via napi) -
// Pure "agent choice + prompt -> spawn plan"; the renderer's dry-run preview
// checks the SAME Rust planner the main process runs, so the two never drift.

/// Plan a commit-message generation as `{ok:true, plan:{binary,args,stdinPayload,
/// label}} | {ok:false, error}` JSON (the TS `CommitMessagePlanResult` union).
/// Input is the `CommitMessagePlanInput` object as JSON + the prompt.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "planCommitMessageGeneration"))]
pub fn plan_commit_message_generation_json(plan_input_json: &str, prompt: &str) -> String {
    commit_message_plan_result_to_json(plan_input_json, prompt)
}

/// Resolve the spawn binary + prefix args from an optional command override, as
/// `{ok:true, binary, prefixArgs} | {ok:false, error}` JSON.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "planAgentBinary"))]
pub fn plan_agent_binary_json(default_binary: &str, command_override: Option<String>) -> String {
    plan_agent_binary_result_to_json(default_binary, command_override.as_deref())
}

fn commit_message_plan_result_to_json(plan_input_json: &str, prompt: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(plan_input_json) else {
        return serde_json::json!({ "ok": false, "error": "Invalid plan input JSON." }).to_string();
    };
    let input = orca_agents::CommitMessagePlanInput {
        agent_id: value.get("agentId").and_then(|v| v.as_str()).unwrap_or_default(),
        model: value.get("model").and_then(|v| v.as_str()).unwrap_or_default(),
        thinking_level: value.get("thinkingLevel").and_then(|v| v.as_str()),
        custom_agent_command: value.get("customAgentCommand").and_then(|v| v.as_str()),
        agent_command_override: value.get("agentCommandOverride").and_then(|v| v.as_str()),
        agent_args: value.get("agentArgs").and_then(|v| v.as_str()),
    };
    match orca_agents::plan_commit_message_generation(&input, prompt) {
        // TS always emits stdinPayload as an explicit string|null (never absent).
        Ok(plan) => serde_json::json!({
            "ok": true,
            "plan": {
                "binary": plan.binary,
                "args": plan.args,
                "stdinPayload": plan.stdin_payload,
                "label": plan.label,
            }
        })
        .to_string(),
        Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
    }
}

fn plan_agent_binary_result_to_json(default_binary: &str, command_override: Option<&str>) -> String {
    match orca_agents::plan_agent_binary(default_binary, command_override) {
        Ok((binary, prefix_args)) => {
            serde_json::json!({ "ok": true, "binary": binary, "prefixArgs": prefix_args }).to_string()
        }
        Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
    }
}

/// Build the PR-fields generation prompt (TS `buildPullRequestFieldsPrompt`); the
/// renderer's dry-run preview dialog runs this. `context_json` is the
/// `PullRequestDraftContext` object; returns the prompt string.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "buildPullRequestFieldsPrompt"))]
pub fn build_pull_request_fields_prompt_json(context_json: &str, custom_prompt: &str) -> String {
    orca_agents::build_pull_request_fields_prompt(&parse_pull_request_context(context_json), custom_prompt)
}

/// Parse an agent's PR-fields JSON reply (TS `parseGeneratedPullRequestFields`) as
/// `{ok:true, fields:{base,title,body,draft}} | {ok:false, error}` JSON. Exported for
/// parity/surface symmetry (the renderer only calls build; parse runs in main via napi).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "parseGeneratedPullRequestFields"))]
pub fn parse_generated_pull_request_fields_json(raw: &str, fallback_json: &str) -> String {
    let fallback = parse_pull_request_context(fallback_json);
    match orca_agents::parse_generated_pull_request_fields(raw, &fallback) {
        Ok(fields) => serde_json::json!({
            "ok": true,
            "fields": { "base": fields.base, "title": fields.title, "body": fields.body, "draft": fields.draft }
        })
        .to_string(),
        Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
    }
}

/// Run one terminal quick-command helper by name over its JSON input, returning
/// JSON (TS `terminal-quick-commands.ts`). The renderer drives normalize + the
/// typed-object accessors through this — see `orca_agents::terminal_quick_command_json`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "terminalQuickCommandOp"))]
pub fn terminal_quick_command_op_json(function: &str, input_json: &str) -> String {
    let input = serde_json::from_str::<serde_json::Value>(input_json).unwrap_or(serde_json::Value::Null);
    orca_agents::terminal_quick_command_json::dispatch(function, &input).to_string()
}

/// Dispatch one TUI agent-startup plan builder by name over its camelCase JSON
/// (TS `tui-agent-startup.ts`). The renderer drives buildAgentStartupPlan /
/// …Resume… / …Draft… through this — see `orca_agents::tui_agent_startup_json`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "tuiAgentStartupOp"))]
pub fn tui_agent_startup_op_json(function: &str, input_json: &str) -> String {
    let input = serde_json::from_str::<serde_json::Value>(input_json).unwrap_or(serde_json::Value::Null);
    orca_agents::tui_agent_startup_json::dispatch(function, &input).to_string()
}

/// Aggregate pure-module dispatch — the relay/renderer twin of the napi
/// `orcaDispatch`, running the IDENTICAL registry so output is byte-identical.
/// `input_json` empty/invalid → JSON null (a no-arg call). Returns the module's
/// JSON result, or an `__dispatch_error__` object for an unregistered module.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "orcaDispatch"))]
pub fn orca_dispatch(module: &str, function: &str, input_json: &str) -> String {
    let value =
        serde_json::from_str::<serde_json::Value>(input_json).unwrap_or(serde_json::Value::Null);
    match orca_dispatch::dispatch(module, function, &value) {
        Some(v) => v.to_string(),
        None => {
            serde_json::json!({ "__dispatch_error__": format!("unknown module {module}") }).to_string()
        }
    }
}

fn parse_pull_request_context(context_json: &str) -> orca_agents::PullRequestDraftContext {
    let value = serde_json::from_str::<serde_json::Value>(context_json).unwrap_or(serde_json::Value::Null);
    let str_field = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let bool_field = |key: &str| value.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    orca_agents::PullRequestDraftContext {
        branch: value.get("branch").and_then(|v| v.as_str()).map(str::to_string),
        base: str_field("base"),
        branch_changed_by_preparation: bool_field("branchChangedByPreparation"),
        current_title: str_field("currentTitle"),
        current_body: str_field("currentBody"),
        current_draft: bool_field("currentDraft"),
        commit_summary: str_field("commitSummary"),
        change_summary: str_field("changeSummary"),
        patch: str_field("patch"),
    }
}

// --- async relay git-executor bridge (wasm only) --------------------------
// The relay's multi-round git ops run the SAME orca-git logic the main process
// runs, but the main process uses the napi "A bridge" (block_on a JS promise on
// a libuv worker thread) — impossible in single-threaded wasm, where blocking
// would deadlock the event loop that must resolve the promise. Here the logic is
// driven through `AsyncGitRunner`, awaiting the relay's JS git executor via
// wasm_bindgen_futures. The result/error classification MUST match the napi
// bridge (native/orca-node/src/git_executor_bridge.rs `run_impl`) so relay
// output — including error messages — is byte-identical to the main process.
#[cfg(target_arch = "wasm32")]
mod async_bridge {
    use orca_git::runner::{AsyncGitRunner, GitError, GitOutput};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    /// Wraps the relay's JS git executor,
    /// `(args: string[], stdin: string | null) => Promise<{stdout, stderr, exitCode}>`.
    /// The executor MUST resolve (never reject) for a git process that spawned and
    /// exited — carrying its `exitCode` — so a non-zero exit classifies exactly
    /// like the native `ProcessGitRunner` (some callers read a non-zero code as
    /// data). A promise rejection (or a synchronous throw) means the spawn failed.
    pub struct WasmGitExecutor {
        pub callback: js_sys::Function,
    }

    impl AsyncGitRunner for WasmGitExecutor {
        async fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<GitOutput, GitError> {
            let args_array = js_sys::Array::new();
            for arg in args {
                args_array.push(&JsValue::from_str(arg));
            }
            let stdin_value = match stdin {
                Some(text) => JsValue::from_str(text),
                None => JsValue::NULL,
            };
            // A synchronous throw (before a promise is returned) is a spawn failure.
            let returned = self
                .callback
                .call2(&JsValue::NULL, &args_array, &stdin_value)
                .map_err(spawn_failure)?;
            match JsFuture::from(js_sys::Promise::from(returned)).await {
                Ok(output) => classify_bridge_output(&output),
                // A promise rejection is also a spawn failure (git never exited).
                Err(err) => Err(spawn_failure(err)),
            }
        }
    }

    /// `{stdout, stderr, exitCode}` → `GitOutput` / `GitError`, matching the napi
    /// bridge's `run_impl` classification exactly.
    fn classify_bridge_output(output: &JsValue) -> Result<GitOutput, GitError> {
        let stdout = string_field(output, "stdout");
        let stderr = string_field(output, "stderr");
        let exit_code = js_sys::Reflect::get(output, &JsValue::from_str("exitCode"))
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0) as i32;
        if exit_code == 0 {
            return Ok(GitOutput { stdout, stderr });
        }
        // Carry git's stderr in `message` so orca-git's classifiers/normalizers
        // (which read GitError.message) see the real diagnostic; fall back to the
        // exit-code form only when stderr is empty (the napi bridge's fallback,
        // also the missing-tracking-ref signal classified by code, not message).
        let message = if stderr.trim().is_empty() {
            format!("git exited with {:?}", Some(exit_code))
        } else {
            stderr.clone()
        };
        Err(GitError { code: Some(exit_code), message, stdout, stderr })
    }

    fn string_field(object: &JsValue, key: &str) -> String {
        js_sys::Reflect::get(object, &JsValue::from_str(key))
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_default()
    }

    /// A spawn failure (synchronous throw or promise rejection): mirror the napi
    /// bridge's `failed to spawn git: {err}` message (code `None`) so downstream
    /// error handling is identical.
    fn spawn_failure(err: JsValue) -> GitError {
        let detail = err
            .dyn_ref::<js_sys::Error>()
            .map(|error| String::from(error.message()))
            .or_else(|| err.as_string())
            .unwrap_or_else(|| format!("{err:?}"));
        GitError::from_message(format!("failed to spawn git: {detail}"))
    }
}

/// Relay twin of the napi `resolve_git_remote_rebase_source_via_executor`: drive
/// orca-git's read-only rebase-source resolver (`git remote` → longest matching
/// remote → `check-ref-format`) over the relay's async JS git executor. Resolves
/// `{remoteName, branchName, displayName}` JSON; rejects with the RAW resolver
/// message (the resolver never normalizes — the TS caller keeps its outer
/// `normalizeGitErrorMessage(err, 'pull')`), preserved as a JS `Error` so
/// `error.message` reads it.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = "resolveGitRemoteRebaseSourceViaExecutor")]
pub async fn resolve_git_remote_rebase_source_via_executor(
    executor: js_sys::Function,
    base_ref: String,
) -> Result<String, JsValue> {
    let runner = async_bridge::WasmGitExecutor { callback: executor };
    match orca_git::rebase_source::resolve_git_remote_rebase_source_async(&runner, &base_ref).await {
        Ok(source) => {
            Ok(orca_git::status_result::git_remote_rebase_source_to_json(&source).to_string())
        }
        Err(err) => Err(js_sys::Error::new(&err.message).into()),
    }
}

/// Relay twin of the napi `get_upstream_status_via_executor` (EXPLICIT publish
/// target): drive orca-git's upstream/ahead-behind status — `check-ref-format` →
/// `rev-parse` verify → (conditional) `rev-list` → (conditional) cherry-mark
/// `log` — over the relay's async JS git executor, with the data-dependent
/// decisions and the no-upstream swallow + error normalization owned by Rust.
/// Resolves the `GitUpstreamStatus` JSON (exact TS shape); rejects with the
/// already-normalized message (preserved as a JS `Error`). The JS-boundary
/// "Invalid PR push target …" shape guard stays in the relay's TS caller.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = "getUpstreamStatusViaExecutor")]
pub async fn get_upstream_status_via_executor(
    executor: js_sys::Function,
    remote_name: String,
    branch_name: String,
    remote_url: Option<String>,
) -> Result<String, JsValue> {
    let runner = async_bridge::WasmGitExecutor { callback: executor };
    let target = orca_git::push_target::GitPushTarget { remote_name, branch_name, remote_url };
    match orca_git::upstream::get_publish_target_upstream_status_async(&runner, &target).await {
        Ok(status) => Ok(orca_git::status_result::git_upstream_status_to_json(&status).to_string()),
        Err(err) => Err(js_sys::Error::new(&err.message).into()),
    }
}
