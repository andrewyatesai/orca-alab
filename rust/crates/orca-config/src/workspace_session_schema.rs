//! Tolerant validation of the persisted workspace-session JSON, ported from
//! `src/shared/workspace-session-schema.ts`.
//!
//! The session blob is written by older builds and read by newer ones, so a
//! field-type flip or truncated write must never reach the renderer. This is the
//! single "reject and fall back to defaults" boundary: structurally invalid blobs
//! collapse to an error string, while individual tolerated quirks (an unknown
//! `launchAgent`, a corrupted `lastVisitedAt` timestamp, oversized browser
//! history) are repaired in place rather than failing the whole session.
//!
//! Port notes — this mirrors the zod schema's *accept/reject* decisions and its
//! explicit `.preprocess`/`.transform`/`.catch` repairs over tolerant
//! `serde_json::Value`. Two deliberate, observable-behaviour-preserving
//! deviations from zod: (1) we do not silently strip unrecognized keys from the
//! output (nothing on the read path depends on stripping, and preserving them is
//! closer to the "tolerate future fields" policy); (2) error strings carry a
//! field path plus a short message rather than zod's exact wording. The
//! `browserUrlHistory` repair uses a best-effort URL canonicalization because no
//! WHATWG URL parser exists in this crate tier — faithful for the common
//! http(s) case, approximate for exotic URLs (documented at the helper).

use orca_agents::is_tui_agent;
use orca_core::terminal_tab_id::is_valid_terminal_tab_id;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Cap on persisted browser history entries (mirrors
/// `workspace-session-browser-history.ts`'s `MAX_BROWSER_HISTORY_ENTRIES`).
pub const MAX_BROWSER_HISTORY_ENTRIES: usize = 200;

/// Discriminated result so callers fall back to defaults without a try/catch.
#[derive(Clone, Debug, PartialEq)]
pub enum ParsedWorkspaceSession {
    Ok(Value),
    Err(String),
}

impl ParsedWorkspaceSession {
    pub fn is_ok(&self) -> bool {
        matches!(self, ParsedWorkspaceSession::Ok(_))
    }

    pub fn value(&self) -> Option<&Value> {
        match self {
            ParsedWorkspaceSession::Ok(value) => Some(value),
            ParsedWorkspaceSession::Err(_) => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            ParsedWorkspaceSession::Err(message) => Some(message),
            ParsedWorkspaceSession::Ok(_) => None,
        }
    }
}

/// Validate raw JSON as a workspace session. On success the value is the input
/// with the explicit repairs applied; on failure a compact `path: message`.
pub fn parse_workspace_session(raw: &Value) -> ParsedWorkspaceSession {
    match validate_session(raw) {
        Ok(()) => ParsedWorkspaceSession::Ok(build_output(raw)),
        Err(issue) => {
            // Keep the error compact — only the first divergent field is actionable.
            let path =
                if issue.path.is_empty() { "<root>".to_string() } else { issue.path.join(".") };
            ParsedWorkspaceSession::Err(format!("{path}: {}", issue.message))
        }
    }
}

// ─── Issue + path plumbing ──────────────────────────────────────────

struct Issue {
    path: Vec<String>,
    message: String,
}

fn at(path: &[String], message: &str) -> Issue {
    Issue { path: path.to_vec(), message: message.to_string() }
}

fn push(path: &[String], segment: &str) -> Vec<String> {
    let mut next = path.to_vec();
    next.push(segment.to_string());
    next
}

type VResult = Result<(), Issue>;

// ─── Primitive validators ───────────────────────────────────────────

fn v_string(value: &Value, path: &[String]) -> VResult {
    if value.is_string() {
        Ok(())
    } else {
        Err(at(path, "Expected string"))
    }
}

fn v_number(value: &Value, path: &[String]) -> VResult {
    // Parsed JSON numbers are always finite, matching zod `z.number()` here.
    if value.is_number() {
        Ok(())
    } else {
        Err(at(path, "Expected number"))
    }
}

fn v_boolean(value: &Value, path: &[String]) -> VResult {
    if value.is_boolean() {
        Ok(())
    } else {
        Err(at(path, "Expected boolean"))
    }
}

fn v_literal_true(value: &Value, path: &[String]) -> VResult {
    if value == &Value::Bool(true) {
        Ok(())
    } else {
        Err(at(path, "Invalid literal value, expected true"))
    }
}

fn v_terminal_tab_id(value: &Value, path: &[String]) -> VResult {
    match value.as_str() {
        Some(text) if is_valid_terminal_tab_id(text) => Ok(()),
        Some(_) => Err(at(path, "terminal tab id must not contain \":\"")),
        None => Err(at(path, "Expected string")),
    }
}

fn v_nullable<F>(value: &Value, path: &[String], inner: F) -> VResult
where
    F: Fn(&Value, &[String]) -> VResult,
{
    if value.is_null() {
        Ok(())
    } else {
        inner(value, path)
    }
}

fn v_enum(value: &Value, path: &[String], allowed: &[&str]) -> VResult {
    match value.as_str() {
        Some(text) if allowed.contains(&text) => Ok(()),
        _ => Err(at(path, "Invalid enum value")),
    }
}

// ─── Container validators ───────────────────────────────────────────

fn v_array<F>(value: &Value, path: &[String], item: F) -> VResult
where
    F: Fn(&Value, &[String]) -> VResult,
{
    let Some(array) = value.as_array() else {
        return Err(at(path, "Expected array"));
    };
    for (index, element) in array.iter().enumerate() {
        item(element, &push(path, &index.to_string()))?;
    }
    Ok(())
}

fn v_record<F>(
    value: &Value,
    path: &[String],
    key_check: Option<fn(&str) -> bool>,
    validate_value: F,
) -> VResult
where
    F: Fn(&Value, &[String]) -> VResult,
{
    let Some(object) = value.as_object() else {
        return Err(at(path, "Expected object"));
    };
    for (key, element) in object {
        let child = push(path, key);
        if let Some(check) = key_check {
            if !check(key.as_str()) {
                return Err(at(&child, "Invalid record key"));
            }
        }
        validate_value(element, &child)?;
    }
    Ok(())
}

fn req_field<F>(obj: &Map<String, Value>, key: &str, path: &[String], validate: F) -> VResult
where
    F: Fn(&Value, &[String]) -> VResult,
{
    let child = push(path, key);
    match obj.get(key) {
        Some(value) => validate(value, &child),
        None => Err(Issue { path: child, message: "Required".to_string() }),
    }
}

fn opt_field<F>(obj: &Map<String, Value>, key: &str, path: &[String], validate: F) -> VResult
where
    F: Fn(&Value, &[String]) -> VResult,
{
    // JSON has no `undefined`; an absent key satisfies `.optional()`, while a
    // present `null` is validated by `validate` (so non-nullable optionals reject it).
    match obj.get(key) {
        Some(value) => validate(value, &push(path, key)),
        None => Ok(()),
    }
}

fn as_object<'a>(value: &'a Value, path: &[String]) -> Result<&'a Map<String, Value>, Issue> {
    value.as_object().ok_or_else(|| at(path, "Expected object"))
}

