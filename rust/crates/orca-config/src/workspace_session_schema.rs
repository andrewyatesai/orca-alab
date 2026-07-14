//! Tolerant validation of the persisted workspace-session JSON, ported from
//! `src/shared/workspace-session-schema.ts` (zod 4.4.3).
//!
//! The session blob is written by older builds and read by newer ones, so a
//! field-type flip or truncated write must never reach the renderer. This is the
//! single "reject and fall back to defaults" boundary: structurally invalid blobs
//! collapse to an error string, while individual tolerated quirks (an unknown
//! `launchAgent` or `viewMode`, a corrupted `lastVisitedAt` timestamp, invalid
//! sleeping-agent records, oversized browser history) are repaired in place
//! rather than failing the whole session.
//!
//! Port notes — this mirrors zod's observable behavior exactly: unknown keys are
//! stripped wherever `z.object` strips them, and error strings reproduce zod
//! 4.4.3's en-locale wording (`Invalid input: expected string, received number`,
//! `Invalid key in record`, …). The only remaining approximation is the
//! `browserUrlHistory` URL canonicalization: no WHATWG URL parser exists in this
//! crate tier, so it is faithful for the common http(s) case and approximate for
//! exotic URLs (documented at the helper).

use orca_agents::is_tui_agent;
use orca_core::terminal_tab_id::is_valid_terminal_tab_id;
use serde_json::{json, Map, Value};
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

/// Validate raw JSON as a workspace session. On success the value is zod's
/// parse output (unknown keys stripped, repairs applied); on failure a compact
/// `path: message` built from the first zod issue.
pub fn parse_workspace_session(raw: &Value) -> ParsedWorkspaceSession {
    match p_session(Some(raw), &[]) {
        Ok(value) => ParsedWorkspaceSession::Ok(value),
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

/// Parsers take `Option<&Value>` so a missing key (JS `undefined`) flows through
/// the same code path as a present value, exactly like zod field parsing.
type PResult = Result<Value, Issue>;

// ─── zod 4.4.3 en-locale message builders ───────────────────────────

const MSG_INVALID_INPUT: &str = "Invalid input";
const MSG_INVALID_KEY: &str = "Invalid key in record";
const MSG_TAB_ID_REFINE: &str = "terminal tab id must not contain \":\"";

/// zod's `parsedType` name for the received value (`undefined` for a missing key).
fn received_of(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

fn invalid_type(expected: &str, value: Option<&Value>, path: &[String]) -> Issue {
    at(path, &format!("Invalid input: expected {expected}, received {}", received_of(value)))
}

fn invalid_option(allowed: &[&str], path: &[String]) -> Issue {
    let joined = allowed.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join("|");
    at(path, &format!("Invalid option: expected one of {joined}"))
}

fn too_small_chars(minimum: usize, path: &[String]) -> Issue {
    at(path, &format!("Too small: expected string to have >={minimum} characters"))
}

// ─── JS string/object semantics ─────────────────────────────────────

/// JS `String.prototype.trim` set: WhiteSpace (incl. U+FEFF) + LineTerminator.
/// Differs from Rust `char::is_whitespace` (which trims U+0085 but not U+FEFF).
fn is_js_trim_char(c: char) -> bool {
    matches!(c,
        '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}'
        | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}'
        | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_trim_char)
}

/// `.length` in TS counts UTF-16 code units, not chars.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Canonical JS array index (< 2^32-1): these keys are visited first, in
/// ascending numeric order, when iterating object properties.
fn js_array_index(key: &str) -> Option<u32> {
    if key == "0" {
        return Some(0);
    }
    if !key.as_bytes().first().is_some_and(|b| (b'1'..=b'9').contains(b)) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

/// JS own-property order: integer-like keys ascending, then insertion order.
/// Determines which entry zod reports first when several are invalid.
fn js_ordered_keys(map: &Map<String, Value>) -> Vec<&String> {
    let mut integer_keys: Vec<(u32, &String)> = Vec::new();
    let mut string_keys: Vec<&String> = Vec::new();
    for key in map.keys() {
        match js_array_index(key) {
            Some(index) => integer_keys.push((index, key)),
            None => string_keys.push(key),
        }
    }
    integer_keys.sort_by_key(|(index, _)| *index);
    integer_keys.into_iter().map(|(_, key)| key).chain(string_keys).collect()
}

// ─── Primitive parsers ──────────────────────────────────────────────

fn p_string(value: Option<&Value>, path: &[String]) -> PResult {
    match value {
        Some(v @ Value::String(_)) => Ok(v.clone()),
        other => Err(invalid_type("string", other, path)),
    }
}

fn p_string_min1(value: Option<&Value>, path: &[String]) -> PResult {
    let parsed = p_string(value, path)?;
    if parsed.as_str().is_some_and(str::is_empty) {
        return Err(too_small_chars(1, path));
    }
    Ok(parsed)
}

fn p_number(value: Option<&Value>, path: &[String]) -> PResult {
    // Parsed JSON numbers are always finite, so plain `z.number()` needs no
    // NaN/Infinity check here.
    match value {
        Some(v @ Value::Number(_)) => Ok(v.clone()),
        other => Err(invalid_type("number", other, path)),
    }
}

fn p_boolean(value: Option<&Value>, path: &[String]) -> PResult {
    match value {
        Some(v @ Value::Bool(_)) => Ok(v.clone()),
        other => Err(invalid_type("boolean", other, path)),
    }
}

fn p_literal_true(value: Option<&Value>, path: &[String]) -> PResult {
    match value {
        Some(Value::Bool(true)) => Ok(Value::Bool(true)),
        // zod invalid_value with a single expected literal, whatever the input type.
        _ => Err(at(path, "Invalid input: expected true")),
    }
}

fn p_enum(value: Option<&Value>, path: &[String], allowed: &[&str]) -> PResult {
    match value {
        Some(v @ Value::String(s)) if allowed.contains(&s.as_str()) => Ok(v.clone()),
        _ => Err(invalid_option(allowed, path)),
    }
}

/// `z.string().min(1).refine(isValidTerminalTabId, …)` — min fires before refine.
fn p_terminal_tab_id(value: Option<&Value>, path: &[String]) -> PResult {
    let parsed = p_string_min1(value, path)?;
    if !parsed.as_str().is_some_and(is_valid_terminal_tab_id) {
        return Err(at(path, MSG_TAB_ID_REFINE));
    }
    Ok(parsed)
}

fn p_nullable<F>(value: Option<&Value>, path: &[String], inner: F) -> PResult
where
    F: FnOnce(Option<&Value>, &[String]) -> PResult,
{
    match value {
        Some(Value::Null) => Ok(Value::Null),
        other => inner(other, path),
    }
}

// ─── Object/record/array plumbing ───────────────────────────────────

fn require_object<'a>(
    value: Option<&'a Value>,
    path: &[String],
) -> Result<&'a Map<String, Value>, Issue> {
    match value {
        Some(Value::Object(map)) => Ok(map),
        other => Err(invalid_type("object", other, path)),
    }
}

fn set_req<F>(
    out: &mut Map<String, Value>,
    obj: &Map<String, Value>,
    key: &str,
    path: &[String],
    parse: F,
) -> Result<(), Issue>
where
    F: FnOnce(Option<&Value>, &[String]) -> PResult,
{
    let child = push(path, key);
    let parsed = parse(obj.get(key), &child)?;
    out.insert(key.to_string(), parsed);
    Ok(())
}

fn set_opt<F>(
    out: &mut Map<String, Value>,
    obj: &Map<String, Value>,
    key: &str,
    path: &[String],
    parse: F,
) -> Result<(), Issue>
where
    F: FnOnce(Option<&Value>, &[String]) -> PResult,
{
    // JSON has no `undefined`: an absent key satisfies `.optional()`, a present
    // `null` is passed to the inner parser (so non-nullable optionals reject it).
    if let Some(value) = obj.get(key) {
        let child = push(path, key);
        out.insert(key.to_string(), parse(Some(value), &child)?);
    }
    Ok(())
}

fn p_record<F>(
    value: Option<&Value>,
    path: &[String],
    key_ok: Option<fn(&str) -> bool>,
    parse_value: F,
) -> PResult
where
    F: Fn(Option<&Value>, &[String]) -> PResult,
{
    let Some(Value::Object(map)) = value else {
        return Err(invalid_type("record", value, path));
    };
    let mut out = Map::new();
    for key in js_ordered_keys(map) {
        let child = push(path, key);
        if let Some(check) = key_ok {
            // zod wraps key-schema failures in a single invalid_key issue.
            if !check(key.as_str()) {
                return Err(at(&child, MSG_INVALID_KEY));
            }
        }
        out.insert(key.clone(), parse_value(Some(&map[key]), &child)?);
    }
    Ok(Value::Object(out))
}

fn p_array<F>(value: Option<&Value>, path: &[String], parse_item: F) -> PResult
where
    F: Fn(Option<&Value>, &[String]) -> PResult,
{
    let Some(Value::Array(items)) = value else {
        return Err(invalid_type("array", value, path));
    };
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        out.push(parse_item(Some(item), &push(path, &index.to_string()))?);
    }
    Ok(Value::Array(out))
}

