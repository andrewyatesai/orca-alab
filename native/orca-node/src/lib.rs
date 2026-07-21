//! Node-API addon exposing the ATERM-backed `orca_terminal::HeadlessTerminal`
//! to the Electron main/daemon process. Mirrors the surface
//! `src/main/daemon/headless-emulator.ts` needs (write / resize / snapshot /
//! cwd / cursor / mouse-modes / serialize) so it can be swapped in behind the
//! `ORCA_RUST_TERMINAL` flag. This is the real JS -> napi -> aterm path.
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

// The IO-tier "A bridge": run orca-git's sync GitRunner logic over an async JS
// git executor (Rust drives, JS executes — SSH-safe).
mod git_executor_bridge;

const DEFAULT_SCROLLBACK: u32 = 5000;

/// One OSC-8 hyperlink run in a snapshot. Field names marshal to camelCase
/// (`startCol`/`endCol`), matching the renderer's `TerminalOscLinkRange`.
/// `endCol` is exclusive.
#[napi(object)]
pub struct JsOscLinkRange {
    pub row: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub uri: String,
}

#[napi(js_name = "HeadlessTerminal")]
pub struct JsHeadlessTerminal {
    // Option so dispose() can drop the engine (grid + tiered scrollback)
    // deterministically instead of waiting for the GC finalizer; disposed
    // calls return empty defaults.
    inner: Option<orca_terminal::HeadlessTerminal>,
}

// Every export carries catch_unwind: a Rust panic unwinding across the extern-C
// napi boundary aborts the whole daemon/Electron-main process (all sessions);
// catch_unwind converts it into a JS exception the caller can contain per-session.
#[napi]
impl JsHeadlessTerminal {
    /// JS passes (cols, rows); the engine takes (rows, cols) internally.
    #[napi(constructor, catch_unwind)]
    pub fn new(cols: u32, rows: u32, scrollback: Option<u32>) -> Self {
        let scrollback = scrollback.unwrap_or(DEFAULT_SCROLLBACK) as usize;
        Self {
            inner: Some(orca_terminal::HeadlessTerminal::with_scrollback(
                rows as usize,
                cols as usize,
                scrollback,
            )),
        }
    }

    #[napi(catch_unwind)]
    pub fn write(&mut self, data: Buffer) {
        if let Some(inner) = self.inner.as_mut() {
            inner.process(&data);
        }
    }

    #[napi(catch_unwind)]
    pub fn resize(&mut self, cols: u32, rows: u32) {
        if let Some(inner) = self.inner.as_mut() {
            inner.resize(rows as usize, cols as usize);
        }
    }

    /// Visible grid rows (trailing blanks trimmed) — the render snapshot.
    #[napi(catch_unwind)]
    pub fn snapshot(&self) -> Vec<String> {
        self.inner.as_ref().map(|t| t.snapshot()).unwrap_or_default()
    }

    #[napi(catch_unwind)]
    pub fn scrollback_len(&self) -> u32 {
        self.inner.as_ref().map_or(0, |t| t.scrollback_len() as u32)
    }