// ─── Recursive layout-node unions (z.lazy) ──────────────────────────

fn v_terminal_pane_layout_node(value: &Value, path: &[String]) -> VResult {
    // union(leaf, split): accept if either branch validates.
    if v_pane_leaf(value, path).is_ok() || v_pane_split(value, path).is_ok() {
        return Ok(());
    }
    Err(at(path, "Invalid terminal pane layout node"))
}

fn v_pane_leaf(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    if object.get("type").and_then(Value::as_str) != Some("leaf") {
        return Err(at(&push(path, "type"), "Invalid literal value, expected \"leaf\""));
    }
    req_field(object, "leafId", path, v_string)
}

fn v_pane_split(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    if object.get("type").and_then(Value::as_str) != Some("split") {
        return Err(at(&push(path, "type"), "Invalid literal value, expected \"split\""));
    }
    req_field(object, "direction", path, |v, p| v_enum(v, p, &["vertical", "horizontal"]))?;
    req_field(object, "first", path, v_terminal_pane_layout_node)?;
    req_field(object, "second", path, v_terminal_pane_layout_node)?;
    opt_field(object, "ratio", path, v_number)
}

fn v_tab_group_layout_node(value: &Value, path: &[String]) -> VResult {
    if v_group_leaf(value, path).is_ok() || v_group_split(value, path).is_ok() {
        return Ok(());
    }
    Err(at(path, "Invalid tab group layout node"))
}

fn v_group_leaf(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    if object.get("type").and_then(Value::as_str) != Some("leaf") {
        return Err(at(&push(path, "type"), "Invalid literal value, expected \"leaf\""));
    }
    req_field(object, "groupId", path, v_string)
}