// ─── Recursive layout-node unions (z.lazy) ──────────────────────────

fn p_pane_layout_node(value: Option<&Value>, path: &[String]) -> PResult {
    // z.union: the first branch that parses wins; total failure surfaces a
    // single invalid_union issue ("Invalid input") at the node's own path.
    p_pane_leaf(value, path)
        .or_else(|_| p_pane_split(value, path))
        .map_err(|_| at(path, MSG_INVALID_INPUT))
}

fn p_pane_leaf(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    if obj.get("type").and_then(Value::as_str) != Some("leaf") {
        return Err(at(&push(path, "type"), "Invalid input: expected \"leaf\""));
    }
    out.insert("type".to_string(), Value::String("leaf".to_string()));
    set_req(&mut out, obj, "leafId", path, p_string)?;
    Ok(Value::Object(out))
}

fn p_pane_split(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    if obj.get("type").and_then(Value::as_str) != Some("split") {
        return Err(at(&push(path, "type"), "Invalid input: expected \"split\""));
    }
    out.insert("type".to_string(), Value::String("split".to_string()));
    set_req(&mut out, obj, "direction", path, |v, p| p_enum(v, p, &["vertical", "horizontal"]))?;
    set_req(&mut out, obj, "first", path, p_pane_layout_node)?;
    set_req(&mut out, obj, "second", path, p_pane_layout_node)?;
    set_opt(&mut out, obj, "ratio", path, p_number)?;
    Ok(Value::Object(out))
}

fn p_group_layout_node(value: Option<&Value>, path: &[String]) -> PResult {
    p_group_leaf(value, path)
        .or_else(|_| p_group_split(value, path))
        .map_err(|_| at(path, MSG_INVALID_INPUT))
}

fn p_group_leaf(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    if obj.get("type").and_then(Value::as_str) != Some("leaf") {
        return Err(at(&push(path, "type"), "Invalid input: expected \"leaf\""));
    }
    out.insert("type".to_string(), Value::String("leaf".to_string()));
    set_req(&mut out, obj, "groupId", path, p_string)?;
    Ok(Value::Object(out))
}

fn p_group_split(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    if obj.get("type").and_then(Value::as_str) != Some("split") {
        return Err(at(&push(path, "type"), "Invalid input: expected \"split\""));
    }
    out.insert("type".to_string(), Value::String("split".to_string()));
    set_req(&mut out, obj, "direction", path, |v, p| p_enum(v, p, &["horizontal", "vertical"]))?;
    set_req(&mut out, obj, "first", path, p_group_layout_node)?;
    set_req(&mut out, obj, "second", path, p_group_layout_node)?;
    set_opt(&mut out, obj, "ratio", path, p_number)?;
    Ok(Value::Object(out))
}

// ─── Leaf object schemas (all strip unknown keys, like z.object) ────

fn p_terminal_layout_snapshot(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "root", path, |v, p| p_nullable(v, p, p_pane_layout_node))?;
    set_req(&mut out, obj, "activeLeafId", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "expandedLeafId", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "ptyIdsByLeafId", path, |v, p| p_record(v, p, None, p_string))?;
    set_opt(&mut out, obj, "buffersByLeafId", path, |v, p| p_record(v, p, None, p_string))?;
    set_opt(&mut out, obj, "scrollbackRefsByLeafId", path, |v, p| p_record(v, p, None, p_string))?;
    set_opt(&mut out, obj, "titlesByLeafId", path, |v, p| p_record(v, p, None, p_string))?;
    Ok(Value::Object(out))
}