    #[napi(catch_unwind)]
    pub fn clear_scrollback(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.clear_scrollback();
        }
    }

    /// Replayable ANSI for the snapshot (scrollback + visible grid). `&mut` so
    /// the adapter can memoise the result by content-generation + cursor.
    /// `scrollbackRows` caps the prepended history (omit = all, 0 = viewport-only),
    /// matching `@xterm/addon-serialize`'s `serialize({scrollback})`.
    #[napi(catch_unwind)]
    pub fn serialize_ansi(&mut self, scrollback_rows: Option<u32>) -> String {
        self.inner
            .as_mut()
            .map(|t| t.serialize_ansi(scrollback_rows.map(|n| n as usize)))
            .unwrap_or_default()
    }

    /// Scrollback history only (no grid/cursor framing) — what the daemon stores
    /// in `scrollbackAnsi` so alt-screen sessions restore their scrollback.
    /// `maxRows` caps to the most-recent N history lines (omit = all).
    #[napi(catch_unwind)]
    pub fn serialize_scrollback_ansi(&self, max_rows: Option<u32>) -> String {
        self.inner
            .as_ref()
            .map(|t| t.serialize_scrollback_ansi(max_rows.map(|n| n as usize)))
            .unwrap_or_default()
    }

    /// OSC-8 hyperlink ranges over the serialized window (the same `scrollbackRows`
    /// of history `serializeAnsi` prepends, then the visible grid), so restored
    /// snapshots keep clickable links.
    #[napi(catch_unwind)]
    pub fn osc_link_ranges(&self, scrollback_rows: Option<u32>) -> Vec<JsOscLinkRange> {
        let Some(inner) = self.inner.as_ref() else {
            return Vec::new();
        };
        inner
            .osc_link_ranges(scrollback_rows.map(|n| n as usize))
            .into_iter()
            .map(|r| JsOscLinkRange {
                row: r.row as u32,
                start_col: r.start_col as u32,
                end_col: r.end_col as u32,
                uri: r.uri,
            })
            .collect()
    }

    /// Window title (OSC 0/2), or null when unset — feeds the snapshot's
    /// `lastTitle` for agent detection.
    #[napi(catch_unwind)]
    pub fn title(&self) -> Option<String> {
        self.inner.as_ref().and_then(|t| t.title())
    }

    #[napi(catch_unwind)]
    pub fn cwd(&self) -> Option<String> {
        self.inner
            .as_ref()
            .and_then(|t| t.cwd().map(str::to_string))
    }

    /// `[row, col]` cursor position.
    #[napi(catch_unwind)]
    pub fn cursor(&self) -> Vec<u32> {
        let (r, c) = self.inner.as_ref().map_or((0, 0), |t| t.cursor());
        vec![r as u32, c as u32]
    }

    #[napi(catch_unwind)]
    pub fn mouse_tracking(&self) -> String {
        use orca_terminal::MouseTracking::{Any, Button, Normal, None as MtNone, X10};
        // Capitalised variant names — the daemon factory's RUST_MOUSE_MODE map
        // keys on these (None/X10/Normal/Button/Any).
        match self.inner.as_ref().map(|t| t.mouse_tracking()) {
            None | Some(MtNone) => "None",
            Some(X10) => "X10",
            Some(Normal) => "Normal",
            Some(Button) => "Button",
            Some(Any) => "Any",
        }
        .to_string()
    }

    #[napi(catch_unwind)]
    pub fn sgr_mouse(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.sgr_mouse())
    }

    #[napi(catch_unwind)]
    pub fn sgr_pixels(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.sgr_pixels())
    }

    #[napi(catch_unwind)]
    pub fn is_alternate_screen(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.is_alternate_screen())
    }

    #[napi(catch_unwind)]
    pub fn bracketed_paste(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.bracketed_paste())
    }

    #[napi(catch_unwind)]
    pub fn application_cursor(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.application_cursor())
    }

    /// Drop the engine now. The daemon churns through many sessions, so freeing
    /// the multi-MB grid/scrollback must not wait for a GC finalizer.
    #[napi(catch_unwind)]
    pub fn dispose(&mut self) {
        self.inner = None;
    }
}

#[napi(catch_unwind)]
pub fn engine() -> String {
    "aterm".to_string()
}

// --- orca-git: the verified status/numstat/line-count parsers, exposed to JS
// via this same .node. They are the SOLE implementation in the main process
// (the duplicated TS parsers were deleted after the dual-run parity phase; the
// relay runs the same core via wasm). JSON strings are the marshalling format
// (the status_result.rs builders match the original TS shapes verbatim,
// omitting None fields). ---

/// Streaming `git status --porcelain=v2 --branch` parser — the chunked path the
/// daemon feeds raw stdout bytes. Ported from the (since deleted)
/// `StatusPorcelainParser` in `src/main/git/status-porcelain-parser.ts`.
#[napi(js_name = "GitStatusParser")]
pub struct JsGitStatusParser {
    // Option because into_result consumes the parser; result() take()s it.
    inner: Option<orca_git::status_stream::StatusPorcelainParser>,
}

#[napi]
impl JsGitStatusParser {
    #[napi(constructor, catch_unwind)]
    pub fn new() -> Self {
        Self {
            inner: Some(orca_git::status_stream::StatusPorcelainParser::new()),
        }
    }

    /// Feed one raw chunk. Returns true once the changed-entry count exceeds
    /// `limit` (0 disables the cap), signaling the caller to stop git.
    #[napi(catch_unwind)]
    pub fn update(&mut self, chunk: Buffer, limit: u32) -> bool {
        match self.inner.as_mut() {
            Some(parser) => parser.update(&chunk, limit as usize),
            // Already consumed by result(); nothing more to scan.
            None => false,
        }
    }

    /// Flush a final record with no trailing newline (e.g. when git exits).
    #[napi(catch_unwind)]
    pub fn finish(&mut self) {
        if let Some(parser) = self.inner.as_mut() {
            parser.finish();
        }
    }

    /// Consume the parser and return the status-result JSON. After the first call
    /// the parser is gone; a second call returns a valid empty result, never a panic.
    #[napi(catch_unwind)]
    pub fn result(&mut self, limit: u32) -> String {
        let result = match self.inner.take() {
            Some(parser) => parser.into_result(limit as usize),
            None => orca_git::status_stream::StatusPorcelainParser::new().into_result(limit as usize),
        };
        orca_git::status_result::status_parse_result_to_json(&result).to_string()
    }
}

/// One-shot status scan (the relay entry point): the cap is applied DURING the
/// scan, so `entries` is bounded by `limit` instead of materialize-then-truncate.
#[napi(catch_unwind)]
pub fn parse_status_porcelain(stdout: Buffer, limit: u32) -> String {
    let result = orca_git::status_stream::parse_status_porcelain(&stdout, limit as usize);
    orca_git::status_result::status_parse_result_to_json(&result).to_string()
}