fn v_group_split(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    if object.get("type").and_then(Value::as_str) != Some("split") {
        return Err(at(&push(path, "type"), "Invalid literal value, expected \"split\""));
    }
    req_field(object, "direction", path, |v, p| v_enum(v, p, &["horizontal", "vertical"]))?;
    req_field(object, "first", path, v_tab_group_layout_node)?;
    req_field(object, "second", path, v_tab_group_layout_node)?;
    opt_field(object, "ratio", path, v_number)
}

// ─── Leaf object schemas ────────────────────────────────────────────

fn v_terminal_layout_snapshot(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "root", path, |v, p| v_nullable(v, p, v_terminal_pane_layout_node))?;
    req_field(object, "activeLeafId", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "expandedLeafId", path, |v, p| v_nullable(v, p, v_string))?;
    opt_field(object, "ptyIdsByLeafId", path, |v, p| v_record(v, p, None, v_string))?;
    opt_field(object, "buffersByLeafId", path, |v, p| v_record(v, p, None, v_string))?;
    opt_field(object, "scrollbackRefsByLeafId", path, |v, p| v_record(v, p, None, v_string))?;
    opt_field(object, "titlesByLeafId", path, |v, p| v_record(v, p, None, v_string))
}

fn v_terminal_tab(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "id", path, v_terminal_tab_id)?;
    req_field(object, "ptyId", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "worktreeId", path, v_string)?;
    req_field(object, "title", path, v_string)?;
    opt_field(object, "defaultTitle", path, v_string)?;
    opt_field(object, "generatedTitle", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "customTitle", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "color", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "sortOrder", path, v_number)?;
    req_field(object, "createdAt", path, v_number)?;
    opt_field(object, "generation", path, v_number)?;
    // launchAgent: `.catch(undefined)` — never fails the parse (repaired in output).
    Ok(())
}

fn v_tab(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "id", path, v_string)?;
    req_field(object, "entityId", path, v_string)?;
    req_field(object, "groupId", path, v_string)?;
    req_field(object, "worktreeId", path, v_string)?;
    req_field(object, "contentType", path, |v, p| {
        v_enum(v, p, &["terminal", "editor", "diff", "conflict-review", "browser"])
    })?;
    req_field(object, "label", path, v_string)?;
    opt_field(object, "generatedLabel", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "customLabel", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "color", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "sortOrder", path, v_number)?;
    req_field(object, "createdAt", path, v_number)?;
    opt_field(object, "isPreview", path, v_boolean)?;
    opt_field(object, "isPinned", path, v_boolean)
}

fn v_tab_group(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "id", path, v_string)?;
    req_field(object, "worktreeId", path, v_string)?;
    req_field(object, "activeTabId", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "tabOrder", path, |v, p| v_array(v, p, v_string))?;
    opt_field(object, "recentTabIds", path, |v, p| v_array(v, p, v_string))
}

fn v_persisted_open_file(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "filePath", path, v_string)?;
    req_field(object, "relativePath", path, v_string)?;
    req_field(object, "worktreeId", path, v_string)?;
    req_field(object, "language", path, v_string)?;
    opt_field(object, "isPreview", path, v_boolean)?;
    opt_field(object, "runtimeEnvironmentId", path, |v, p| v_nullable(v, p, v_string))?;
    opt_field(object, "dirtyDraftContent", path, v_string)
}

fn v_browser_load_error(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "code", path, v_number)?;
    req_field(object, "description", path, v_string)?;
    req_field(object, "validatedUrl", path, v_string)
}

const BROWSER_VIEWPORT_PRESET_IDS: [&str; 7] =
    ["mobile-s", "mobile-m", "mobile-l", "tablet", "laptop", "laptop-l", "desktop"];

fn v_browser_workspace(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "id", path, v_string)?;
    req_field(object, "worktreeId", path, v_string)?;
    opt_field(object, "label", path, v_string)?;
    opt_field(object, "sessionProfileId", path, |v, p| v_nullable(v, p, v_string))?;
    opt_field(object, "activePageId", path, |v, p| v_nullable(v, p, v_string))?;
    opt_field(object, "pageIds", path, |v, p| v_array(v, p, v_string))?;
    req_field(object, "url", path, v_string)?;
    req_field(object, "title", path, v_string)?;
    req_field(object, "loading", path, v_boolean)?;
    req_field(object, "faviconUrl", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "canGoBack", path, v_boolean)?;
    req_field(object, "canGoForward", path, v_boolean)?;
    req_field(object, "loadError", path, |v, p| v_nullable(v, p, v_browser_load_error))?;
    req_field(object, "createdAt", path, v_number)
}