fn p_terminal_tab(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "id", path, p_terminal_tab_id)?;
    set_req(&mut out, obj, "ptyId", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "worktreeId", path, p_string)?;
    set_req(&mut out, obj, "title", path, p_string)?;
    set_opt(&mut out, obj, "defaultTitle", path, p_string)?;
    set_opt(&mut out, obj, "generatedTitle", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "quickCommandLabel", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "customTitle", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "color", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "isPinned", path, p_boolean)?;
    set_req(&mut out, obj, "sortOrder", path, p_number)?;
    set_req(&mut out, obj, "createdAt", path, p_number)?;
    set_opt(&mut out, obj, "generation", path, p_number)?;
    set_opt(&mut out, obj, "startupCwd", path, p_string_min1)?;
    // launchAgent: z.custom(isTuiAgent).optional().catch(undefined) — a stale or
    // unknown agent is dropped rather than failing the whole session.
    if let Some(agent) = obj.get("launchAgent") {
        if agent.as_str().is_some_and(is_tui_agent) {
            out.insert("launchAgent".to_string(), agent.clone());
        }
    }
    Ok(Value::Object(out))
}

const TAB_CONTENT_TYPES: [&str; 7] =
    ["terminal", "editor", "diff", "conflict-review", "check-details", "browser", "simulator"];
const WORKSPACE_VISIBLE_TAB_TYPES: [&str; 4] = ["terminal", "editor", "browser", "simulator"];

fn p_tab(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "id", path, p_string)?;
    set_req(&mut out, obj, "entityId", path, p_string)?;
    set_req(&mut out, obj, "groupId", path, p_string)?;
    set_req(&mut out, obj, "worktreeId", path, p_string)?;
    set_req(&mut out, obj, "contentType", path, |v, p| p_enum(v, p, &TAB_CONTENT_TYPES))?;
    set_req(&mut out, obj, "label", path, p_string)?;
    set_opt(&mut out, obj, "generatedLabel", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "quickCommandLabel", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "customLabel", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "color", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "sortOrder", path, p_number)?;
    set_req(&mut out, obj, "createdAt", path, p_number)?;
    set_opt(&mut out, obj, "isPreview", path, p_boolean)?;
    set_opt(&mut out, obj, "isPinned", path, p_boolean)?;
    // viewMode: z.enum(['terminal','chat']).catch('terminal').optional() — an
    // unknown future mode degrades to the safe default instead of failing.
    if let Some(mode) = obj.get("viewMode") {
        let repaired = match mode.as_str() {
            Some("terminal") | Some("chat") => mode.clone(),
            _ => Value::String("terminal".to_string()),
        };
        out.insert("viewMode".to_string(), repaired);
    }
    Ok(Value::Object(out))
}

fn p_tab_group(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "id", path, p_string)?;
    set_req(&mut out, obj, "worktreeId", path, p_string)?;
    set_req(&mut out, obj, "activeTabId", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "tabOrder", path, |v, p| p_array(v, p, p_string))?;
    set_opt(&mut out, obj, "recentTabIds", path, |v, p| p_array(v, p, p_string))?;
    Ok(Value::Object(out))
}

fn p_persisted_open_file(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "filePath", path, p_string)?;
    set_req(&mut out, obj, "relativePath", path, p_string)?;
    set_req(&mut out, obj, "worktreeId", path, p_string)?;
    set_req(&mut out, obj, "language", path, p_string)?;
    set_opt(&mut out, obj, "isPreview", path, p_boolean)?;
    set_opt(&mut out, obj, "runtimeEnvironmentId", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "dirtyDraftContent", path, p_string)?;
    Ok(Value::Object(out))
}

fn p_browser_load_error(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "code", path, p_number)?;
    set_req(&mut out, obj, "description", path, p_string)?;
    set_req(&mut out, obj, "validatedUrl", path, p_string)?;
    Ok(Value::Object(out))
}

const BROWSER_VIEWPORT_PRESET_IDS: [&str; 7] =
    ["mobile-s", "mobile-m", "mobile-l", "tablet", "laptop", "laptop-l", "desktop"];

fn p_browser_workspace(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "id", path, p_string)?;
    set_req(&mut out, obj, "worktreeId", path, p_string)?;
    set_opt(&mut out, obj, "label", path, p_string)?;
    set_opt(&mut out, obj, "sessionProfileId", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "sessionPartition", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "activePageId", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "pageIds", path, |v, p| p_array(v, p, p_string))?;
    set_req(&mut out, obj, "url", path, p_string)?;
    set_req(&mut out, obj, "title", path, p_string)?;
    set_req(&mut out, obj, "loading", path, p_boolean)?;
    set_req(&mut out, obj, "faviconUrl", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "canGoBack", path, p_boolean)?;
    set_req(&mut out, obj, "canGoForward", path, p_boolean)?;
    set_req(&mut out, obj, "loadError", path, |v, p| p_nullable(v, p, p_browser_load_error))?;
    set_req(&mut out, obj, "createdAt", path, p_number)?;
    Ok(Value::Object(out))
}

fn p_browser_page(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "id", path, p_string)?;
    set_req(&mut out, obj, "workspaceId", path, p_string)?;
    set_req(&mut out, obj, "worktreeId", path, p_string)?;
    set_req(&mut out, obj, "url", path, p_string)?;
    set_req(&mut out, obj, "title", path, p_string)?;
    set_req(&mut out, obj, "loading", path, p_boolean)?;
    set_req(&mut out, obj, "faviconUrl", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "canGoBack", path, p_boolean)?;
    set_req(&mut out, obj, "canGoForward", path, p_boolean)?;
    set_req(&mut out, obj, "loadError", path, |v, p| p_nullable(v, p, p_browser_load_error))?;
    set_req(&mut out, obj, "createdAt", path, p_number)?;
    set_opt(&mut out, obj, "browserRuntimeEnvironmentId", path, |v, p| {
        p_nullable(v, p, p_string)
    })?;
    set_opt(&mut out, obj, "viewportPresetId", path, |v, p| {
        p_nullable(v, p, |vv, pp| p_enum(vv, pp, &BROWSER_VIEWPORT_PRESET_IDS))
    })?;
    Ok(Value::Object(out))
}