/// `git diff --numstat` (text or `-z`) parsed to `{path: {added?, removed?}}`.
#[napi(catch_unwind)]
pub fn parse_numstat(stdout: Buffer) -> String {
    let entries = orca_git::numstat::parse_numstat(&stdout);
    orca_git::status_result::numstat_to_json(&entries).to_string()
}

/// `git worktree list --porcelain` (or the `-z` NUL form) parsed to the
/// `GitWorktreeInfo[]` JSON the TS `parseWorktreeList` produces (`isSparse`
/// omitted when false).
#[napi(catch_unwind)]
pub fn parse_worktree_list(output: String, nul_delimited: bool) -> String {
    let worktrees = orca_git::worktree::parse_worktree_list(&output, nul_delimited);
    orca_git::worktree::worktree_list_to_json(&worktrees).to_string()
}

/// NUL-delimited `git log` output (in `GIT_HISTORY_COMMIT_FORMAT`) parsed to the
/// `GitHistoryItem[]` JSON the TS `parseGitHistoryLog` produces.
#[napi(catch_unwind)]
pub fn parse_git_history_log(stdout: String) -> String {
    let items = orca_git::git_history_log_parser::parse_git_history_log(&stdout);
    orca_git::git_history_log_parser::git_history_log_to_json(&items).to_string()
}

/// Count additions for an untracked file's contents: null for binary, 0 for empty,
/// else the trailing-newline-aware line count.
#[napi(catch_unwind)]
pub fn count_additions_in_buffer(bytes: Buffer) -> Option<u32> {
    orca_git::line_count::count_additions_in_buffer(&bytes)
}

/// Validate a persisted push target's *value* rules — the substantive
/// path-traversal-safety check for a remote name / branch name / optional GitHub
/// URL that gets replayed into `git push`. Returns the TS-identical error message,
/// or `None` when valid. The `unknown`→typed guards (and their `Invalid PR push
/// target …` messages) stay in JS; this shares `orca_core` with the parity harness.
#[napi(catch_unwind)]
pub fn validate_git_push_target_rules(
    remote_name: String,
    branch_name: String,
    remote_url: Option<String>,
) -> Option<String> {
    orca_core::git_push_target::validate_git_push_target(
        &remote_name,
        &branch_name,
        remote_url.as_deref(),
    )
    .err()
}

/// Approximate added/removed line counts; returns the line-stats JSON, or null
/// for the large-input guard.
#[napi(catch_unwind)]
pub fn compute_line_stats(original: String, modified: String, status: String) -> Option<String> {
    orca_git::line_count::compute_line_stats(&original, &modified, &status)
        .map(|stats| orca_git::status_result::line_stats_to_json(Some(stats)).to_string())
}

/// Decode a git C-quoted (octal-escaped) path. Raw (unquoted) input passes through.
/// js_name keeps the capital-Q the TS `decodeGitCQuotedPath` uses (napi would
/// otherwise lowercase "cquoted").
#[napi(js_name = "decodeGitCQuotedPath", catch_unwind)]
pub fn decode_git_cquoted_path(value: String) -> String {
    orca_core::git_cquoted_path::decode_git_cquoted_path(&value)
}

/// True when a git fetch/pull error message means the remote ref does not
/// exist (an expected state, not a failure). The `unknown`→message extraction
/// stays at the JS boundary.
#[napi(catch_unwind)]
pub fn is_missing_remote_ref_git_error(message: String) -> bool {
    orca_git::fetch_error_classification::is_missing_remote_ref_git_error(&message)
}

fn clone_path_flavor(platform: &str) -> orca_core::cross_platform_path::PathFlavor {
    if platform == "win32" {
        orca_core::cross_platform_path::PathFlavor::Windows
    } else {
        orca_core::cross_platform_path::PathFlavor::Posix
    }
}

/// Derive the default `git clone` folder name from a URL; throws the
/// TS-identical message for names that would escape the destination.
#[napi(catch_unwind)]
pub fn derive_clone_repo_name_from_url(url: String) -> napi::Result<String> {
    orca_git::repo_clone_path::derive_clone_repo_name_from_url(&url)
        .map_err(napi::Error::from_reason)
}

/// Derive `<destination>/<repoName>` for `git clone`, validating the
/// destination is absolute and the result stays inside it. `platform` is the
/// Node `process.platform` value ("win32" → Windows path rules, else POSIX).
#[napi(catch_unwind)]
pub fn derive_validated_clone_path(
    url: String,
    destination: String,
    platform: String,
) -> napi::Result<String> {
    orca_git::repo_clone_path::derive_validated_clone_path(
        &url,
        &destination,
        clone_path_flavor(&platform),
    )
    .map_err(napi::Error::from_reason)
}