fn v_browser_page(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "id", path, v_string)?;
    req_field(object, "workspaceId", path, v_string)?;
    req_field(object, "worktreeId", path, v_string)?;
    req_field(object, "url", path, v_string)?;
    req_field(object, "title", path, v_string)?;
    req_field(object, "loading", path, v_boolean)?;
    req_field(object, "faviconUrl", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "canGoBack", path, v_boolean)?;
    req_field(object, "canGoForward", path, v_boolean)?;
    req_field(object, "loadError", path, |v, p| v_nullable(v, p, v_browser_load_error))?;
    req_field(object, "createdAt", path, v_number)?;
    opt_field(object, "viewportPresetId", path, |v, p| {
        v_nullable(v, p, |vv, pp| v_enum(vv, pp, &BROWSER_VIEWPORT_PRESET_IDS))
    })
}

fn v_browser_history_entry(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "url", path, v_string)?;
    req_field(object, "normalizedUrl", path, v_string)?;
    req_field(object, "title", path, v_string)?;
    req_field(object, "lastVisitedAt", path, v_number)?;
    req_field(object, "visitCount", path, v_number)
}

// ─── Top-level session ──────────────────────────────────────────────

fn v_session(value: &Value, path: &[String]) -> VResult {
    let object = as_object(value, path)?;
    req_field(object, "activeRepoId", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "activeWorktreeId", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "activeTabId", path, |v, p| v_nullable(v, p, v_string))?;
    req_field(object, "tabsByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_array(vv, pp, v_terminal_tab))
    })?;
    req_field(object, "terminalLayoutsByTabId", path, |v, p| {
        v_record(v, p, Some(is_valid_terminal_tab_id as fn(&str) -> bool), v_terminal_layout_snapshot)
    })?;
    opt_field(object, "activeWorktreeIdsOnShutdown", path, |v, p| v_array(v, p, v_string))?;
    opt_field(object, "openFilesByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_array(vv, pp, v_persisted_open_file))
    })?;
    opt_field(object, "activeFileIdByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_nullable(vv, pp, v_string))
    })?;
    opt_field(object, "markdownFrontmatterVisible", path, |v, p| {
        v_record(v, p, None, v_boolean)
    })?;
    opt_field(object, "browserTabsByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_array(vv, pp, v_browser_workspace))
    })?;
    opt_field(object, "browserPagesByWorkspace", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_array(vv, pp, v_browser_page))
    })?;
    opt_field(object, "activeBrowserTabIdByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_nullable(vv, pp, v_string))
    })?;
    opt_field(object, "activeTabTypeByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_enum(vv, pp, &["terminal", "editor", "browser"]))
    })?;
    opt_field(object, "browserUrlHistory", path, |v, p| {
        v_array(v, p, v_browser_history_entry)
    })?;
    opt_field(object, "activeTabIdByWorktree", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_nullable(vv, pp, v_string))
    })?;
    opt_field(object, "unifiedTabs", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_array(vv, pp, v_tab))
    })?;
    opt_field(object, "tabGroups", path, |v, p| {
        v_record(v, p, None, |vv, pp| v_array(vv, pp, v_tab_group))
    })?;
    opt_field(object, "tabGroupLayouts", path, |v, p| {
        v_record(v, p, None, v_tab_group_layout_node)
    })?;
    opt_field(object, "activeGroupIdByWorktree", path, |v, p| v_record(v, p, None, v_string))?;
    opt_field(object, "activeConnectionIdsAtShutdown", path, |v, p| v_array(v, p, v_string))?;
    opt_field(object, "remoteSessionIdsByTabId", path, |v, p| {
        v_record(v, p, Some(is_valid_terminal_tab_id as fn(&str) -> bool), v_string)
    })?;
    // lastVisitedAtByWorktreeId: `.preprocess` cleans an object before validation;
    // a present non-object (incl. null) is rejected, an object always passes.
    opt_field(object, "lastVisitedAtByWorktreeId", path, |v, p| {
        if v.is_object() {
            Ok(())
        } else {
            Err(at(p, "Expected object"))
        }
    })?;
    opt_field(object, "defaultTerminalTabsAppliedByWorktreeId", path, |v, p| {
        v_record(v, p, None, v_literal_true)
    })
}