fn p_browser_history_entry(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "url", path, p_string)?;
    set_req(&mut out, obj, "normalizedUrl", path, p_string)?;
    set_req(&mut out, obj, "title", path, p_string)?;
    set_req(&mut out, obj, "lastVisitedAt", path, p_number)?;
    set_req(&mut out, obj, "visitCount", path, p_number)?;
    Ok(Value::Object(out))
}

/// `browserHistoryEntriesSchema` — validate entries, then apply the
/// `normalizeBrowserHistoryEntries` transform to the stripped entries.
fn p_browser_url_history(value: Option<&Value>, path: &[String]) -> PResult {
    let parsed = p_array(value, path, p_browser_history_entry)?;
    Ok(normalize_browser_history(&parsed))
}

// ─── activeWorkspaceKey (z.custom over isWorkspaceKey) ──────────────

/// `workspace-scope.ts` `isWorkspaceKey`: a `worktree:`/`folder:` prefix with a
/// non-empty remainder.
fn is_workspace_key(value: &str) -> bool {
    value.strip_prefix("worktree:").is_some_and(|rest| !rest.is_empty())
        || value.strip_prefix("folder:").is_some_and(|rest| !rest.is_empty())
}

fn p_workspace_key(value: Option<&Value>, path: &[String]) -> PResult {
    match value {
        Some(v @ Value::String(s)) if is_workspace_key(s) => Ok(v.clone()),
        // z.custom failures carry zod's generic message.
        _ => Err(at(path, MSG_INVALID_INPUT)),
    }
}

// ─── lastVisitedAtByWorktreeId (.preprocess repair) ─────────────────

/// Keep only finite, non-negative numeric timestamps so a single corrupted
/// value can't poison the whole-session parse. Mirrors the TS `.preprocess`:
/// arrays are objects in JS, so their index/value entries are cleaned too;
/// other non-objects fall through to the record parse and fail there.
fn p_last_visited(value: Option<&Value>, path: &[String]) -> PResult {
    let entries: Vec<(String, &Value)> = match value {
        Some(Value::Object(map)) => {
            js_ordered_keys(map).into_iter().map(|k| (k.clone(), &map[k])).collect()
        }
        Some(Value::Array(items)) => {
            items.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect()
        }
        other => return Err(invalid_type("record", other, path)),
    };
    let mut cleaned = Map::new();
    for (key, candidate) in entries {
        if let Some(number) = candidate.as_f64() {
            if number.is_finite() && number >= 0.0 {
                // Preserve the original number node (int vs float representation).
                cleaned.insert(key, candidate.clone());
            }
        }
    }
    Ok(Value::Object(cleaned))
}

// ─── Sleeping agents (workspace-session-sleeping-agents.ts) ─────────

const RESUMABLE_TUI_AGENTS: [&str; 9] = [
    "claude",
    "codex",
    "gemini",
    "antigravity",
    "opencode",
    "mimo-code",
    "droid",
    "grok",
    "devin",
];
const AGENT_STATUS_STATES: [&str; 4] = ["working", "blocked", "waiting", "done"];
const SLEEPING_ORIGINS: [&str; 3] = ["worktree-sleep", "quit", "live"];
const PROVIDER_SESSION_ID_MAX_LENGTH: usize = 512;

fn is_unsafe_object_key(key: &str) -> bool {
    matches!(key, "__proto__" | "constructor" | "prototype")
}