/// Stable key for comparing clone paths (WSL-UNC aware). Callers pass an
/// already-resolved absolute path — the cwd `resolve()` stays in JS.
#[napi(catch_unwind)]
pub fn get_clone_path_comparison_key(clone_path: String) -> String {
    orca_git::repo_clone_path::get_clone_path_comparison_key(&clone_path)
}

/// Normalise a git remote-operation error message into the user-facing string.
/// `message` is `None` for a non-Error throw (fixed fallback); `operation` is
/// `"push" | "pull" | "fetch" | "upstream"` (unrecognised → `None`), matching
/// the TS default-parameter behaviour. Mirrors the wasm export the relay runs.
#[napi(catch_unwind)]
pub fn normalize_git_error_message(message: Option<String>, operation: Option<String>) -> String {
    let operation = match operation.as_deref() {
        Some("push") => Some(orca_text::git_remote_error::GitRemoteOperation::Push),
        Some("pull") => Some(orca_text::git_remote_error::GitRemoteOperation::Pull),
        Some("fetch") => Some(orca_text::git_remote_error::GitRemoteOperation::Fetch),
        Some("upstream") => Some(orca_text::git_remote_error::GitRemoteOperation::Upstream),
        _ => None,
    };
    orca_text::git_remote_error::normalize_git_error_message(message.as_deref(), operation)
}

/// True only for clearly-no-upstream signals (an expected state, gated on a
/// `fatal:` prefix). `None` message → false (a non-Error throw in TS).
#[napi(catch_unwind)]
pub fn is_no_upstream_error(message: Option<String>) -> bool {
    orca_text::git_remote_error::is_no_upstream_error(message.as_deref())
}

/// Scrub credentials embedded in a git URL within `message` (keeps SSH
/// user-info; strips `user:password@` on any scheme + HTTP(S) token-only
/// `user@`).
#[napi(catch_unwind)]
pub fn strip_credentials_from_message(message: String) -> String {
    orca_text::git_remote_error::strip_credentials_from_message(&message)
}

/// Which Pi-compatible agent a launch command starts: `"omp"` for OMP
/// (`omp` / `omp.sh`), else `"pi"`.
#[napi(catch_unwind)]
pub fn detect_pi_agent_kind_from_command(command: Option<String>) -> String {
    match orca_text::pi_agent_kind::detect_pi_agent_kind_from_command(command.as_deref()) {
        orca_text::pi_agent_kind::PiAgentKind::Omp => "omp".to_string(),
        orca_text::pi_agent_kind::PiAgentKind::Pi => "pi".to_string(),
    }
}

/// Skill markdown frontmatter summary (`name`/`description`) as JSON.
#[napi(catch_unwind)]
pub fn summarize_skill_markdown(markdown: String) -> String {
    let summary = orca_text::skill_metadata::summarize_skill_markdown(&markdown);
    let mut out = serde_json::Map::new();
    if let Some(name) = summary.name {
        out.insert("name".to_string(), serde_json::Value::String(name));
    }
    if let Some(description) = summary.description {
        out.insert("description".to_string(), serde_json::Value::String(description));
    }
    serde_json::Value::Object(out).to_string()
}

/// Plan a commit-message generation as the TS `CommitMessagePlanResult` union
/// (`{ok:true, plan:{binary,args,stdinPayload,label}} | {ok:false, error}`) JSON.
/// Input is the `CommitMessagePlanInput` object as JSON + the prompt.
#[napi(catch_unwind)]
pub fn plan_commit_message_generation(plan_input_json: String, prompt: String) -> String {
    commit_message_plan_result_to_json(&plan_input_json, &prompt)
}