fn validate_session(value: &Value) -> VResult {
    v_session(value, &[])
}

// ─── Output repairs (applied on success) ────────────────────────────

fn build_output(raw: &Value) -> Value {
    let Some(object) = raw.as_object() else {
        return raw.clone();
    };
    let mut output = object.clone();
    if let Some(tabs) = output.get("tabsByWorktree").cloned() {
        output.insert("tabsByWorktree".to_string(), repair_tabs_by_worktree(&tabs));
    }
    if let Some(last_visited) = output.get("lastVisitedAtByWorktreeId").cloned() {
        output.insert("lastVisitedAtByWorktreeId".to_string(), clean_last_visited(&last_visited));
    }
    if let Some(history) = output.get("browserUrlHistory").cloned() {
        output.insert("browserUrlHistory".to_string(), normalize_browser_history(&history));
    }
    Value::Object(output)
}

fn repair_tabs_by_worktree(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut output = Map::new();
    for (worktree_id, tabs) in object {
        let repaired = match tabs.as_array() {
            Some(array) => Value::Array(array.iter().map(repair_launch_agent).collect()),
            None => tabs.clone(),
        };
        output.insert(worktree_id.clone(), repaired);
    }
    Value::Object(output)
}

/// `launchAgent: z.custom(isTuiAgent).catch(undefined)` — drop a stale/unknown
/// agent id so it never reaches the restored tab, keeping a known one intact.
fn repair_launch_agent(tab: &Value) -> Value {
    let Some(object) = tab.as_object() else {
        return tab.clone();
    };
    let mut output = object.clone();
    // Compute the decision before mutating so the `get` borrow ends first.
    let drop_agent = match output.get("launchAgent") {
        Some(agent) => !agent.as_str().is_some_and(is_tui_agent),
        None => false,
    };
    if drop_agent {
        output.remove("launchAgent");
    }
    Value::Object(output)
}

/// `.preprocess` repair: keep only finite, non-negative numeric timestamps so a
/// single corrupted value can't poison the whole-session parse.
fn clean_last_visited(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut output = Map::new();
    for (key, candidate) in object {
        if let Some(number) = candidate.as_f64() {
            if number.is_finite() && number >= 0.0 {
                // Preserve the original number node (int vs float representation).
                output.insert(key.clone(), candidate.clone());
            }
        }
    }
    Value::Object(output)
}

// ─── Browser-history normalization (mirrors workspace-session-browser-history) ─

struct HistoryCandidate {
    entry: Value,
    safe_url: String,
    key: String,
    last_visited_at: f64,
}

/// Dedupe by normalized URL, keep most-recent visits, cap at
/// `MAX_BROWSER_HISTORY_ENTRIES`. Mirrors `normalizeBrowserHistoryEntries`.
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the result never exceeds the history cap.
#[cfg_attr(trust_verify, trust::ensures(|out: &Value|
    out.as_array().map_or(true, |entries| entries.len() <= MAX_BROWSER_HISTORY_ENTRIES)))]