/// `hasUnsafeProviderSessionIdChars` / `hasUnsafeLaunchEnvChars` — same rule.
fn has_unsafe_control_chars(text: &str) -> bool {
    text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

/// `normalizeSessionId`: trimmed, non-empty, <=512 UTF-16 units, no leading
/// `-`, no control chars.
fn normalize_session_id(value: Option<&Value>) -> Option<String> {
    let trimmed = js_trim(value?.as_str()?);
    if trimmed.is_empty()
        || utf16_len(trimmed) > PROVIDER_SESSION_ID_MAX_LENGTH
        || trimmed.starts_with('-')
        || has_unsafe_control_chars(trimmed)
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// `normalizeAgentProviderSession` + the inner z.object: the object strips
/// `transcriptPath`, so the parsed value is always `{ key, id }`.
fn normalize_agent_provider_session(raw: &Value) -> Option<Value> {
    let record = raw.as_object()?;
    let key = record.get("key").and_then(Value::as_str)?;
    if key != "session_id" && key != "conversation_id" {
        return None;
    }
    let id = normalize_session_id(record.get("id"))?;
    Some(json!({ "key": key, "id": id }))
}

/// `sleepingAgentLaunchEnvSchema` preprocess: any bad key/value invalidates the
/// whole env (returns None), valid keys are stored trimmed.
fn clean_launch_env(value: Option<&Value>) -> Option<Value> {
    let map = value?.as_object()?;
    let mut cleaned = Map::new();
    for key in js_ordered_keys(map) {
        let trimmed = js_trim(key);
        let entry = &map[key];
        let value_ok = entry.as_str().is_some_and(|s| !s.contains('\0'));
        if trimmed.is_empty()
            || is_unsafe_object_key(trimmed)
            || trimmed.contains('=')
            || has_unsafe_control_chars(trimmed)
            || !value_ok
        {
            return None;
        }
        cleaned.insert(trimmed.to_string(), entry.clone());
    }
    Some(Value::Object(cleaned))
}

/// `sleepingAgentLaunchConfigSchema`: safeParse of the base object — any
/// failure collapses to `undefined` (config dropped, record kept).
fn parse_launch_config(raw: &Value) -> Option<Value> {
    let obj = raw.as_object()?;
    let mut out = Map::new();
    if let Some(command) = obj.get("agentCommand") {
        if !command.is_string() {
            return None;
        }
        out.insert("agentCommand".to_string(), command.clone());
    }
    let args = obj.get("agentArgs")?;
    if !args.is_string() {
        return None;
    }
    out.insert("agentArgs".to_string(), args.clone());
    out.insert("agentEnv".to_string(), clean_launch_env(obj.get("agentEnv"))?);
    Some(Value::Object(out))
}

/// `sleepingAgentSessionRecordSchema.safeParse` — None means the record is
/// silently dropped from the cleaned map (issues never surface).
fn parse_sleeping_record(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let mut out = Map::new();
    // paneKey: z.string().refine(length > 0)
    let pane_key = obj.get("paneKey")?;
    if pane_key.as_str().is_none_or(str::is_empty) {
        return None;
    }
    out.insert("paneKey".to_string(), pane_key.clone());
    if let Some(tab_id) = obj.get("tabId") {
        let text = tab_id.as_str()?;
        if text.is_empty() || !is_valid_terminal_tab_id(text) {
            return None;
        }
        out.insert("tabId".to_string(), tab_id.clone());
    }
    let worktree_id = obj.get("worktreeId")?;
    if worktree_id.as_str().is_none_or(str::is_empty) {
        return None;
    }
    out.insert("worktreeId".to_string(), worktree_id.clone());
    let agent = obj.get("agent")?;
    if !agent.as_str().is_some_and(|a| RESUMABLE_TUI_AGENTS.contains(&a)) {
        return None;
    }
    out.insert("agent".to_string(), agent.clone());
    out.insert(
        "providerSession".to_string(),
        normalize_agent_provider_session(obj.get("providerSession")?)?,
    );
    let prompt = obj.get("prompt")?;
    if !prompt.is_string() {
        return None;
    }
    out.insert("prompt".to_string(), prompt.clone());
    let state = obj.get("state")?;
    if !state.as_str().is_some_and(|s| AGENT_STATUS_STATES.contains(&s)) {
        return None;
    }
    out.insert("state".to_string(), state.clone());
    for stamp_key in ["capturedAt", "updatedAt"] {
        let stamp = obj.get(stamp_key)?;
        // z.number().finite().positive() — JSON numbers are finite already.
        if !stamp.as_f64().is_some_and(|n| n > 0.0) {
            return None;
        }
        out.insert(stamp_key.to_string(), stamp.clone());
    }
    for text_key in ["terminalTitle", "lastAssistantMessage"] {
        if let Some(text) = obj.get(text_key) {
            if !text.is_string() {
                return None;
            }
            out.insert(text_key.to_string(), text.clone());
        }
    }
    if let Some(interrupted) = obj.get("interrupted") {
        if !interrupted.is_boolean() {
            return None;
        }
        out.insert("interrupted".to_string(), interrupted.clone());
    }
    if let Some(connection_id) = obj.get("connectionId") {
        if !connection_id.is_null() && !connection_id.is_string() {
            return None;
        }
        out.insert("connectionId".to_string(), connection_id.clone());
    }
    // launchConfig failures drop only the config, never the record.
    if let Some(config) = obj.get("launchConfig") {
        if let Some(parsed) = parse_launch_config(config) {
            out.insert("launchConfig".to_string(), parsed);
        }
    }
    if let Some(origin) = obj.get("origin") {
        if !origin.as_str().is_some_and(|o| SLEEPING_ORIGINS.contains(&o)) {
            return None;
        }
        out.insert("origin".to_string(), origin.clone());
    }
    Some(Value::Object(out))
}

/// `sleepingAgentSessionsByPaneKeySchema` preprocess: drop invalid records and
/// unsafe keys, keep records whose `paneKey` matches their map key; an empty
/// result collapses to `undefined` (key omitted). Never fails the session.
fn parse_sleeping_sessions(value: Option<&Value>) -> Option<Value> {
    let map = value?.as_object()?;
    let mut cleaned = Map::new();
    for key in js_ordered_keys(map) {
        if is_unsafe_object_key(key) {
            continue;
        }
        if let Some(record) = parse_sleeping_record(&map[key]) {
            if record.get("paneKey").and_then(Value::as_str) == Some(key.as_str()) {
                cleaned.insert(key.clone(), record);
            }
        }
    }
    if cleaned.is_empty() {
        None
    } else {
        Some(Value::Object(cleaned))
    }
}

// ─── Top-level session (shape order matters for the first issue) ────

fn p_session(value: Option<&Value>, path: &[String]) -> PResult {
    let obj = require_object(value, path)?;
    let mut out = Map::new();
    set_req(&mut out, obj, "activeRepoId", path, |v, p| p_nullable(v, p, p_string))?;
    set_opt(&mut out, obj, "activeWorkspaceKey", path, |v, p| p_nullable(v, p, p_workspace_key))?;
    set_req(&mut out, obj, "activeWorktreeId", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "activeTabId", path, |v, p| p_nullable(v, p, p_string))?;
    set_req(&mut out, obj, "tabsByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_array(vv, pp, p_terminal_tab))
    })?;
    set_req(&mut out, obj, "terminalLayoutsByTabId", path, |v, p| {
        p_record(v, p, Some(is_valid_terminal_tab_id), p_terminal_layout_snapshot)
    })?;
    set_opt(&mut out, obj, "activeWorktreeIdsOnShutdown", path, |v, p| p_array(v, p, p_string))?;
    set_opt(&mut out, obj, "openFilesByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_array(vv, pp, p_persisted_open_file))
    })?;
    set_opt(&mut out, obj, "activeFileIdByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_nullable(vv, pp, p_string))
    })?;
    set_opt(&mut out, obj, "markdownFrontmatterVisible", path, |v, p| {
        p_record(v, p, None, p_boolean)
    })?;
    set_opt(&mut out, obj, "browserTabsByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_array(vv, pp, p_browser_workspace))
    })?;
    set_opt(&mut out, obj, "browserPagesByWorkspace", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_array(vv, pp, p_browser_page))
    })?;
    set_opt(&mut out, obj, "activeBrowserTabIdByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_nullable(vv, pp, p_string))
    })?;
    set_opt(&mut out, obj, "activeTabTypeByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_enum(vv, pp, &WORKSPACE_VISIBLE_TAB_TYPES))
    })?;
    set_opt(&mut out, obj, "browserUrlHistory", path, p_browser_url_history)?;
    set_opt(&mut out, obj, "activeTabIdByWorktree", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_nullable(vv, pp, p_string))
    })?;
    set_opt(&mut out, obj, "unifiedTabs", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_array(vv, pp, p_tab))
    })?;
    set_opt(&mut out, obj, "tabGroups", path, |v, p| {
        p_record(v, p, None, |vv, pp| p_array(vv, pp, p_tab_group))
    })?;
    set_opt(&mut out, obj, "tabGroupLayouts", path, |v, p| {
        p_record(v, p, None, p_group_layout_node)
    })?;
    set_opt(&mut out, obj, "activeGroupIdByWorktree", path, |v, p| {
        p_record(v, p, None, p_string)
    })?;
    set_opt(&mut out, obj, "activeConnectionIdsAtShutdown", path, |v, p| {
        p_array(v, p, p_string)
    })?;
    set_opt(&mut out, obj, "remoteSessionIdsByTabId", path, |v, p| {
        p_record(v, p, Some(is_valid_terminal_tab_id), p_string)
    })?;
    set_opt(&mut out, obj, "lastVisitedAtByWorktreeId", path, p_last_visited)?;
    set_opt(&mut out, obj, "defaultTerminalTabsAppliedByWorktreeId", path, |v, p| {
        p_record(v, p, None, p_literal_true)
    })?;
    // sleepingAgentSessionsByPaneKey's preprocess never fails; an empty or
    // invalid map collapses to `undefined` and the key is omitted.
    if let Some(cleaned) = parse_sleeping_sessions(obj.get("sleepingAgentSessionsByPaneKey")) {
        out.insert("sleepingAgentSessionsByPaneKey".to_string(), cleaned);
    }
    Ok(Value::Object(out))
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

    fn parse(raw: Value) -> ParsedWorkspaceSession {
        parse_workspace_session(&raw)
    }

    fn minimal() -> Value {
        json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {},
            "terminalLayoutsByTabId": {}
        })
    }

    fn minimal_with(entries: &[(&str, Value)]) -> Value {
        let mut session = minimal();
        for (key, value) in entries {
            session[*key] = value.clone();
        }
        session
    }

    #[test]
    fn accepts_a_minimal_valid_session() {
        let result = parse(minimal());
        assert_eq!(result.value(), Some(&minimal()));
    }

    #[test]
    fn accepts_a_fully_populated_session_with_optional_fields() {
        let result = parse(json!({
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
    fn strips_unknown_keys_at_every_object_depth() {
        // Deviation A fix: z.object strips unrecognized keys everywhere.
        let result = parse(json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "sessionSchemaVersion": 42,
            "tabsByWorktree": {
                "wt-1": [{
                    "id": "tab-1",
                    "ptyId": null,
                    "worktreeId": "wt-1",
                    "title": "zsh",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1,
                    "legacyPaneState": { "zoom": 2 }
                }]
            },
            "terminalLayoutsByTabId": {
                "tab-1": {
                    "root": { "type": "leaf", "leafId": "leaf-a", "legacySize": 120 },
                    "activeLeafId": "leaf-a",
                    "expandedLeafId": null,
                    "zoomLevel": 1.5
                }
            }
        }));
        let value = result.value().unwrap();
        assert!(value.get("sessionSchemaVersion").is_none());
        assert!(value["tabsByWorktree"]["wt-1"][0].get("legacyPaneState").is_none());
        assert!(value["terminalLayoutsByTabId"]["tab-1"].get("zoomLevel").is_none());
        assert!(value["terminalLayoutsByTabId"]["tab-1"]["root"].get("legacySize").is_none());
    }

    #[test]
    fn preserves_a_valid_launch_agent_and_drops_an_unknown_one() {
        let session = minimal_with(&[(
            "tabsByWorktree",
            json!({
                "wt": [
                    {
                        "id": "tab1", "ptyId": null, "worktreeId": "wt", "title": "codex",
                        "customTitle": null, "color": null, "sortOrder": 0, "createdAt": 1,
                        "launchAgent": "codex"
                    },
                    {
                        "id": "tab2", "ptyId": null, "worktreeId": "wt", "title": "bash",
                        "customTitle": null, "color": null, "sortOrder": 1, "createdAt": 1,
                        "launchAgent": "some-retired-agent"
                    }
                ]
            }),
        )]);
        let result = parse(session);
        let value = result.value().unwrap();
        assert_eq!(value["tabsByWorktree"]["wt"][0]["launchAgent"], json!("codex"));
        assert!(value["tabsByWorktree"]["wt"][1].get("launchAgent").is_none());
    }

    #[test]
    fn rejects_a_wrong_typed_pty_id_with_zod_wording() {
        let session = minimal_with(&[(
            "tabsByWorktree",
            json!({
                "wt-1": [{
                    "id": "tab-1", "ptyId": 42, "worktreeId": "wt-1", "title": "zsh",
                    "customTitle": null, "color": null, "sortOrder": 0, "createdAt": 0
                }]
            }),
        )]);
        assert_eq!(
            parse(session).error(),
            Some("tabsByWorktree.wt-1.0.ptyId: Invalid input: expected string, received number")
        );
    }

    #[test]
    fn rejects_missing_required_fields_with_zod_wording() {
        assert_eq!(
            parse(json!({})).error(),
            Some("activeRepoId: Invalid input: expected string, received undefined")
        );
        assert_eq!(
            parse(json!({
                "activeRepoId": null,
                "activeWorktreeId": null,
                "activeTabId": null,
                "terminalLayoutsByTabId": {}
            }))
            .error(),
            Some("tabsByWorktree: Invalid input: expected record, received undefined")
        );
    }

    #[test]
    fn rejects_non_object_roots_with_zod_wording() {
        assert_eq!(
            parse(Value::Null).error(),
            Some("<root>: Invalid input: expected object, received null")
        );
        assert_eq!(
            parse(json!("garbage")).error(),
            Some("<root>: Invalid input: expected object, received string")
        );
        assert_eq!(
            parse(json!(42)).error(),
            Some("<root>: Invalid input: expected object, received number")
        );
    }

    #[test]
    fn rejects_an_array_where_a_record_is_expected() {
        let session = minimal_with(&[("tabsByWorktree", json!([]))]);
        assert_eq!(
            parse(session).error(),
            Some("tabsByWorktree: Invalid input: expected record, received array")
        );
    }

    #[test]
    fn reports_invalid_record_keys_with_zod_wording() {
        let session = minimal_with(&[(
            "terminalLayoutsByTabId",
            json!({ "bad:key": { "root": null, "activeLeafId": null, "expandedLeafId": null } }),
        )]);
        assert_eq!(
            parse(session).error(),
            Some("terminalLayoutsByTabId.bad:key: Invalid key in record")
        );
    }

    #[test]
    fn reports_union_failures_as_invalid_input_at_the_node_path() {
        let session = minimal_with(&[(
            "terminalLayoutsByTabId",
            json!({ "t": { "root": { "type": "nope" }, "activeLeafId": null, "expandedLeafId": null } }),
        )]);
        assert_eq!(parse(session).error(), Some("terminalLayoutsByTabId.t.root: Invalid input"));
        let missing_root = minimal_with(&[(
            "terminalLayoutsByTabId",
            json!({ "t": { "activeLeafId": null, "expandedLeafId": null } }),
        )]);
        assert_eq!(
            parse(missing_root).error(),
            Some("terminalLayoutsByTabId.t.root: Invalid input")
        );
    }

    #[test]
    fn reports_enum_and_literal_failures_with_zod_wording() {
        let bad_type = minimal_with(&[("activeTabTypeByWorktree", json!({ "wt": "diff" }))]);
        assert_eq!(
            parse(bad_type).error(),
            Some(
                "activeTabTypeByWorktree.wt: Invalid option: expected one of \
                 \"terminal\"|\"editor\"|\"browser\"|\"simulator\""
            )
        );
        let bad_marker =
            minimal_with(&[("defaultTerminalTabsAppliedByWorktreeId", json!({ "a": false }))]);
        assert_eq!(
            parse(bad_marker).error(),
            Some("defaultTerminalTabsAppliedByWorktreeId.a: Invalid input: expected true")
        );
    }

    #[test]
    fn rejects_terminal_tab_ids_with_min_before_refine() {
        let colon = minimal_with(&[(
            "tabsByWorktree",
            json!({
                "wt-1": [{
                    "id": "tab:with-colon", "ptyId": null, "worktreeId": "wt-1", "title": "zsh",
                    "customTitle": null, "color": null, "sortOrder": 0, "createdAt": 0
                }]
            }),
        )]);
        assert_eq!(
            parse(colon).error(),
            Some("tabsByWorktree.wt-1.0.id: terminal tab id must not contain \":\"")
        );
        let empty = minimal_with(&[(
            "tabsByWorktree",
            json!({
                "wt-1": [{
                    "id": "", "ptyId": null, "worktreeId": "wt-1", "title": "zsh",
                    "customTitle": null, "color": null, "sortOrder": 0, "createdAt": 0
                }]
            }),
        )]);
        assert_eq!(
            parse(empty).error(),
            Some("tabsByWorktree.wt-1.0.id: Too small: expected string to have >=1 characters")
        );
    }

    #[test]
    fn enforces_startup_cwd_min_length() {
        let session = minimal_with(&[(
            "tabsByWorktree",
            json!({
                "wt-1": [{
                    "id": "tab-1", "ptyId": null, "worktreeId": "wt-1", "title": "zsh",
                    "customTitle": null, "color": null, "sortOrder": 0, "createdAt": 0,
                    "startupCwd": ""
                }]
            }),
        )]);
        assert_eq!(
            parse(session).error(),
            Some(
                "tabsByWorktree.wt-1.0.startupCwd: Too small: expected string to have >=1 \
                 characters"
            )
        );
    }

    #[test]
    fn validates_active_workspace_key_via_is_workspace_key() {
        let ok = minimal_with(&[("activeWorkspaceKey", json!("worktree:repo::/x"))]);
        assert_eq!(parse(ok).value().unwrap()["activeWorkspaceKey"], json!("worktree:repo::/x"));
        let folder = minimal_with(&[("activeWorkspaceKey", json!("folder:abc"))]);
        assert!(parse(folder).is_ok());
        let null_key = minimal_with(&[("activeWorkspaceKey", Value::Null)]);
        assert_eq!(parse(null_key).value().unwrap()["activeWorkspaceKey"], Value::Null);
        for bad in [json!("not-a-workspace-key"), json!("worktree:"), json!(7)] {
            let session = minimal_with(&[("activeWorkspaceKey", bad)]);
            assert_eq!(parse(session).error(), Some("activeWorkspaceKey: Invalid input"));
        }
    }

    #[test]
    fn repairs_unknown_view_mode_to_terminal() {
        let session = minimal_with(&[(
            "unifiedTabs",
            json!({
                "wt-1": [
                    {
                        "id": "t1", "entityId": "e1", "groupId": "g", "worktreeId": "wt-1",
                        "contentType": "terminal", "label": "zsh", "customLabel": null,
                        "color": null, "sortOrder": 0, "createdAt": 1, "viewMode": "canvas"
                    },
                    {
                        "id": "t2", "entityId": "e2", "groupId": "g", "worktreeId": "wt-1",
                        "contentType": "terminal", "label": "claude", "customLabel": null,
                        "color": null, "sortOrder": 1, "createdAt": 1, "viewMode": "chat"
                    }
                ]
            }),
        )]);
        let result = parse(session);
        let tabs = &result.value().unwrap()["unifiedTabs"]["wt-1"];
        assert_eq!(tabs[0]["viewMode"], json!("terminal"));
        assert_eq!(tabs[1]["viewMode"], json!("chat"));
    }

    #[test]
    fn accepts_check_details_and_simulator_content_types() {
        let session = minimal_with(&[(
            "unifiedTabs",
            json!({
                "wt-1": [
                    {
                        "id": "t1", "entityId": "e1", "groupId": "g", "worktreeId": "wt-1",
                        "contentType": "check-details", "label": "Checks", "customLabel": null,
                        "color": null, "sortOrder": 0, "createdAt": 1
                    },
                    {
                        "id": "t2", "entityId": "e2", "groupId": "g", "worktreeId": "wt-1",
                        "contentType": "simulator", "label": "iOS Simulator", "customLabel": null,
                        "color": null, "sortOrder": 1, "createdAt": 1
                    }
                ]
            }),
        )]);
        assert!(parse(session).is_ok());
    }

    #[test]
    fn drops_bad_last_visited_at_entries_rather_than_failing_the_session() {
        let session = minimal_with(&[(
            "lastVisitedAtByWorktreeId",
            json!({
                "good": 1_700_000_000_000_i64,
                "nan": null,
                "negative": -5,
                "string": "nope"
            }),
        )]);
        let result = parse(session);
        assert_eq!(
            result.value().unwrap()["lastVisitedAtByWorktreeId"],
            json!({ "good": 1_700_000_000_000_i64 })
        );
        // typeof [] === 'object' in JS, so arrays are cleaned entry-wise too.
        let array_session = minimal_with(&[("lastVisitedAtByWorktreeId", json!([5]))]);
        assert_eq!(
            parse(array_session).value().unwrap()["lastVisitedAtByWorktreeId"],
            json!({ "0": 5 })
        );
        let string_session = minimal_with(&[("lastVisitedAtByWorktreeId", json!("x"))]);
        assert_eq!(
            parse(string_session).error(),
            Some("lastVisitedAtByWorktreeId: Invalid input: expected record, received string")
        );
    }

    #[test]
    fn cleans_sleeping_agent_sessions_and_trims_provider_session_ids() {
        let session = minimal_with(&[(
            "sleepingAgentSessionsByPaneKey",
            json!({
                "pane-good": {
                    "paneKey": "pane-good",
                    "worktreeId": "wt-1",
                    "agent": "claude",
                    "providerSession": { "key": "session_id", "id": "  sess-42  " },
                    "prompt": "resume me",
                    "state": "waiting",
                    "capturedAt": 1,
                    "updatedAt": 2,
                    "unknownField": true
                },
                "pane-broken": {
                    "paneKey": "pane-broken",
                    "worktreeId": "",
                    "agent": "claude",
                    "providerSession": { "key": "session_id", "id": "sess-43" },
                    "prompt": "broken record",
                    "state": "waiting",
                    "capturedAt": 1,
                    "updatedAt": 2
                }
            }),
        )]);
        let result = parse(session);
        let cleaned = &result.value().unwrap()["sleepingAgentSessionsByPaneKey"];
        assert!(cleaned.get("pane-broken").is_none());
        assert_eq!(cleaned["pane-good"]["providerSession"], json!({ "key": "session_id", "id": "sess-42" }));
        assert!(cleaned["pane-good"].get("unknownField").is_none());
    }

    #[test]
    fn omits_sleeping_agent_sessions_when_nothing_survives_cleanup() {
        for raw in [Value::Null, json!("nope"), json!({}), json!({ "p": { "paneKey": "q" } })] {
            let session = minimal_with(&[("sleepingAgentSessionsByPaneKey", raw)]);
            let result = parse(session);
            assert!(result.value().unwrap().get("sleepingAgentSessionsByPaneKey").is_none());
        }
    }

    #[test]
    fn drops_only_the_launch_config_when_its_env_is_unsafe() {
        let record = |env: Value| {
            json!({
                "paneKey": "p",
                "worktreeId": "wt",
                "agent": "codex",
                "providerSession": { "key": "session_id", "id": "sess-1" },
                "prompt": "",
                "state": "done",
                "capturedAt": 1,
                "updatedAt": 1,
                "launchConfig": { "agentCommand": "codex", "agentArgs": "-r", "agentEnv": env }
            })
        };
        let bad = minimal_with(&[(
            "sleepingAgentSessionsByPaneKey",
            json!({ "p": record(json!({ "A=B": "x" })) }),
        )]);
        let bad_value = parse(bad);
        let kept = &bad_value.value().unwrap()["sleepingAgentSessionsByPaneKey"]["p"];
        assert!(kept.get("launchConfig").is_none());
        // Env keys are stored trimmed when the whole env is safe.
        let good = minimal_with(&[(
            "sleepingAgentSessionsByPaneKey",
            json!({ "p": record(json!({ "  PATH  ": "/bin" })) }),
        )]);
        let good_value = parse(good);
        assert_eq!(
            good_value.value().unwrap()["sleepingAgentSessionsByPaneKey"]["p"]["launchConfig"]
                ["agentEnv"],
            json!({ "PATH": "/bin" })
        );
    }

    #[test]
    fn reports_the_smallest_integer_like_record_key_first() {
        // JS property order: integer-like keys ascending before insertion order.
        let session = minimal_with(&[(
            "markdownFrontmatterVisible",
            json!({ "b": "bad1", "10": "bad2", "2": "bad3" }),
        )]);
        assert_eq!(
            parse(session).error(),
            Some("markdownFrontmatterVisible.2: Invalid input: expected boolean, received string")
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
        let session = minimal_with(&[("browserUrlHistory", Value::Array(history))]);
        let result = parse(session);
        let value = result.value().unwrap();
        let entries = value["browserUrlHistory"].as_array().unwrap();
        assert_eq!(entries.len(), MAX_BROWSER_HISTORY_ENTRIES);
        assert_eq!(entries.last().unwrap()["url"], json!("https://example.com/199"));
    }

    #[test]
    fn accepts_default_tab_idempotency_markers() {
        let session = minimal_with(&[(
            "defaultTerminalTabsAppliedByWorktreeId",
            json!({ "repo1::/path/wt1": true }),
        )]);
        let result = parse(session);
        assert_eq!(
            result.value().unwrap()["defaultTerminalTabsAppliedByWorktreeId"],
            json!({ "repo1::/path/wt1": true })
        );
    }

    // ─── Parity vector replay (same check the harness performs) ─────

    const PENDING_VECTORS: &str = include_str!(
        "../../../../tools/parity/vectors/workspace-session-schema.json"
    );

    /// Order-insensitive object compare with f64 number equality — mirrors
    /// `orca-parity`'s `json_semantic_eq` (this crate must not depend on it).
    fn semantic_eq(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
                (Some(p), Some(q)) => p == q,
                _ => x == y,
            },
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| semantic_eq(p, q))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len()
                    && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| semantic_eq(v, w)))
            }
            _ => a == b,
        }
    }

    #[test]
    fn replays_every_pending_parity_vector_exactly() {
        let doc: Value = serde_json::from_str(PENDING_VECTORS).expect("vectors parse");
        let cases = doc.get("cases").and_then(Value::as_array).expect("cases");
        assert!(!cases.is_empty());
        for (index, case) in cases.iter().enumerate() {
            assert_eq!(
                case.get("function").and_then(Value::as_str),
                Some("parseWorkspaceSession"),
                "case #{index} function"
            );
            let input = case.get("input").expect("input");
            // Expected is optional: some vectors (e.g. browserUrlHistory URL
            // normalization) only assert TS==Rust in the harness and carry no
            // golden value — mirror that tolerance instead of hard-requiring it.
            let Some(expected) = case.get("expected") else {
                continue;
            };
            // Mirror the parity adapter's marshalling of the discriminated union.
            let output = match parse_workspace_session(input) {
                ParsedWorkspaceSession::Ok(value) => json!({ "ok": true, "value": value }),
                ParsedWorkspaceSession::Err(error) => json!({ "ok": false, "error": error }),
            };
            assert!(
                semantic_eq(&output, expected),
                "case #{index} ({}) diverged:\n  rust:     {output}\n  expected: {expected}",
                case.get("note").and_then(Value::as_str).unwrap_or("")
            );
        }
    }
}