/// Resolve the spawn binary + prefix args from an optional command override, as
/// `{ok:true, binary, prefixArgs} | {ok:false, error}` JSON.
#[napi(catch_unwind)]
pub fn plan_agent_binary(default_binary: String, command_override: Option<String>) -> String {
    plan_agent_binary_result_to_json(&default_binary, command_override.as_deref())
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

/// Build the PR-fields generation prompt (TS `buildPullRequestFieldsPrompt`).
/// `context_json` is the `PullRequestDraftContext` object; returns the prompt string.
#[napi(catch_unwind)]
pub fn build_pull_request_fields_prompt(context_json: String, custom_prompt: String) -> String {
    orca_agents::build_pull_request_fields_prompt(&parse_pull_request_context(&context_json), &custom_prompt)
}

/// Parse an agent's PR-fields JSON reply (TS `parseGeneratedPullRequestFields`) as
/// `{ok:true, fields:{base,title,body,draft}} | {ok:false, error}` JSON; `fallback_json`
/// supplies the current PR fields for missing/blank values (the shim throws on `!ok`).
#[napi(catch_unwind)]
pub fn parse_generated_pull_request_fields(raw: String, fallback_json: String) -> String {
    let fallback = parse_pull_request_context(&fallback_json);
    match orca_agents::parse_generated_pull_request_fields(&raw, &fallback) {
        Ok(fields) => serde_json::json!({
            "ok": true,
            "fields": { "base": fields.base, "title": fields.title, "body": fields.body, "draft": fields.draft }
        })
        .to_string(),
        Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
    }
}

/// Run one terminal quick-command helper by name over its JSON input, returning
/// JSON (TS `terminal-quick-commands.ts`). One entry covers normalize + the
/// typed-object accessors — see `orca_agents::terminal_quick_command_json`.
#[napi(catch_unwind)]
pub fn terminal_quick_command_op(function: String, input_json: String) -> String {
    let input = serde_json::from_str::<serde_json::Value>(&input_json).unwrap_or(serde_json::Value::Null);
    orca_agents::terminal_quick_command_json::dispatch(&function, &input).to_string()
}

/// Dispatch one TUI agent-startup plan builder by name over its camelCase JSON
/// (TS `tui-agent-startup.ts`). Covers buildAgentStartupPlan / …Resume… / …Draft…
/// — see `orca_agents::tui_agent_startup_json`. Returns `"null"` for a null plan.
#[napi(catch_unwind)]
pub fn tui_agent_startup_op(function: String, input_json: String) -> String {
    let input = serde_json::from_str::<serde_json::Value>(&input_json).unwrap_or(serde_json::Value::Null);
    orca_agents::tui_agent_startup_json::dispatch(&function, &input).to_string()
}

/// Build a `PullRequestDraftContext` from its camelCase JSON (string fields default
/// to "", `branch` nullable → `None`); shared by prompt-build + reply-parse.
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

/// Parse an OpenSSH config file into `SshConfigHost[]` JSON (the same shape TS
/// `parseSshConfig` returns). `home` is the `~`-expansion base the caller reads
/// from `os.homedir()` — kept explicit so the Rust core stays pure.
#[napi(catch_unwind)]
pub fn parse_ssh_config(content: String, home: String) -> String {
    let hosts = orca_ssh::parse_ssh_config(&content, &home);
    let array: Vec<serde_json::Value> = hosts.iter().map(ssh_config_host_to_json).collect();
    serde_json::Value::Array(array).to_string()
}

fn ssh_config_host_to_json(host: &orca_ssh::SshConfigHost) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("host".into(), serde_json::Value::from(host.host.clone()));
    if let Some(v) = &host.hostname {
        map.insert("hostname".into(), serde_json::Value::from(v.clone()));
    }
    if let Some(v) = host.port {
        map.insert("port".into(), serde_json::Value::from(v));
    }
    if let Some(v) = &host.user {
        map.insert("user".into(), serde_json::Value::from(v.clone()));
    }
    if let Some(v) = &host.identity_file {
        map.insert("identityFile".into(), serde_json::Value::from(v.clone()));
    }
    if let Some(v) = &host.identity_agent {
        map.insert("identityAgent".into(), serde_json::Value::from(v.clone()));
    }
    if let Some(v) = host.identities_only {
        map.insert("identitiesOnly".into(), serde_json::Value::from(v));
    }
    if let Some(v) = host.gssapi_authentication {
        map.insert("gssapiAuthentication".into(), serde_json::Value::from(v));
    }
    if let Some(v) = &host.proxy_command {
        map.insert("proxyCommand".into(), serde_json::Value::from(v.clone()));
    }
    if let Some(v) = host.proxy_use_fdpass {
        map.insert("proxyUseFdpass".into(), serde_json::Value::from(v));
    }
    if let Some(v) = &host.proxy_jump {
        map.insert("proxyJump".into(), serde_json::Value::from(v.clone()));
    }
    serde_json::Value::Object(map)
}

/// Validate raw session JSON as a `WorkspaceSessionState`, returning the TS
/// `ParsedWorkspaceSession` union (`{ok:true, value} | {ok:false, error}`) JSON.
/// Same parse/repair `src/main/persistence.ts` relied on the deleted shared zod
/// schema for — the Rust orca-config port is now the sole impl.
#[napi(catch_unwind)]
pub fn parse_workspace_session(raw_json: String) -> String {
    // JSON.stringify always yields valid JSON; Null models a non-object input,
    // which the parser rejects exactly as zod did.
    let raw: serde_json::Value = serde_json::from_str(&raw_json).unwrap_or(serde_json::Value::Null);
    match orca_config::parse_workspace_session(&raw) {
        orca_config::ParsedWorkspaceSession::Ok(value) => {
            serde_json::json!({ "ok": true, "value": value }).to_string()
        }
        orca_config::ParsedWorkspaceSession::Err(error) => {
            serde_json::json!({ "ok": false, "error": error }).to_string()
        }
    }
}

#[napi(catch_unwind)]
pub fn git_engine() -> &'static str {
    "orca-git"
}