fn normalize_browser_history(value: &Value) -> Value {
    let Some(array) = value.as_array() else {
        return value.clone();
    };
    let mut candidates: Vec<HistoryCandidate> = array
        .iter()
        .map(|entry| {
            let raw_url = entry.get("url").and_then(Value::as_str).unwrap_or_default();
            let safe_url = redact_kagi_session_token(raw_url);
            let key = normalize_browser_history_url(&safe_url);
            let last_visited_at =
                entry.get("lastVisitedAt").and_then(Value::as_f64).unwrap_or(0.0);
            HistoryCandidate { entry: entry.clone(), safe_url, key, last_visited_at }
        })
        .collect();
    // Stable sort by recency desc — persisted history may not be in recency order.
    candidates.sort_by(|a, b| {
        b.last_visited_at
            .partial_cmp(&a.last_visited_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seen: HashSet<String> = HashSet::new();
    let mut normalized: Vec<Value> = Vec::new();
    for candidate in candidates {
        if seen.contains(&candidate.key) {
            continue;
        }
        seen.insert(candidate.key.clone());
        let mut entry = candidate.entry.as_object().cloned().unwrap_or_default();
        entry.insert("url".to_string(), Value::String(candidate.safe_url));
        entry.insert("normalizedUrl".to_string(), Value::String(candidate.key));
        normalized.push(Value::Object(entry));
        if normalized.len() >= MAX_BROWSER_HISTORY_ENTRIES {
            break;
        }
    }
    Value::Array(normalized)
}

/// Best-effort Kagi private-session token redaction. A faithful WHATWG parse
/// isn't available in this tier; non-matching URLs pass through unchanged (the
/// common case), matching the TS `catch -> return rawUrl` path.
fn redact_kagi_session_token(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    let is_kagi_search = lower.starts_with("https://kagi.com/search")
        || lower.starts_with("https://www.kagi.com/search");
    if !is_kagi_search {
        return url.to_string();
    }
    let Some(query_start) = url.find('?') else {
        return url.to_string();
    };
    let (base, tail) = url.split_at(query_start);
    let (query, fragment) = match tail.find('#') {
        Some(hash) => (&tail[1..hash], &tail[hash..]),
        None => (&tail[1..], ""),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| pair.split('=').next().unwrap_or("") != "token")
        .filter(|pair| !pair.is_empty())
        .collect();
    if kept.is_empty() {
        format!("{base}{fragment}")
    } else {
        format!("{base}?{}{fragment}", kept.join("&"))
    }
}

/// Best-effort URL canonicalization for the dedupe key: lowercase scheme + host
/// and strip a single trailing `/`. Falls back to the lowercased input, matching
/// the TS `catch` branch for unparseable URLs. Approximate vs WHATWG for exotic
/// URLs (percent-encoding, default ports), faithful for the common http(s) case.
fn normalize_browser_history_url(url: &str) -> String {
    let safe = redact_kagi_session_token(url);
    match split_scheme_authority(&safe) {
        Some((scheme, authority, rest)) => {
            let mut normalized = format!(
                "{}{}{}",
                scheme.to_ascii_lowercase(),
                authority.to_ascii_lowercase(),
                rest
            );
            if normalized.ends_with('/') {
                normalized.pop();
            }
            normalized
        }
        None => safe.to_ascii_lowercase(),
    }
}

fn split_scheme_authority(url: &str) -> Option<(&str, &str, &str)> {
    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end + 3];
    let after = &url[scheme_end + 3..];
    let authority_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    Some((scheme, &after[..authority_end], &after[authority_end..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_a_minimal_valid_session() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {},
            "terminalLayoutsByTabId": {}
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn accepts_a_fully_populated_session_with_optional_fields() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": "repo1",
            "activeWorktreeId": "repo1::/path/wt1",
            "activeTabId": "tab1",
            "tabsByWorktree": {
                "repo1::/path/wt1": [{
                    "id": "tab1",
                    "ptyId": "daemon-session-abc",
                    "worktreeId": "repo1::/path/wt1",
                    "title": "bash",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1_700_000_000_000_i64
                }]
            },
            "terminalLayoutsByTabId": {
                "tab1": {
                    "root": {
                        "type": "split",
                        "direction": "vertical",
                        "first": { "type": "leaf", "leafId": "pane:1" },
                        "second": { "type": "leaf", "leafId": "pane:2" }
                    },
                    "activeLeafId": "pane:1",
                    "expandedLeafId": null,
                    "ptyIdsByLeafId": { "pane:1": "daemon-session-A" }
                }
            },
            "activeWorktreeIdsOnShutdown": ["repo1::/path/wt1"]
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn preserves_a_valid_launch_agent_on_a_terminal_tab() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {
                "wt": [{
                    "id": "tab1",
                    "ptyId": null,
                    "worktreeId": "wt",
                    "title": "codex",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1,
                    "launchAgent": "codex"
                }]
            },
            "terminalLayoutsByTabId": {}
        }));
        assert!(result.is_ok());
        let value = result.value().unwrap();
        assert_eq!(value["tabsByWorktree"]["wt"][0]["launchAgent"], json!("codex"));
    }

    #[test]
    fn drops_an_unknown_launch_agent_without_failing_the_whole_session() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {
                "wt": [{
                    "id": "tab1",
                    "ptyId": null,
                    "worktreeId": "wt",
                    "title": "bash",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1,
                    "launchAgent": "some-retired-agent"
                }]
            },
            "terminalLayoutsByTabId": {}
        }));
        assert!(result.is_ok());
        let value = result.value().unwrap();
        assert!(value["tabsByWorktree"]["wt"][0].get("launchAgent").is_none());
    }

    #[test]
    fn rejects_a_session_where_pty_id_is_a_number_schema_drift() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {
                "wt": [{
                    "id": "tab1",
                    "ptyId": 42,
                    "worktreeId": "wt",
                    "title": "bash",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 0
                }]
            },
            "terminalLayoutsByTabId": {}
        }));
        assert!(!result.is_ok());
        assert!(result.error().unwrap().contains("ptyId"));
    }

    #[test]
    fn preserves_generated_terminal_title_fields_for_persistence_hydration() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": "wt",
            "activeTabId": "tab1",
            "tabsByWorktree": {
                "wt": [{
                    "id": "tab1",
                    "ptyId": null,
                    "worktreeId": "wt",
                    "title": "Claude working",
                    "defaultTitle": "Terminal 1",
                    "generatedTitle": "Refactor auth",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 0
                }]
            },
            "terminalLayoutsByTabId": {},
            "unifiedTabs": {
                "wt": [{
                    "id": "tab1",
                    "entityId": "tab1",
                    "groupId": "group1",
                    "worktreeId": "wt",
                    "contentType": "terminal",
                    "label": "Claude working",
                    "generatedLabel": "Refactor auth",
                    "customLabel": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 0
                }]
            }
        }));
        assert!(result.is_ok());
        let value = result.value().unwrap();
        assert_eq!(value["tabsByWorktree"]["wt"][0]["generatedTitle"], json!("Refactor auth"));
        assert_eq!(value["unifiedTabs"]["wt"][0]["generatedLabel"], json!("Refactor auth"));
    }

    #[test]
    fn rejects_a_session_with_missing_required_top_level_fields() {
        let result = parse_workspace_session(&json!({ "activeRepoId": null }));
        assert!(!result.is_ok());
    }

    #[test]
    fn rejects_a_truncated_json_object() {
        let result = parse_workspace_session(&json!({}));
        assert!(!result.is_ok());
    }

    #[test]
    fn rejects_non_object_input() {
        assert!(!parse_workspace_session(&Value::Null).is_ok());
        assert!(!parse_workspace_session(&json!("garbage")).is_ok());
        assert!(!parse_workspace_session(&json!(42)).is_ok());
    }

    #[test]
    fn drops_bad_last_visited_at_entries_rather_than_failing_the_session() {
        // Number.NaN / Number.POSITIVE_INFINITY are not representable in JSON; they
        // persist as null (the projection used here), which — like the bad string
        // and the negative timestamp — the cleaner drops.
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {},
            "terminalLayoutsByTabId": {},
            "lastVisitedAtByWorktreeId": {
                "good": 1_700_000_000_000_i64,
                "nan": null,
                "infinite": null,
                "negative": -5,
                "string": "nope"
            }
        }));
        assert!(result.is_ok());
        let value = result.value().unwrap();
        assert_eq!(value["lastVisitedAtByWorktreeId"], json!({ "good": 1_700_000_000_000_i64 }));
    }

    #[test]
    fn accepts_default_tab_idempotency_markers() {
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {},
            "terminalLayoutsByTabId": {},
            "defaultTerminalTabsAppliedByWorktreeId": { "repo1::/path/wt1": true }
        }));
        assert!(result.is_ok());
        let value = result.value().unwrap();
        assert_eq!(
            value["defaultTerminalTabsAppliedByWorktreeId"],
            json!({ "repo1::/path/wt1": true })
        );
    }

    #[test]
    fn caps_oversized_browser_history_while_parsing_legacy_workspace_sessions() {
        let history: Vec<Value> = (0..500)
            .map(|index| {
                json!({
                    "url": format!("https://example.com/{index}"),
                    "normalizedUrl": format!("https://example.com/{index}"),
                    "title": format!("Example {index}"),
                    "lastVisitedAt": 1_700_000_000_000_i64 - index as i64,
                    "visitCount": 1
                })
            })
            .collect();
        let result = parse_workspace_session(&json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {},
            "terminalLayoutsByTabId": {},
            "browserUrlHistory": history
        }));
        assert!(result.is_ok());
        let value = result.value().unwrap();
        let entries = value["browserUrlHistory"].as_array().unwrap();
        assert_eq!(entries.len(), MAX_BROWSER_HISTORY_ENTRIES);
        assert_eq!(entries.last().unwrap()["url"], json!("https://example.com/199"));
    }
}