/// Aggregate pure-module dispatch: the single napi entry every ported module
/// ships through (no per-module export). `input_json` empty/invalid → JSON null
/// (a no-arg call). Returns the module's JSON result, or an `__dispatch_error__`
/// object when no Rust dispatch is registered for `module`.
#[napi(catch_unwind)]
pub fn orca_dispatch(module: String, function: String, input_json: String) -> String {
    let value =
        serde_json::from_str::<serde_json::Value>(&input_json).unwrap_or(serde_json::Value::Null);
    match orca_dispatch::dispatch(&module, &function, &value) {
        Some(v) => v.to_string(),
        None => {
            serde_json::json!({ "__dispatch_error__": format!("unknown module {module}") }).to_string()
        }
    }
}

// --- orca-runtime: the multi-agent orchestration store, exposed as a stateful
// class the main-process TS OrchestrationDb shim delegates to (the node:sqlite
// twin was deleted). JS-side nondeterminism (generated ids, ISO completion
// stamps, display strings) is passed IN by the shim; every other timestamp uses
// SQLite datetime('now') — byte-identical to what the deleted TS store wrote.
// Row methods marshal via the TS Row JSON (serde output matches types.ts). ---

use orca_runtime::orchestration::{NewMessage, OrchestrationDb};

fn napi_err<E: std::fmt::Display>(err: E) -> napi::Error {
    napi::Error::from_reason(err.to_string())
}

fn row_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[napi(js_name = "OrchestrationStore")]
pub struct JsOrchestrationStore {
    // Option so close() can drop the connection deterministically (WAL lock
    // release matters on Windows); calls after close() throw.
    inner: Option<OrchestrationDb>,
}

impl JsOrchestrationStore {
    fn store(&self) -> napi::Result<&OrchestrationDb> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("OrchestrationStore is closed"))
    }
}

#[napi]
impl JsOrchestrationStore {
    #[napi(constructor, catch_unwind)]
    pub fn new(path: String) -> napi::Result<Self> {
        let inner = if path == ":memory:" {
            OrchestrationDb::open_in_memory()
        } else {
            OrchestrationDb::open(&path)
        }
        .map_err(napi_err)?;
        Ok(Self { inner: Some(inner) })
    }

    // ---- messages ----

    #[napi(catch_unwind)]
    #[allow(clippy::too_many_arguments)]
    pub fn insert_message(
        &self,
        id: String,
        from_handle: String,
        to_handle: String,
        subject: String,
        body: String,
        message_type: String,
        priority: String,
        thread_id: Option<String>,
        payload: Option<String>,
        sender_pane_key: Option<String>,
    ) -> napi::Result<String> {
        let message = NewMessage { id, from_handle, to_handle, subject, body, message_type, priority, thread_id, payload, sender_pane_key };
        self.store()?.send_message(&message).map(|m| row_json(&m)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_message_by_id(&self, id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.get_message_by_id(&id).map_err(napi_err)?.map(|m| row_json(&m)))
    }

    #[napi(catch_unwind)]
    pub fn get_unread_messages(&self, handle: String, types: Option<Vec<String>>) -> napi::Result<String> {
        self.store()?.get_unread_messages(&handle, types.as_deref()).map(|m| row_json(&m)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_undelivered_unread_messages(&self, handle: String, types: Option<Vec<String>>) -> napi::Result<String> {
        self.store()?
            .get_undelivered_unread_messages(&handle, types.as_deref())
            .map(|m| row_json(&m))
            .map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_all_messages(&self, handle: String, limit: f64) -> napi::Result<String> {
        self.store()?.get_all_messages(&handle, limit as i64).map(|m| row_json(&m)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_all_messages_for_handle(&self, handle: String, limit: f64, types: Option<Vec<String>>) -> napi::Result<String> {
        self.store()?
            .get_all_messages_for_handle(&handle, limit as i64, types.as_deref())
            .map(|m| row_json(&m))
            .map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_inbox(&self, limit: f64) -> napi::Result<String> {
        self.store()?.get_inbox(limit as i64).map(|m| row_json(&m)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_thread_messages_for(&self, thread_id: String, to_handle: String, after_sequence: Option<f64>) -> napi::Result<String> {
        self.store()?
            .get_thread_messages_for(&thread_id, &to_handle, after_sequence.map(|n| n as i64))
            .map(|m| row_json(&m))
            .map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn mark_as_read(&self, ids: Vec<String>) -> napi::Result<()> {
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.store()?.mark_as_read(&refs).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn mark_as_read_and_delivered(&self, ids: Vec<String>) -> napi::Result<()> {
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.store()?.mark_as_read_and_delivered(&refs).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn convert_lifecycle_message_to_rejection(&self, message_id: String, reason: String) -> napi::Result<Option<String>> {
        Ok(self
            .store()?
            .convert_lifecycle_message_to_rejection(&message_id, &reason)
            .map_err(napi_err)?
            .map(|m| row_json(&m)))
    }

    #[napi(catch_unwind)]
    pub fn mark_as_delivered(&self, ids: Vec<String>) -> napi::Result<()> {
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.store()?.mark_as_delivered(&refs).map_err(napi_err)
    }

    // ---- tasks ----

    #[napi(catch_unwind)]
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        id: String,
        spec: String,
        parent_id: Option<String>,
        deps: Vec<String>,
        created_by: Option<String>,
        task_title: Option<String>,
        display_name: Option<String>,
    ) -> napi::Result<String> {
        let deps: Vec<&str> = deps.iter().map(String::as_str).collect();
        self.store()?
            .create_task(&id, &spec, parent_id.as_deref(), &deps, created_by.as_deref(), task_title.as_deref(), display_name.as_deref())
            .map(|t| row_json(&t))
            .map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_task(&self, id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.get_task(&id).map_err(napi_err)?.map(|t| row_json(&t)))
    }

    #[napi(catch_unwind)]
    pub fn list_tasks(&self, status: Option<String>) -> napi::Result<String> {
        self.store()?.list_tasks(status.as_deref()).map(|t| row_json(&t)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn list_tasks_with_dispatch(&self, status: Option<String>) -> napi::Result<String> {
        self.store()?.list_tasks_with_dispatch(status.as_deref()).map(|t| row_json(&t)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn update_task_status(
        &self,
        id: String,
        status: String,
        result: Option<String>,
        completed_at: Option<String>,
    ) -> napi::Result<Option<String>> {
        Ok(self
            .store()?
            .update_task_status(&id, &status, result.as_deref(), completed_at.as_deref())
            .map_err(napi_err)?
            .map(|t| row_json(&t)))
    }

    // ---- dispatch contexts ----

    #[napi(catch_unwind)]
    pub fn create_dispatch_context(&self, task_id: String, assignee_handle: String, id: String, assignee_pane_key: Option<String>) -> napi::Result<String> {
        self.store()?
            .create_dispatch_context(&task_id, &assignee_handle, &id, assignee_pane_key.as_deref())
            .map(|d| row_json(&d))
            .map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_dispatch_context(&self, task_id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.get_dispatch_context(&task_id).map_err(napi_err)?.map(|d| row_json(&d)))
    }

    #[napi(catch_unwind)]
    pub fn get_dispatch_context_by_id(&self, id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.dispatch_context_by_id(&id).map_err(napi_err)?.map(|d| row_json(&d)))
    }

    #[napi(catch_unwind)]
    pub fn get_active_dispatch_for_terminal(&self, handle: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.get_active_dispatch_for_terminal(&handle).map_err(napi_err)?.map(|d| row_json(&d)))
    }

    #[napi(catch_unwind)]
    pub fn get_latest_dispatch_for_terminal(&self, handle: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.get_latest_dispatch_for_terminal(&handle).map_err(napi_err)?.map(|d| row_json(&d)))
    }

    #[napi(catch_unwind)]
    pub fn complete_dispatch(&self, id: String) -> napi::Result<()> {
        self.store()?.complete_dispatch(&id).map(|_| ()).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn complete_active_dispatch_for_task(&self, task_id: String) -> napi::Result<()> {
        self.store()?.complete_active_dispatch_for_task(&task_id).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn fail_active_dispatch_for_task(&self, task_id: String, error: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.fail_active_dispatch_for_task(&task_id, &error).map_err(napi_err)?.map(|d| row_json(&d)))
    }

    #[napi(catch_unwind)]
    pub fn fail_dispatch(&self, id: String, error: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.fail_dispatch(&id, &error).map_err(napi_err)?.map(|d| row_json(&d)))
    }

    #[napi(catch_unwind)]
    pub fn record_heartbeat(&self, id: String, at: String) -> napi::Result<()> {
        self.store()?.record_heartbeat(&id, &at).map(|_| ()).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_stale_dispatches(&self, threshold_iso: String) -> napi::Result<String> {
        self.store()?.get_stale_dispatches(&threshold_iso).map(|d| row_json(&d)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn set_dispatch_timestamps(&self, id: String, dispatched_at: Option<String>, last_heartbeat_at: Option<String>) -> napi::Result<()> {
        self.store()?
            .set_dispatch_timestamps(&id, dispatched_at.as_deref(), last_heartbeat_at.as_deref())
            .map(|_| ())
            .map_err(napi_err)
    }

    // ---- decision gates ----

    #[napi(catch_unwind)]
    pub fn create_gate(&self, id: String, task_id: String, question: String, options: Vec<String>) -> napi::Result<String> {
        let options: Vec<&str> = options.iter().map(String::as_str).collect();
        self.store()?.create_gate(&id, &task_id, &question, &options).map(|g| row_json(&g)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn resolve_gate(&self, id: String, resolution: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.resolve_gate(&id, &resolution).map_err(napi_err)?.map(|g| row_json(&g)))
    }

    #[napi(catch_unwind)]
    pub fn timeout_gate(&self, id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.timeout_gate(&id).map_err(napi_err)?.map(|g| row_json(&g)))
    }

    #[napi(catch_unwind)]
    pub fn list_gates(&self, task_id: Option<String>, status: Option<String>) -> napi::Result<String> {
        self.store()?.list_gates(task_id.as_deref(), status.as_deref()).map(|g| row_json(&g)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_gate(&self, id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.gate_by_id(&id).map_err(napi_err)?.map(|g| row_json(&g)))
    }

    // ---- coordinator runs ----

    #[napi(catch_unwind)]
    pub fn create_coordinator_run(&self, id: String, spec: String, coordinator_handle: String, poll_interval_ms: Option<f64>) -> napi::Result<String> {
        self.store()?
            .create_coordinator_run(&id, &spec, &coordinator_handle, poll_interval_ms.map(|n| n as i64))
            .map(|r| row_json(&r))
            .map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn get_coordinator_run(&self, id: String) -> napi::Result<Option<String>> {
        Ok(self.store()?.coordinator_run_by_id(&id).map_err(napi_err)?.map(|r| row_json(&r)))
    }

    #[napi(catch_unwind)]
    pub fn update_coordinator_run(&self, id: String, status: String, completed_at: Option<String>) -> napi::Result<Option<String>> {
        Ok(self.store()?.update_coordinator_run(&id, &status, completed_at.as_deref()).map_err(napi_err)?.map(|r| row_json(&r)))
    }

    #[napi(catch_unwind)]
    pub fn get_active_coordinator_run(&self) -> napi::Result<Option<String>> {
        Ok(self.store()?.active_coordinator_run().map_err(napi_err)?.map(|r| row_json(&r)))
    }

    #[napi(catch_unwind)]
    pub fn get_active_coordinator_runs(&self) -> napi::Result<String> {
        self.store()?.active_coordinator_runs().map(|r| row_json(&r)).map_err(napi_err)
    }

    // ---- queries + lifecycle ----

    #[napi(catch_unwind)]
    pub fn get_idle_terminals(&self, exclude_handles: Vec<String>) -> napi::Result<String> {
        let refs: Vec<&str> = exclude_handles.iter().map(String::as_str).collect();
        self.store()?.get_idle_terminals(&refs).map(|h| row_json(&h)).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn reset_all(&self) -> napi::Result<()> {
        self.store()?.reset_all().map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn reset_tasks(&self) -> napi::Result<()> {
        self.store()?.reset_tasks().map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn reset_messages(&self) -> napi::Result<()> {
        self.store()?.reset_messages().map_err(napi_err)
    }

    /// Raw all-tables dump (real ids/timestamps) for the parity state harness.
    #[napi(catch_unwind)]
    pub fn dump_tables_json(&self) -> napi::Result<String> {
        self.store()?.dump_all_rows().map(|v| v.to_string()).map_err(napi_err)
    }

    #[napi(catch_unwind)]
    pub fn close(&mut self) {
        self.inner = None;
    }
}

/// Result of feeding a chunk to [`NdjsonParser`]: the complete lines to JSON-parse
/// (in order) plus the observed byte sizes of any oversized lines that were dropped.
#[napi(object)]
pub struct NdjsonFeedResult {
    /// Complete lines (newline-stripped, non-empty) in arrival order.
    pub lines: Vec<String>,
    /// Byte sizes of dropped oversized lines (one per oversized report).
    pub oversized: Vec<u32>,
}

/// Stateful NDJSON byte-budget line splitter (orca_net::NdjsonSplitter) — the OOM
/// guard for the daemon socket. `feed` returns complete lines for the caller to
/// JSON.parse; oversized lines are dropped + the stream resyncs at the next newline.
#[napi(js_name = "NdjsonParser")]
pub struct JsNdjsonParser {
    inner: orca_net::NdjsonSplitter,
}

#[napi]
impl JsNdjsonParser {
    #[napi(constructor, catch_unwind)]
    pub fn new(max_line_bytes: Option<u32>) -> Self {
        let max = max_line_bytes
            .map(|n| n as usize)
            .unwrap_or(orca_net::NDJSON_MAX_LINE_BYTES);
        Self {
            inner: orca_net::NdjsonSplitter::new(max),
        }
    }

    #[napi(catch_unwind)]
    pub fn feed(&mut self, chunk: String) -> NdjsonFeedResult {
        let mut lines = Vec::new();
        let mut oversized = Vec::new();
        for event in self.inner.feed_collect(&chunk) {
            match event {
                orca_net::NdjsonEvent::Line(line) => lines.push(line),
                orca_net::NdjsonEvent::Oversized { observed_bytes } => {
                    oversized.push(u32::try_from(observed_bytes).unwrap_or(u32::MAX));
                }
            }
        }
        NdjsonFeedResult { lines, oversized }
    }

    #[napi(catch_unwind)]
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}
