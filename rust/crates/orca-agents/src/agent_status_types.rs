//! Agent status payload parse/normalize, ported from the parser half of
//! `src/shared/agent-status-types.ts`.
//!
//! Validates an untrusted agent status payload (hook/OSC-9999) into the lean
//! `ParsedAgentStatusPayload`: a pre-parse JSON structure guard, state
//! allow-list, per-field trim/collapse/cap, and UTF-16-safe truncation that
//! never leaves a lone surrogate. Over vendored `serde_json`.
//!
//! The twin's behaviour spans four TS files and all four are ported here:
//! `agent-status-types.ts` (shapes, caps, subagents), its field normalizers in
//! `agent-status-field-normalization.ts` (bounded single-line preview,
//! paragraph-preserving multiline, untouched `interactivePrompt`),
//! `orca-dispatch-status-prompt.ts` (dispatch-preamble compaction) and
//! `json-text-structure-limit.ts` (the token/depth guard run before `JSON.parse`).
//!
//! Everything that measures a length works in UTF-16 code units, because the
//! twin's caps and scan bounds are `String.prototype.length`. Counting `char`s
//! instead would let an emoji-dense prompt through at twice the budget.

use serde_json::{Number, Value};

pub const AGENT_STATUS_STATES: [&str; 4] = ["working", "blocked", "waiting", "done"];
pub const AGENT_STATUS_MAX_FIELD_LENGTH: usize = 200;
pub const AGENT_STATUS_TOOL_NAME_MAX_LENGTH: usize = 60;
pub const AGENT_STATUS_TOOL_INPUT_MAX_LENGTH: usize = 160;
pub const AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH: usize = 8000;
/// Holds full AskUserQuestion JSON, so it is capped but never previewed.
pub const AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH: usize = 16000;
pub const AGENT_TYPE_MAX_LENGTH: usize = 40;
pub const AGENT_MODEL_MAX_LENGTH: usize = 120;
pub const AGENT_STATUS_MAX_SUBAGENTS: usize = 32;
const AGENT_SUBAGENT_ID_MAX_LENGTH: usize = 64;
pub const AGENT_STATE_HISTORY_MAX: usize = 20;
/// `30 * 60 * 1000`, held as `f64` because the twin compares it against a JS
/// millisecond subtraction.
pub const AGENT_STATUS_STALE_AFTER_MS: f64 = 30.0 * 60.0 * 1000.0;

/// Dispatch lifecycle states that mean the work is finished and the pane is
/// safe to sleep (`SETTLED_DISPATCH_STATUSES`).
pub const SETTLED_DISPATCH_STATUSES: [&str; 3] = ["completed", "failed", "circuit_broken"];

/// The six fields `agentSubagentsEqual` compares, in the twin's order.
const SUBAGENT_EQUALITY_FIELDS: [&str; 6] =
    ["id", "state", "startedAt", "agentType", "model", "description"];

/// Pre-parse guard bounds (`AGENT_STATUS_JSON_STRUCTURE_LIMITS`).
pub const AGENT_STATUS_JSON_STRUCTURAL_TOKEN_LIMIT: usize = 4096;
pub const AGENT_STATUS_JSON_NESTING_DEPTH_LIMIT: usize = 16;

const SINGLE_LINE_FIELD_SCAN_OVERHEAD: usize = 64;
const SINGLE_LINE_FIELD_SCAN_MULTIPLIER: usize = 8;

pub const ORCA_DISPATCH_STATUS_PREAMBLE_PREFIX: &str =
    "You are working inside Orca, a multi-agent IDE.";
pub const ORCA_DISPATCH_STATUS_TASK_MARKER: &str = "=== TASK ===";
const ORCA_DISPATCH_STATUS_TASK_ID_MARKER: &str = "Your task ID is:";
/// Real preambles put `=== TASK ===` ~4KB in, past the single-line scan budget.
const ORCA_DISPATCH_STATUS_SOURCE_SCAN_LIMIT: usize = 24_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatusState {
    Working,
    Blocked,
    Waiting,
    Done,
}

impl AgentStatusState {
    fn from_id(value: &str) -> Option<AgentStatusState> {
        match value {
            "working" => Some(AgentStatusState::Working),
            "blocked" => Some(AgentStatusState::Blocked),
            "waiting" => Some(AgentStatusState::Waiting),
            "done" => Some(AgentStatusState::Done),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            AgentStatusState::Working => "working",
            AgentStatusState::Blocked => "blocked",
            AgentStatusState::Waiting => "waiting",
            AgentStatusState::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSubagentState {
    Working,
    Blocked,
    Waiting,
    Idle,
}

impl AgentSubagentState {
    fn from_id(value: &str) -> Option<AgentSubagentState> {
        match value {
            "working" => Some(AgentSubagentState::Working),
            "blocked" => Some(AgentSubagentState::Blocked),
            "waiting" => Some(AgentSubagentState::Waiting),
            "idle" => Some(AgentSubagentState::Idle),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            AgentSubagentState::Working => "working",
            AgentSubagentState::Blocked => "blocked",
            AgentSubagentState::Waiting => "waiting",
            AgentSubagentState::Idle => "idle",
        }
    }
}

/// A live in-process child of the reporting session.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentSubagentSnapshot {
    pub id: String,
    pub state: AgentSubagentState,
    /// `f64` because the twin holds whatever `JSON.parse` produced — a JS
    /// number — and non-finite/non-numeric input coerces to 0.
    pub started_at: f64,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedAgentStatusPayload {
    pub state: AgentStatusState,
    pub prompt: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub interactive_prompt: Option<String>,
    pub last_assistant_message: Option<String>,
    pub interrupted: Option<bool>,
    pub launch_failed: Option<bool>,
    pub subagents: Option<Vec<AgentSubagentSnapshot>>,
}

// ─── UTF-16 primitives ──────────────────────────────────────────────────────

fn units_of(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn string_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `String.prototype.trim`'s whitespace set — deliberately NOT Rust's
/// `char::is_whitespace`, which counts U+0085 (JS does not) and skips U+FEFF
/// (JS trims it). Also the exact set JS `\s` matches.
fn is_ecma_trim_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x20 | 0x09..=0x0d
            | 0xa0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
            | 0xfeff
    )
}

fn is_single_line_separator(unit: u16) -> bool {
    matches!(unit, 0x0d | 0x0a | 0x2028 | 0x2029)
}

fn is_line_break(unit: u16) -> bool {
    unit == 0x0a || unit == 0x0d
}

fn trimmed_bounds(units: &[u16]) -> (usize, usize) {
    let mut start = 0;
    let mut end = units.len();
    while start < end && units.get(start).copied().is_some_and(is_ecma_trim_whitespace) {
        start += 1;
    }
    while end > start
        && end
            .checked_sub(1)
            .and_then(|index| units.get(index).copied())
            .is_some_and(is_ecma_trim_whitespace)
    {
        end -= 1;
    }
    (start, end)
}

fn trimmed(units: &[u16]) -> &[u16] {
    let (start, end) = trimmed_bounds(units);
    units.get(start..end).unwrap_or(&[])
}

fn find_units(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len().saturating_sub(needle.len()))
        .find(|&index| haystack.get(index..index.saturating_add(needle.len())) == Some(needle))
}

/// Truncate to `max_length` UTF-16 code units, dropping a trailing lone high
/// surrogate so the result is always valid UTF-16 (no replacement glyph).
// Trust contract: inert unless verification runs; checked under `targo trust`.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<u16>| out.len() <= max_length))]
fn truncate_preserving_surrogates(units: &[u16], max_length: usize) -> Vec<u16> {
    // Why `<` and not `<=`: the twin only short-circuits BELOW the cap, so a
    // string sitting exactly on it still goes through the surrogate guard.
    if units.len() < max_length {
        return units.to_vec();
    }
    let mut end = max_length.min(units.len());
    if end
        .checked_sub(1)
        .and_then(|index| units.get(index))
        .is_some_and(|unit| (0xd800..=0xdbff).contains(unit))
    {
        end -= 1;
    }
    units.get(..end).unwrap_or(units).to_vec()
}

// ─── JS value semantics for the predicate half of the twin ──────────────────
//
// `hasUnsettledOrUnknownDispatch`, `isFreshNonDoneAgentStatus` and
// `agentSubagentsEqual` take *unvalidated* objects: no state allow-list, no
// field normalization, just JS truthiness, `-`/`<=` numeric coercion and `!==`.
// Modelling those three operators is what keeps the port from answering `false`
// where the twin answers `true` on a field the type says cannot be missing but
// a hook payload omits anyway.

/// JS truthiness of a decoded JSON value. `undefined` is the caller's `None`,
/// which is falsy everywhere below.
fn is_js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        // JSON cannot carry NaN, so 0/-0 is the only falsy number.
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn is_ecma_trim_whitespace_char(character: char) -> bool {
    u16::try_from(character as u32).is_ok_and(is_ecma_trim_whitespace)
}

/// `ToNumber` over a *radix-prefixed* `StringNumericLiteral` body (`0x`/`0o`/`0b`).
fn radix_literal_to_number(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut value = 0.0_f64;
    for character in digits.chars() {
        let Some(digit) = character.to_digit(radix) else {
            return f64::NAN;
        };
        value = value * f64::from(radix) + f64::from(digit);
    }
    value
}

/// JS `ToNumber(string)`. Rust's own `f64` parser is close but not the same
/// grammar — it accepts `inf`, `infinity` and `nan`, which JS rejects — so the
/// literal is charset-gated before it is handed over.
fn js_string_to_number(text: &str) -> f64 {
    let body = text.trim_matches(is_ecma_trim_whitespace_char);
    if body.is_empty() {
        return 0.0;
    }
    match body {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = body.strip_prefix(prefix) {
            return radix_literal_to_number(digits, radix);
        }
    }
    if !body
        .chars()
        .all(|character| character.is_ascii_digit() || matches!(character, 'e' | 'E' | '+' | '-' | '.'))
    {
        return f64::NAN;
    }
    body.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Array.prototype.join(',')` — `null` elements stringify to the empty string
/// there, unlike `String(null)`.
fn js_array_to_string(items: &[Value]) -> String {
    items
        .iter()
        .map(|item| match item {
            Value::Null => String::new(),
            other => js_value_to_string(other),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn js_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        // Rust's shortest-round-trip `Display` differs from JS's exponent
        // spelling only in ways that re-read as the same number, and this text
        // is always fed straight back into `ToNumber`.
        Value::Number(number) => number.as_f64().map(|number| number.to_string()).unwrap_or_default(),
        Value::String(text) => text.clone(),
        Value::Array(items) => js_array_to_string(items),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// JS `ToNumber` for an operand of `-` or `<=`. `None` is `undefined` → NaN,
/// which is what makes every comparison against a missing timestamp false.
fn js_to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => js_string_to_number(text),
        Some(Value::Array(items)) => js_string_to_number(&js_array_to_string(items)),
        // `ToPrimitive({})` is "[object Object]".
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// JS `===` for two decoded JSON values, `None` standing for `undefined`.
/// Objects and arrays compare by *reference* there; two values decoded from
/// JSON are never the same reference, so they are never strictly equal.
fn js_strict_equals(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (None, None) => true,
        (None, _) | (_, None) => false,
        (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(left)), Some(Value::Bool(right))) => left == right,
        (Some(Value::Number(left)), Some(Value::Number(right))) => {
            match (left.as_f64(), right.as_f64()) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        (Some(Value::String(left)), Some(Value::String(right))) => left == right,
        _ => false,
    }
}

// ─── Field normalizers (agent-status-field-normalization.ts) ────────────────

/// Trim, fold separator runs to one space, cap — with the source scan bounded
/// so a paste-sized field cannot cost a full pass just to render a small label.
fn normalize_single_line_preview(units: &[u16], max_length: usize) -> Vec<u16> {
    let scan_end = units.len().min(
        max_length
            .saturating_mul(SINGLE_LINE_FIELD_SCAN_MULTIPLIER)
            .saturating_add(SINGLE_LINE_FIELD_SCAN_OVERHEAD),
    );
    let mut index = 0;
    while index < scan_end && units.get(index).copied().is_some_and(is_ecma_trim_whitespace) {
        index += 1;
    }

    let mut normalized: Vec<u16> = Vec::new();
    let mut line_separator_run = false;
    while index < scan_end && normalized.len() < max_length {
        let Some(unit) = units.get(index).copied() else {
            break;
        };
        if is_single_line_separator(unit) {
            // The CRLF lookahead reads the raw source, not the scan window.
            if unit == 0x0d && units.get(index.saturating_add(1)) == Some(&0x0a) {
                index += 1;
            }
            if !line_separator_run {
                normalized.push(0x20);
            }
            line_separator_run = true;
            index += 1;
            continue;
        }
        normalized.push(unit);
        line_separator_run = false;
        index += 1;
    }

    if normalized.len() < max_length {
        while normalized.last().copied().is_some_and(is_ecma_trim_whitespace) {
            normalized.pop();
        }
    }
    truncate_preserving_surrogates(&normalized, max_length)
}

/// Assistant messages render with `whitespace-pre-wrap`, so line breaks are
/// content: fold `\r\n`/`\r`/U+2028/U+2029 to `\n` and cap blank-line runs at
/// one instead of collapsing to a single line.
fn normalize_multiline_field(units: &[u16], max_length: usize) -> Vec<u16> {
    let (start, end) = trimmed_bounds(units);
    let mut normalized: Vec<u16> = Vec::new();
    let mut newline_run = 0usize;
    let mut index = start;
    while index < end && normalized.len() < max_length {
        let Some(unit) = units.get(index).copied() else {
            break;
        };
        if is_single_line_separator(unit) {
            if unit == 0x0d && units.get(index.saturating_add(1)) == Some(&0x0a) {
                index += 1;
            }
            if newline_run < 2 {
                normalized.push(0x0a);
            }
            newline_run += 1;
            index += 1;
            continue;
        }
        normalized.push(unit);
        newline_run = 0;
        index += 1;
    }
    truncate_preserving_surrogates(&normalized, max_length)
}

fn normalize_optional_field(value: Option<&Value>, max_length: usize) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    let normalized = string_of(&normalize_single_line_preview(&units_of(text), max_length));
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_optional_multiline_field(value: Option<&Value>, max_length: usize) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    let normalized = string_of(&normalize_multiline_field(&units_of(text), max_length));
    (!normalized.is_empty()).then_some(normalized)
}

/// `interactivePrompt` carries raw AskUserQuestion JSON the client parses back,
/// so unlike every other field it is NOT trimmed and NOT line-folded — only
/// capped (surrogate-safely) and emptied to `None`.
fn normalize_interactive_prompt_field(value: Option<&Value>, max_length: usize) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    if text.is_empty() {
        return None;
    }
    let truncated = string_of(&truncate_preserving_surrogates(&units_of(text), max_length));
    (!truncated.is_empty()).then_some(truncated)
}

// ─── Dispatch preamble compaction (orca-dispatch-status-prompt.ts) ──────────

/// Bounded so leading whitespace cannot walk a multi-MB paste before the
/// normalizer's own scan cap applies.
fn is_orca_dispatch_status_prompt(units: &[u16]) -> bool {
    let scan_end = units.len().min(ORCA_DISPATCH_STATUS_SOURCE_SCAN_LIMIT);
    let mut start = 0;
    while start < scan_end && units.get(start).copied().is_some_and(is_ecma_trim_whitespace) {
        start += 1;
    }
    let prefix = units_of(ORCA_DISPATCH_STATUS_PREAMBLE_PREFIX);
    start.saturating_add(prefix.len()) <= scan_end
        && units.get(start..start.saturating_add(prefix.len())) == Some(prefix.as_slice())
}

/// The standalone-line `=== TASK ===`, so a base-drift commit subject quoting
/// the marker cannot impersonate it. Already-compacted single-line previews
/// keep the marker inline on purpose, so re-normalization stays idempotent.
fn find_orca_dispatch_task_marker_index(units: &[u16], marker: &[u16]) -> Option<usize> {
    let mut search_from = 0usize;
    while search_from < units.len() {
        let Some(relative) = find_units(units.get(search_from..).unwrap_or(&[]), marker) else {
            break;
        };
        let marker_index = search_from.saturating_add(relative);
        let marker_end = marker_index.saturating_add(marker.len());
        let starts_line = marker_index == 0
            || marker_index
                .checked_sub(1)
                .and_then(|index| units.get(index).copied())
                .is_some_and(is_line_break);
        let ends_line = marker_end == units.len()
            || units.get(marker_end).copied().is_some_and(is_line_break);
        if starts_line && ends_line {
            return Some(marker_index);
        }
        search_from = marker_end;
    }
    if units.iter().copied().any(is_line_break) {
        None
    } else {
        find_units(units, marker)
    }
}

/// `body.split(/\r?\n/)` — a bare `\r` is NOT a separator there.
fn split_lf_lines(units: &[u16]) -> Vec<&[u16]> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    for (index, unit) in units.iter().enumerate() {
        if *unit != 0x0a {
            continue;
        }
        let mut line_end = index;
        if index
            .checked_sub(1)
            .and_then(|previous| units.get(previous))
            == Some(&0x0d)
        {
            line_end = index.saturating_sub(1);
        }
        lines.push(units.get(line_start..line_end).unwrap_or(&[]));
        line_start = index.saturating_add(1);
    }
    lines.push(units.get(line_start..).unwrap_or(&[]));
    lines
}

/// `line.trim().replace(/\s+/g, ' ')`.
fn collapse_whitespace_runs(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut in_run = false;
    for unit in trimmed(units).iter().copied() {
        if is_ecma_trim_whitespace(unit) {
            if !in_run {
                out.push(0x20);
            }
            in_run = true;
            continue;
        }
        out.push(unit);
        in_run = false;
    }
    out
}

/// Collapse a multi-KB dispatch preamble to
/// `<prefix> Your task ID is: <id> === TASK === <body>` so preamble detection,
/// the task id and the task body all survive the 200-unit field cap.
fn compact_dispatch_prompt_for_status(units: &[u16], max_length: usize) -> Vec<u16> {
    let scan_end = units.len().min(ORCA_DISPATCH_STATUS_SOURCE_SCAN_LIMIT);
    let mut start = 0;
    while start < scan_end && units.get(start).copied().is_some_and(is_ecma_trim_whitespace) {
        start += 1;
    }
    let scan = units.get(start..scan_end).unwrap_or(&[]);

    let id_marker = units_of(ORCA_DISPATCH_STATUS_TASK_ID_MARKER);
    let mut task_id: Vec<u16> = Vec::new();
    if let Some(marker_index) = find_units(scan, &id_marker) {
        let after = scan
            .get(marker_index.saturating_add(id_marker.len())..)
            .unwrap_or(&[]);
        let mut id_start = 0usize;
        while id_start < after.len()
            && after.get(id_start).copied().is_some_and(is_ecma_trim_whitespace)
        {
            id_start += 1;
        }
        let id_rest = after.get(id_start..).unwrap_or(&[]);
        let id_end = id_rest
            .iter()
            .copied()
            .position(is_ecma_trim_whitespace)
            .unwrap_or(id_rest.len());
        task_id = trimmed(id_rest.get(..id_end).unwrap_or(&[])).to_vec();
    }

    let task_marker = units_of(ORCA_DISPATCH_STATUS_TASK_MARKER);
    let mut task_body: Vec<u16> = Vec::new();
    if let Some(marker_index) = find_orca_dispatch_task_marker_index(scan, &task_marker) {
        let body = scan
            .get(marker_index.saturating_add(task_marker.len())..)
            .unwrap_or(&[]);
        for line in split_lf_lines(body) {
            let preview = collapse_whitespace_runs(line);
            if !preview.is_empty() {
                task_body = preview;
                break;
            }
        }
    }

    let mut compact = units_of(ORCA_DISPATCH_STATUS_PREAMBLE_PREFIX);
    if !task_id.is_empty() {
        compact.push(0x20);
        compact.extend_from_slice(&id_marker);
        compact.push(0x20);
        compact.extend_from_slice(&task_id);
    }
    if !task_body.is_empty() {
        compact.push(0x20);
        compact.extend_from_slice(&task_marker);
        compact.push(0x20);
        compact.extend_from_slice(&task_body);
    }
    normalize_single_line_preview(&compact, max_length)
}

fn normalize_prompt_field(value: Option<&Value>) -> String {
    let Some(text) = value.and_then(Value::as_str) else {
        return String::new();
    };
    let units = units_of(text);
    let normalized = if is_orca_dispatch_status_prompt(&units) {
        compact_dispatch_prompt_for_status(&units, AGENT_STATUS_MAX_FIELD_LENGTH)
    } else {
        normalize_single_line_preview(&units, AGENT_STATUS_MAX_FIELD_LENGTH)
    };
    string_of(&normalized)
}

// ─── Pre-parse structure guard (json-text-structure-limit.ts) ───────────────

/// Reject pathological structure BEFORE handing the text to the JSON parser.
/// Only ASCII punctuation is inspected, so scanning `char`s is the same walk
/// the twin makes over UTF-16 units.
fn json_text_structure_within_limits(
    content: &str,
    structural_token_limit: usize,
    nesting_depth_limit: usize,
) -> bool {
    let mut structural_tokens = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for character in content.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            continue;
        }
        if !matches!(character, '{' | '}' | '[' | ']' | ',' | ':') {
            continue;
        }
        structural_tokens = structural_tokens.saturating_add(1);
        if structural_tokens > structural_token_limit {
            return false;
        }
        if character == '{' || character == '[' {
            depth = depth.saturating_add(1);
            if depth > nesting_depth_limit {
                return false;
            }
        } else if character == '}' || character == ']' {
            depth = depth.saturating_sub(1);
        }
    }
    true
}

// ─── Subagents ──────────────────────────────────────────────────────────────

fn normalize_subagent_snapshot(value: &Value) -> Option<AgentSubagentSnapshot> {
    let object = value.as_object()?;
    let id_units = trimmed(&units_of(object.get("id").and_then(Value::as_str)?)).to_vec();
    if id_units.is_empty() || id_units.len() > AGENT_SUBAGENT_ID_MAX_LENGTH {
        return None;
    }
    let state = AgentSubagentState::from_id(object.get("state").and_then(Value::as_str)?)?;
    Some(AgentSubagentSnapshot {
        id: string_of(&id_units),
        state,
        started_at: object
            .get("startedAt")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0),
        agent_type: normalize_optional_field(object.get("agentType"), AGENT_TYPE_MAX_LENGTH),
        model: normalize_optional_field(object.get("model"), AGENT_MODEL_MAX_LENGTH),
        description: normalize_optional_field(
            object.get("description"),
            AGENT_STATUS_TOOL_INPUT_MAX_LENGTH,
        ),
    })
}

fn normalize_subagents_field(value: Option<&Value>) -> Option<Vec<AgentSubagentSnapshot>> {
    let items = value.and_then(Value::as_array)?;
    if items.is_empty() {
        return None;
    }
    let mut normalized = Vec::new();
    for item in items {
        if let Some(snapshot) = normalize_subagent_snapshot(item) {
            normalized.push(snapshot);
            if normalized.len() >= AGENT_STATUS_MAX_SUBAGENTS {
                break;
            }
        }
    }
    (!normalized.is_empty()).then_some(normalized)
}

/// Structural equality so callers can reuse the previous list (and skip fanout)
/// when nothing actually changed.
pub fn agent_subagents_equal(
    a: Option<&[AgentSubagentSnapshot]>,
    b: Option<&[AgentSubagentSnapshot]>,
) -> bool {
    match (a, b) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        _ => false,
    }
}

/// The same comparison over the values as they arrive, before any
/// normalization. THIS is the twin's contract, and it is not the same question
/// as [`agent_subagents_equal`]: the twin never coerces, so a snapshot whose
/// `startedAt` is absent is unequal to one whose `startedAt` is `0`, while the
/// normalizer folds both to `0`. Comparing normalized snapshots would answer
/// "equal" and suppress a fanout the twin performs.
///
/// `None` means "the twin does something no JSON value can reproduce here, and
/// this port declines to guess". Exactly two shapes reach it, both outside the
/// declared `AgentSubagentSnapshot[] | undefined` type:
///   * a truthy non-array operand — the twin reads `.length` and `[i]` off it,
///     so answering would mean modelling strings, plain objects and array-likes
///     as indexable collections;
///   * a `null` element the twin actually reaches — it dereferences `null.id`
///     and throws. Elements the twin never reaches (past a length mismatch, or
///     after an earlier pair already decided the answer) are not refused.
pub fn agent_subagents_equal_values(a: Option<&Value>, b: Option<&Value>) -> Option<bool> {
    let left_truthy = a.is_some_and(is_js_truthy);
    let right_truthy = b.is_some_and(is_js_truthy);
    // `if (!a || !b || a.length !== b.length) return !a && !b`
    if !left_truthy || !right_truthy {
        return Some(!left_truthy && !right_truthy);
    }
    let (Some(left), Some(right)) = (a.and_then(Value::as_array), b.and_then(Value::as_array))
    else {
        return None;
    };
    if left.len() != right.len() {
        return Some(false);
    }
    // Walked in the twin's own order so a refusal happens only where the twin
    // would actually throw, not merely where a null exists somewhere.
    for (x, y) in left.iter().zip(right.iter()) {
        if x.is_null() || y.is_null() {
            return None;
        }
        let equal = SUBAGENT_EQUALITY_FIELDS
            .iter()
            .all(|field| js_strict_equals(x.get(field), y.get(field)));
        if !equal {
            return Some(false);
        }
    }
    Some(true)
}

// ─── Status predicates ──────────────────────────────────────────────────────

/// Provider `done` hooks can fire mid-Dispatch, so only runtime-confirmed
/// settlement makes sleeping a pane safe. An ABSENT `dispatchStatus` counts as
/// unsettled on purpose — a hook-only context proves nothing about the dispatch
/// — which is why this cannot be written as "is one of the unsettled ids".
pub fn has_unsettled_or_unknown_dispatch(entry: &Value) -> bool {
    // `if (!entry.orchestration) return false` — falsy, not merely absent.
    let Some(orchestration) = entry.get("orchestration").filter(|value| is_js_truthy(value)) else {
        return false;
    };
    let status = orchestration.get("dispatchStatus").and_then(Value::as_str);
    !SETTLED_DISPATCH_STATUSES
        .iter()
        .any(|settled| status == Some(*settled))
}

/// `Boolean(entry && entry.state !== 'done' && now - entry.updatedAt <= staleAfterMs)`.
///
/// `now` is required rather than defaulted: the twin's default is `Date.now()`,
/// a clock read, and a port that invented one would answer a different question
/// on every call. `stale_after_ms` IS defaulted, because its default is a
/// constant — and `Some(Value::Null)` is not the same as absent there, exactly
/// as an explicit `null` argument skips a JS default parameter.
pub fn is_fresh_non_done_agent_status(
    entry: Option<&Value>,
    now: &Value,
    stale_after_ms: Option<&Value>,
) -> bool {
    let Some(entry) = entry.filter(|value| is_js_truthy(value)) else {
        return false;
    };
    if entry.get("state").and_then(Value::as_str) == Some("done") {
        return false;
    }
    let stale_after_ms = match stale_after_ms {
        None => AGENT_STATUS_STALE_AFTER_MS,
        Some(value) => js_to_number(Some(value)),
    };
    // A missing/non-numeric `updatedAt` yields NaN, and every NaN comparison is
    // false — the twin's answer for a row that never reported a timestamp.
    js_to_number(Some(now)) - js_to_number(entry.get("updatedAt")) <= stale_after_ms
}

/// A `startedAt` back on the wire the way `JSON.stringify` would print it:
/// integral JS numbers have no fractional tail.
pub fn started_at_to_json(started_at: f64) -> Value {
    if started_at.fract() == 0.0 && started_at.abs() <= 9_007_199_254_740_991.0 {
        return Value::Number(Number::from(started_at as i64));
    }
    Number::from_f64(started_at).map_or(Value::Number(Number::from(0)), Value::Number)
}

// ─── Entry points ───────────────────────────────────────────────────────────

fn normalize_agent_status_object(parsed: &Value) -> Option<ParsedAgentStatusPayload> {
    let object = parsed.as_object()?;
    let state = AgentStatusState::from_id(object.get("state").and_then(Value::as_str)?)?;
    let done = state == AgentStatusState::Done;
    Some(ParsedAgentStatusPayload {
        state,
        prompt: normalize_prompt_field(object.get("prompt")),
        agent_type: normalize_optional_field(object.get("agentType"), AGENT_TYPE_MAX_LENGTH),
        model: normalize_optional_field(object.get("model"), AGENT_MODEL_MAX_LENGTH),
        tool_name: normalize_optional_field(
            object.get("toolName"),
            AGENT_STATUS_TOOL_NAME_MAX_LENGTH,
        ),
        tool_input: normalize_optional_field(
            object.get("toolInput"),
            AGENT_STATUS_TOOL_INPUT_MAX_LENGTH,
        ),
        interactive_prompt: normalize_interactive_prompt_field(
            object.get("interactivePrompt"),
            AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH,
        ),
        last_assistant_message: normalize_optional_multiline_field(
            object.get("lastAssistantMessage"),
            AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH,
        ),
        // Only meaningful on `done`; require a strict boolean `true`.
        interrupted: (object.get("interrupted") == Some(&Value::Bool(true)) && done).then_some(true),
        launch_failed: (object.get("launchFailed") == Some(&Value::Bool(true)) && done)
            .then_some(true),
        subagents: normalize_subagents_field(object.get("subagents")),
    })
}

pub fn normalize_agent_status_payload(payload: &Value) -> Option<ParsedAgentStatusPayload> {
    normalize_agent_status_object(payload)
}

pub fn parse_agent_status_payload(json: &str) -> Option<ParsedAgentStatusPayload> {
    if !json_text_structure_within_limits(
        json,
        AGENT_STATUS_JSON_STRUCTURAL_TOKEN_LIMIT,
        AGENT_STATUS_JSON_NESTING_DEPTH_LIMIT,
    ) {
        return None;
    }
    let parsed: Value = serde_json::from_str(json).ok()?;
    normalize_agent_status_object(&parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use AgentStatusState::Working;

    fn utf16_len(value: &str) -> usize {
        value.encode_utf16().count()
    }

    fn parse(json: &str) -> ParsedAgentStatusPayload {
        parse_agent_status_payload(json).expect("payload should parse")
    }

    fn empty_payload(state: AgentStatusState) -> ParsedAgentStatusPayload {
        ParsedAgentStatusPayload {
            state,
            prompt: String::new(),
            agent_type: None,
            model: None,
            tool_name: None,
            tool_input: None,
            interactive_prompt: None,
            last_assistant_message: None,
            interrupted: None,
            launch_failed: None,
            subagents: None,
        }
    }

    #[test]
    fn parses_a_valid_working_payload() {
        assert_eq!(
            parse_agent_status_payload(
                r#"{"state":"working","prompt":"Fix the flaky assertion","agentType":"codex"}"#
            ),
            Some(ParsedAgentStatusPayload {
                prompt: "Fix the flaky assertion".to_string(),
                agent_type: Some("codex".to_string()),
                ..empty_payload(Working)
            })
        );
    }

    #[test]
    fn parses_all_agent_status_states() {
        for state in AGENT_STATUS_STATES {
            let result = parse(&format!(r#"{{"state":"{state}"}}"#));
            assert_eq!(AgentStatusState::from_id(state), Some(result.state));
        }
    }

    #[test]
    fn returns_none_for_invalid_state() {
        assert_eq!(parse_agent_status_payload(r#"{"state":"running"}"#), None);
        assert_eq!(parse_agent_status_payload(r#"{"state":"idle"}"#), None);
        assert_eq!(parse_agent_status_payload(r#"{"state":""}"#), None);
    }

    #[test]
    fn returns_none_when_state_is_a_non_string_type() {
        assert_eq!(parse_agent_status_payload(r#"{"state":123}"#), None);
        assert_eq!(parse_agent_status_payload(r#"{"state":true}"#), None);
        assert_eq!(parse_agent_status_payload(r#"{"state":null}"#), None);
    }

    #[test]
    fn returns_none_for_invalid_json() {
        assert_eq!(parse_agent_status_payload("not json"), None);
        assert_eq!(parse_agent_status_payload("{broken"), None);
        assert_eq!(parse_agent_status_payload(""), None);
    }

    #[test]
    fn rejects_excessive_nesting_before_json_parse() {
        let depth = AGENT_STATUS_JSON_NESTING_DEPTH_LIMIT + 1;
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert!(!json_text_structure_within_limits(
            &nested,
            AGENT_STATUS_JSON_STRUCTURAL_TOKEN_LIMIT,
            AGENT_STATUS_JSON_NESTING_DEPTH_LIMIT
        ));
        assert_eq!(parse_agent_status_payload(&nested), None);
    }

    #[test]
    fn rejects_excessive_structural_tokens_before_json_parse() {
        // Why: valid JSON with a valid state, so ONLY the pre-parse guard can
        // reject it — the twin's `assertJsonTextStructureWithinLimits` gate.
        let entries = (0..AGENT_STATUS_JSON_STRUCTURAL_TOKEN_LIMIT)
            .map(|index| format!(r#"{{"id":"x{index}","state":"idle"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(r#"{{"state":"working","subagents":[{entries}]}}"#);
        assert!(serde_json::from_str::<Value>(&json).is_ok());
        assert_eq!(parse_agent_status_payload(&json), None);
    }

    #[test]
    fn structure_guard_ignores_tokens_inside_strings() {
        let braces = "{".repeat(AGENT_STATUS_JSON_NESTING_DEPTH_LIMIT + 5);
        let json = json!({ "state": "working", "toolInput": braces }).to_string();
        assert_eq!(parse(&json).tool_input.as_deref(), Some(braces.as_str()));
    }

    #[test]
    fn returns_none_for_non_object_json() {
        assert_eq!(parse_agent_status_payload(r#""just a string""#), None);
        assert_eq!(parse_agent_status_payload("42"), None);
        assert_eq!(parse_agent_status_payload("null"), None);
        assert_eq!(parse_agent_status_payload("[]"), None);
    }

    #[test]
    fn normalizes_multiline_and_crlf_prompts_to_single_line() {
        assert_eq!(
            parse(r#"{"state":"working","prompt":"line one\nline two\nline three"}"#).prompt,
            "line one line two line three"
        );
        assert_eq!(
            parse(r#"{"state":"working","prompt":"line one\r\nline two\r\nline three"}"#).prompt,
            "line one line two line three"
        );
    }

    #[test]
    fn trims_and_truncates_and_defaults_the_prompt() {
        assert_eq!(
            parse(r#"{"state":"working","prompt":"  padded  "}"#).prompt,
            "padded"
        );
        let long = format!(r#"{{"state":"working","prompt":"{}"}}"#, "x".repeat(300));
        assert_eq!(utf16_len(&parse(&long).prompt), AGENT_STATUS_MAX_FIELD_LENGTH);
        assert_eq!(parse(r#"{"state":"done"}"#).prompt, "");
        assert_eq!(parse(r#"{"state":"working","prompt":42}"#).prompt, "");
    }

    #[test]
    fn compacts_orca_dispatch_preambles_so_the_task_body_survives_truncation() {
        let cli_noise = (0..50)
            .map(|index| {
                format!(
                    "orca orchestration send --to term_parent --type heartbeat --phase step-{index}"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "You are working inside Orca, a multi-agent IDE. You are a dispatched worker.\n\
             Your task ID is: task_compact_1\n\n\
             === CLI COMMANDS ===\n{cli_noise}\n\n\
             === TASK ===\nFix dispatch fallback preview for normalized status prompts"
        );
        let result = parse(&json!({ "state": "working", "prompt": prompt }).to_string());

        assert!(utf16_len(&result.prompt) <= AGENT_STATUS_MAX_FIELD_LENGTH);
        assert!(!result.prompt.contains('\n'));
        assert!(result
            .prompt
            .starts_with("You are working inside Orca, a multi-agent IDE."));
        assert!(result.prompt.contains("Your task ID is: task_compact_1"));
        assert!(result.prompt.contains("=== TASK ==="));
        assert!(result.prompt.contains("Fix dispatch fallback preview"));
        assert!(!result.prompt.contains("CLI COMMANDS"));
        assert!(!result.prompt.contains("heartbeat"));
    }

    #[test]
    fn ignores_task_marker_text_inside_base_drift_commit_subjects() {
        // Why: CRLF covers Windows hook payloads; commit text must not impersonate the separator.
        let prompt = [
            "You are working inside Orca, a multi-agent IDE. You are a dispatched worker.",
            "Your task ID is: task_drift_marker",
            "",
            "--- BASE DRIFT ---",
            "  - docs: explain === TASK === marker parsing",
            "---",
            "",
            "=== TASK ===",
            "Fix the actual dispatch fallback preview",
        ]
        .join("\r\n");
        let result = normalize_agent_status_payload(&json!({ "state": "working", "prompt": prompt }))
            .expect("payload should normalize");

        assert!(result
            .prompt
            .contains("=== TASK === Fix the actual dispatch fallback preview"));
        assert!(!result.prompt.contains("marker parsing"));
    }

    #[test]
    fn keeps_dispatch_detection_bounded_for_oversized_whitespace_prompts() {
        let prompt = " ".repeat(1_000_000);
        let result = normalize_agent_status_payload(&json!({ "state": "working", "prompt": prompt }))
            .expect("payload should normalize");
        assert_eq!(result.prompt, "");
    }

    #[test]
    fn keeps_an_already_compacted_dispatch_preview_idempotent() {
        // Why: a single-line preview keeps the marker inline, and the status
        // prompt is re-normalized on every hop.
        let once = parse(
            &json!({
                "state": "working",
                "prompt": "You are working inside Orca, a multi-agent IDE.\nYour task ID is: task_hop\n\n=== TASK ===\nShip the thing"
            })
            .to_string(),
        )
        .prompt;
        let twice = parse(&json!({ "state": "working", "prompt": &once }).to_string()).prompt;
        assert_eq!(once, twice);
        assert_eq!(
            once,
            "You are working inside Orca, a multi-agent IDE. Your task ID is: task_hop === TASK === Ship the thing"
        );
    }

    #[test]
    fn leaves_non_dispatch_prompts_uncompacted() {
        let prompt = "Your task ID is: not_a_dispatch\n=== TASK ===\nbody";
        assert_eq!(
            parse(&json!({ "state": "working", "prompt": prompt }).to_string()).prompt,
            "Your task ID is: not_a_dispatch === TASK === body"
        );
    }

    #[test]
    fn bounds_single_line_scanning_when_previews_are_mostly_line_breaks() {
        let prompt = format!("Summary{}Details", "\n".repeat(10_000));
        let result = normalize_agent_status_payload(&json!({ "state": "working", "prompt": prompt }))
            .expect("payload should normalize");
        assert_eq!(result.prompt, "Summary");
    }

    #[test]
    fn normalizes_large_single_line_preview_fields_within_the_scan_bound() {
        let prompt = format!("Summary\r\nDetails {}", "x".repeat(20_000));
        let tool_input = format!("src/index.ts\u{2028}{}", "line\n".repeat(10_000));
        let result = normalize_agent_status_payload(
            &json!({ "state": "working", "prompt": prompt, "toolInput": tool_input }),
        )
        .expect("payload should normalize");

        assert!(result.prompt.starts_with("Summary Details "));
        assert_eq!(utf16_len(&result.prompt), AGENT_STATUS_MAX_FIELD_LENGTH);
        let tool_input = result.tool_input.expect("toolInput should survive");
        assert!(tool_input.starts_with("src/index.ts "));
        assert!(utf16_len(&tool_input) <= AGENT_STATUS_TOOL_INPUT_MAX_LENGTH);
    }

    #[test]
    fn handles_agent_type_field() {
        assert_eq!(
            parse(r#"{"state":"working","agentType":"cursor"}"#)
                .agent_type
                .as_deref(),
            Some("cursor")
        );
        let long =
            json!({ "state": "working", "agentType": "a".repeat(AGENT_TYPE_MAX_LENGTH + 20) })
                .to_string();
        assert_eq!(
            utf16_len(parse(&long).agent_type.as_deref().unwrap_or_default()),
            AGENT_TYPE_MAX_LENGTH
        );
        assert_eq!(
            parse(r#"{"state":"working","agentType":"   "}"#).agent_type,
            None
        );
        assert_eq!(
            parse(r#"{"state":"working","agentType":"claude\nrogue"}"#)
                .agent_type
                .as_deref(),
            Some("claude rogue")
        );
    }

    #[test]
    fn parses_and_caps_the_model_field() {
        assert_eq!(
            parse(r#"{"state":"working","model":"gpt-5.4-mini"}"#)
                .model
                .as_deref(),
            Some("gpt-5.4-mini")
        );
        let long =
            json!({ "state": "working", "model": "m".repeat(AGENT_MODEL_MAX_LENGTH + 40) })
                .to_string();
        assert_eq!(
            utf16_len(parse(&long).model.as_deref().unwrap_or_default()),
            AGENT_MODEL_MAX_LENGTH
        );
        assert_eq!(parse(r#"{"state":"working","model":42}"#).model, None);
    }

    #[test]
    fn parses_and_caps_optional_fields() {
        let result = parse(
            r#"{"state":"working","toolName":"Edit","toolInput":"/path/to/file.ts","lastAssistantMessage":"Here is the edit I made."}"#,
        );
        assert_eq!(result.tool_name.as_deref(), Some("Edit"));
        assert_eq!(result.tool_input.as_deref(), Some("/path/to/file.ts"));
        assert_eq!(
            result.last_assistant_message.as_deref(),
            Some("Here is the edit I made.")
        );

        let long = json!({
            "state": "working",
            "toolName": "n".repeat(AGENT_STATUS_TOOL_NAME_MAX_LENGTH + 50),
            "toolInput": "i".repeat(AGENT_STATUS_TOOL_INPUT_MAX_LENGTH + 50),
            "lastAssistantMessage": "m".repeat(AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH + 500),
        })
        .to_string();
        let result = parse(&long);
        assert_eq!(
            utf16_len(result.tool_name.as_deref().unwrap_or_default()),
            AGENT_STATUS_TOOL_NAME_MAX_LENGTH
        );
        assert_eq!(
            utf16_len(result.tool_input.as_deref().unwrap_or_default()),
            AGENT_STATUS_TOOL_INPUT_MAX_LENGTH
        );
        assert_eq!(
            utf16_len(result.last_assistant_message.as_deref().unwrap_or_default()),
            AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH
        );
    }

    #[test]
    fn parses_interactive_prompt_without_single_line_collapse() {
        let interactive_prompt = r#"{"questions":[{"question":"Pick one","options":["a","b"]}]}"#;
        let result = parse(
            &json!({ "state": "waiting", "interactivePrompt": interactive_prompt }).to_string(),
        );
        assert_eq!(
            result.interactive_prompt.as_deref(),
            Some(interactive_prompt)
        );
    }

    #[test]
    fn preserves_newlines_inside_interactive_prompt_json() {
        let interactive_prompt = "{\n  \"questions\": []\n}";
        let result = parse(
            &json!({ "state": "waiting", "interactivePrompt": interactive_prompt }).to_string(),
        );
        assert_eq!(
            result.interactive_prompt.as_deref(),
            Some(interactive_prompt)
        );
    }

    #[test]
    fn caps_interactive_prompt_at_its_generous_max_length() {
        let long = "x".repeat(AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH + 500);
        let result = parse(&json!({ "state": "waiting", "interactivePrompt": long }).to_string());
        assert_eq!(
            utf16_len(result.interactive_prompt.as_deref().unwrap_or_default()),
            AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH
        );
        assert_eq!(AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH, 16000);
    }

    #[test]
    fn leaves_interactive_prompt_none_when_absent_or_non_string() {
        assert_eq!(parse(r#"{"state":"working"}"#).interactive_prompt, None);
        assert_eq!(
            parse(r#"{"state":"working","interactivePrompt":42}"#).interactive_prompt,
            None
        );
        assert_eq!(
            parse(r#"{"state":"working","interactivePrompt":""}"#).interactive_prompt,
            None
        );
    }

    #[test]
    fn keeps_untrimmed_whitespace_inside_interactive_prompt() {
        // Why: unlike every other field, interactivePrompt must survive byte-identical.
        let interactive_prompt = "  {\"questions\": []}  ";
        let result = parse(
            &json!({ "state": "waiting", "interactivePrompt": interactive_prompt }).to_string(),
        );
        assert_eq!(
            result.interactive_prompt.as_deref(),
            Some(interactive_prompt)
        );
    }

    #[test]
    fn treats_missing_empty_and_non_string_optional_fields_as_none() {
        let omitted = parse(r#"{"state":"working"}"#);
        assert_eq!(
            (
                omitted.tool_name,
                omitted.tool_input,
                omitted.last_assistant_message
            ),
            (None, None, None)
        );
        let non_string = parse(
            r#"{"state":"working","toolName":42,"toolInput":null,"lastAssistantMessage":[]}"#,
        );
        assert_eq!(
            (
                non_string.tool_name,
                non_string.tool_input,
                non_string.last_assistant_message
            ),
            (None, None, None)
        );
        let empty = parse(
            r#"{"state":"working","toolName":"   ","toolInput":"","lastAssistantMessage":"   "}"#,
        );
        assert_eq!(
            (
                empty.tool_name,
                empty.tool_input,
                empty.last_assistant_message
            ),
            (None, None, None)
        );
    }

    #[test]
    fn collapses_newlines_in_tool_input_single_line_field() {
        assert_eq!(
            parse(r#"{"state":"working","toolInput":"line one\nline two"}"#)
                .tool_input
                .as_deref(),
            Some("line one line two")
        );
    }

    #[test]
    fn preserves_and_caps_paragraph_breaks_in_last_assistant_message() {
        assert_eq!(
            parse(r#"{"state":"done","lastAssistantMessage":"Summary line.\n\nDetails paragraph."}"#)
                .last_assistant_message
                .as_deref(),
            Some("Summary line.\n\nDetails paragraph.")
        );
        assert_eq!(
            parse(r#"{"state":"done","lastAssistantMessage":"a\r\nb\n\n\n\nc"}"#)
                .last_assistant_message
                .as_deref(),
            Some("a\nb\n\nc")
        );
    }

    #[test]
    fn folds_unicode_separators_and_caps_blank_line_runs() {
        let line_sep = parse(
            "{\"state\":\"done\",\"lastAssistantMessage\":\"a\u{2028}\u{2028}\u{2028}\u{2028}b\"}",
        );
        assert_eq!(line_sep.last_assistant_message.as_deref(), Some("a\n\nb"));
        let para_sep = parse(
            "{\"state\":\"done\",\"lastAssistantMessage\":\"a\u{2029}\u{2029}\u{2029}\u{2029}b\"}",
        );
        assert_eq!(para_sep.last_assistant_message.as_deref(), Some("a\n\nb"));
        let mixed = parse(
            "{\"state\":\"done\",\"lastAssistantMessage\":\"a\u{2028}\u{2029}\\n\u{2028}\u{2029}b\"}",
        );
        assert_eq!(mixed.last_assistant_message.as_deref(), Some("a\n\nb"));
    }

    #[test]
    fn respects_prompt_cap_independent_of_other_fields() {
        let json =
            json!({ "state": "working", "prompt": "p".repeat(300), "toolInput": "xxxxx" })
                .to_string();
        let result = parse(&json);
        assert_eq!(utf16_len(&result.prompt), AGENT_STATUS_MAX_FIELD_LENGTH);
        assert_eq!(result.tool_input.as_deref(), Some("xxxxx"));
    }

    #[test]
    fn handles_interrupted_with_strict_boolean_and_done_state() {
        assert_eq!(
            parse(r#"{"state":"done","interrupted":true}"#).interrupted,
            Some(true)
        );
        for state in ["working", "blocked", "waiting"] {
            assert_eq!(
                parse(&format!(r#"{{"state":"{state}","interrupted":true}}"#)).interrupted,
                None
            );
        }
        assert_eq!(
            parse(r#"{"state":"done","interrupted":"true"}"#).interrupted,
            None
        );
        assert_eq!(parse(r#"{"state":"done","interrupted":1}"#).interrupted, None);
        assert_eq!(
            parse(r#"{"state":"done","interrupted":"yes"}"#).interrupted,
            None
        );
    }

    #[test]
    fn handles_launch_failed_with_strict_boolean_and_done_state() {
        assert_eq!(
            parse(r#"{"state":"done","launchFailed":true}"#).launch_failed,
            Some(true)
        );
        for state in ["working", "blocked", "waiting"] {
            assert_eq!(
                parse(&format!(r#"{{"state":"{state}","launchFailed":true}}"#)).launch_failed,
                None
            );
        }
        assert_eq!(
            parse(r#"{"state":"done","launchFailed":"true"}"#).launch_failed,
            None
        );
        assert_eq!(
            parse(r#"{"state":"done","launchFailed":1}"#).launch_failed,
            None
        );
    }

    #[test]
    fn never_leaves_a_lone_high_surrogate_when_truncating() {
        let prompt = format!("x{}", "😀".repeat(AGENT_STATUS_MAX_FIELD_LENGTH));
        let json = json!({ "state": "working", "prompt": prompt }).to_string();
        let units: Vec<u16> = parse(&json).prompt.encode_utf16().collect();
        assert!(units.len() <= AGENT_STATUS_MAX_FIELD_LENGTH);
        assert!(units.len() >= AGENT_STATUS_MAX_FIELD_LENGTH - 1);
        let last = units.last().copied().unwrap_or_default();
        assert!(!(0xd800..=0xdbff).contains(&last));
        if (0xdc00..=0xdfff).contains(&last) {
            let second_last = units
                .len()
                .checked_sub(2)
                .and_then(|index| units.get(index).copied())
                .unwrap_or_default();
            assert!((0xd800..=0xdbff).contains(&second_last));
        }
    }

    #[test]
    fn never_leaves_a_lone_high_surrogate_in_last_assistant_message() {
        let pairs = AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH / 2 + 1;
        let message = format!("x{}", "😀".repeat(pairs));
        let json = json!({ "state": "done", "lastAssistantMessage": message }).to_string();
        let units: Vec<u16> = parse(&json)
            .last_assistant_message
            .unwrap_or_default()
            .encode_utf16()
            .collect();
        assert!(units.len() <= AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH);
        assert!(units.len() >= AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH - 1);
        let last = units.last().copied().unwrap_or_default();
        assert!(!(0xd800..=0xdbff).contains(&last));
        if (0xdc00..=0xdfff).contains(&last) {
            let second_last = units
                .len()
                .checked_sub(2)
                .and_then(|index| units.get(index).copied())
                .unwrap_or_default();
            assert!((0xd800..=0xdbff).contains(&second_last));
        }
    }

    #[test]
    fn never_leaves_a_lone_high_surrogate_in_interactive_prompt() {
        let pairs = AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH / 2 + 1;
        let prompt = format!("x{}", "😀".repeat(pairs));
        let json = json!({ "state": "waiting", "interactivePrompt": prompt }).to_string();
        let units: Vec<u16> = parse(&json)
            .interactive_prompt
            .unwrap_or_default()
            .encode_utf16()
            .collect();
        assert!(units.len() <= AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH);
        assert!(units.len() >= AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH - 1);
        let last = units.last().copied().unwrap_or_default();
        assert!(!(0xd800..=0xdbff).contains(&last));
    }

    #[test]
    fn normalizes_the_subagents_field_dropping_invalid_entries_and_bounding_count() {
        let mut subagents = vec![
            json!({ "id": "a1", "state": "working", "startedAt": 100, "agentType": "general-purpose" }),
            json!({ "id": "r1", "state": "idle", "startedAt": "nope", "description": "line\none" }),
            json!({ "id": "", "state": "working", "startedAt": 1 }),
            json!({ "id": "bad-state", "state": "running", "startedAt": 1 }),
            json!("garbage"),
        ];
        for index in 0..AGENT_STATUS_MAX_SUBAGENTS + 5 {
            subagents.push(json!({ "id": format!("extra-{index}"), "state": "idle", "startedAt": index }));
        }
        let result = parse(&json!({ "state": "working", "subagents": subagents }).to_string());
        let parsed = result.subagents.expect("subagents should be present");

        assert_eq!(parsed.len(), AGENT_STATUS_MAX_SUBAGENTS);
        assert_eq!(
            parsed.first(),
            Some(&AgentSubagentSnapshot {
                id: "a1".to_string(),
                state: AgentSubagentState::Working,
                started_at: 100.0,
                agent_type: Some("general-purpose".to_string()),
                model: None,
                description: None,
            })
        );
        // Why: non-finite startedAt coerces to 0; descriptions fold to one line.
        let second = parsed.get(1).expect("second subagent");
        assert_eq!(second.id, "r1");
        assert_eq!(second.started_at, 0.0);
        assert_eq!(second.description.as_deref(), Some("line one"));
    }

    #[test]
    fn omits_subagents_when_absent_or_empty() {
        assert_eq!(parse(r#"{"state":"done"}"#).subagents, None);
        assert_eq!(parse(r#"{"state":"done","subagents":[]}"#).subagents, None);
        assert_eq!(
            parse(r#"{"state":"done","subagents":[{"id":"x","state":"running"}]}"#).subagents,
            None
        );
    }

    #[test]
    fn drops_subagent_ids_past_the_id_cap() {
        let long_id = "i".repeat(AGENT_SUBAGENT_ID_MAX_LENGTH + 1);
        let at_cap = "i".repeat(AGENT_SUBAGENT_ID_MAX_LENGTH);
        let json = json!({
            "state": "working",
            "subagents": [
                { "id": long_id, "state": "idle" },
                { "id": format!("  {at_cap}  "), "state": "idle" },
            ]
        })
        .to_string();
        let parsed = parse(&json).subagents.expect("subagents should be present");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.first().map(|entry| entry.id.as_str()), Some(at_cap.as_str()));
        assert_eq!(parsed.first().map(|entry| entry.started_at), Some(0.0));
    }

    #[test]
    fn compares_subagent_lists_structurally() {
        let snapshot = AgentSubagentSnapshot {
            id: "a1".to_string(),
            state: AgentSubagentState::Working,
            started_at: 1.0,
            agent_type: None,
            model: None,
            description: None,
        };
        let other = AgentSubagentSnapshot {
            state: AgentSubagentState::Idle,
            ..snapshot.clone()
        };
        let with_model = AgentSubagentSnapshot {
            model: Some("gpt-5.4-mini".to_string()),
            ..snapshot.clone()
        };
        let copy = snapshot.clone();
        let one = std::slice::from_ref(&snapshot);
        assert!(agent_subagents_equal(None, None));
        assert!(agent_subagents_equal(
            Some(one),
            Some(std::slice::from_ref(&copy))
        ));
        assert!(!agent_subagents_equal(Some(one), Some(&[other])));
        assert!(!agent_subagents_equal(Some(one), Some(&[with_model])));
        assert!(!agent_subagents_equal(Some(one), None));
        assert!(!agent_subagents_equal(None, Some(one)));
        assert!(!agent_subagents_equal(
            Some(one),
            Some(&[snapshot.clone(), snapshot.clone()])
        ));
    }

    #[test]
    fn normalize_matches_the_json_round_trip_for_every_literal_shape() {
        // The twin's `normalizeAgentStatusPayload matches the JSON round trip`
        // table, minus its lone-surrogate row: `\ud800` is not a Unicode scalar
        // value, so it cannot cross a Rust `String` at all (see the module note
        // in docs/rust-migration/ported-modules.md on the shim boundary).
        let cases = vec![
            json!({ "state": "working", "prompt": "p", "agentType": "grok", "toolName": "sh", "toolInput": "ls" }),
            json!({ "state": "done", "prompt": "", "agentType": "devin", "interrupted": true }),
            json!({ "state": "working", "prompt": "p", "agentType": "cursor" }),
            json!({ "state": "working", "prompt": "p", "agentType": "copilot", "interactivePrompt": r#"{"q":"pick {one}","options":["a","b"]}"# }),
            json!({ "state": "working", "prompt": "a\r\nb c", "agentType": "gemini", "lastAssistantMessage": "emoji \u{1f389} \u{65e5}\u{672c}\u{8a9e}\r\n\r\n\r\nmulti" }),
            json!({ "state": "working", "prompt": "p", "agentType": "amp", "lastAssistantMessage": "x".repeat(50_000) }),
            json!({ "state": "done", "prompt": "p", "agentType": "hermes", "toolName": "", "toolInput": "" }),
            json!({ "state": "working", "prompt": "p", "agentType": "droid", "toolInput": r#"{"nested":{"deep":{"deeper":[1,2,3]}}}"# }),
            json!({ "state": "working", "prompt": "p", "agentType": "kimi", "lastAssistantMessage": "\"escaped\" quotes and \\ backslashes" }),
            json!({ "state": "working", "prompt": "p", "agentType": "opencode" }),
            json!({ "state": "done", "prompt": "p", "agentType": "antigravity", "interrupted": false }),
            json!({ "state": "working", "prompt": "p", "agentType": "pi", "toolName": "x".repeat(9000) }),
            json!({ "state": "working", "prompt": "x".repeat(9000), "agentType": "omp" }),
            json!({ "state": "working", "prompt": "p", "agentType": "command-code", "lastAssistantMessage": "tail with \u{001b}[0m escape codes" }),
        ];
        for case in cases {
            assert_eq!(
                normalize_agent_status_payload(&case),
                parse_agent_status_payload(&case.to_string()),
                "literal and JSON round trip disagree for {case}"
            );
        }
    }

    #[test]
    fn a_lone_surrogate_escape_cannot_cross_the_utf8_boundary() {
        // The twin's round-trip table has one row this core cannot answer:
        // `\ud800` is not a Unicode scalar value, so neither a Rust `String`
        // nor a `serde_json::Value` can hold it, and serde rejects the whole
        // document before any port code runs. Pinned as a test so the gap is
        // visible in the module that has it — the mitigation is the ledger's
        // shim-boundary contract, where `encodeDispatchPayload` refuses such a
        // payload at the TS edge instead of shipping it into Rust.
        // Observed: "unexpected end of hex escape at line 1 column 68".
        let json = r#"{"state":"working","prompt":"p","lastAssistantMessage":"lone \ud800 pair"}"#;
        assert!(serde_json::from_str::<Value>(json).is_err());
        assert_eq!(parse_agent_status_payload(json), None);
        // A PAIRED surrogate escape is ordinary text and must still parse.
        let paired = r#"{"state":"working","lastAssistantMessage":"pair 😀 ok"}"#;
        assert_eq!(
            parse(paired).last_assistant_message.as_deref(),
            Some("pair 😀 ok")
        );
    }

    #[test]
    fn prints_started_at_the_way_json_stringify_would() {
        assert_eq!(started_at_to_json(100.0), json!(100));
        assert_eq!(started_at_to_json(0.0), json!(0));
        assert_eq!(started_at_to_json(1.5), json!(1.5));
    }

    // ─── agentSubagentsEqual over raw payload values ────────────────────────

    #[test]
    fn compares_raw_subagent_lists_structurally() {
        // The twin's own `agentSubagentsEqual` case table, assertion for
        // assertion, over the values as they arrive rather than snapshots.
        let snapshot = json!({ "id": "a1", "state": "working", "startedAt": 1 });
        let one = json!([snapshot]);
        let copy = json!([{ "id": "a1", "state": "working", "startedAt": 1 }]);
        let idle = json!([{ "id": "a1", "state": "idle", "startedAt": 1 }]);
        let with_model =
            json!([{ "id": "a1", "state": "working", "startedAt": 1, "model": "gpt-5.4-mini" }]);
        let two = json!([snapshot, { "id": "b", "state": "working", "startedAt": 1 }]);

        assert_eq!(agent_subagents_equal_values(None, None), Some(true));
        assert_eq!(
            agent_subagents_equal_values(Some(&one), Some(&copy)),
            Some(true)
        );
        assert_eq!(
            agent_subagents_equal_values(Some(&one), Some(&idle)),
            Some(false)
        );
        assert_eq!(
            agent_subagents_equal_values(Some(&one), Some(&with_model)),
            Some(false)
        );
        assert_eq!(agent_subagents_equal_values(Some(&one), None), Some(false));
        assert_eq!(agent_subagents_equal_values(None, Some(&one)), Some(false));
        assert_eq!(
            agent_subagents_equal_values(Some(&one), Some(&two)),
            Some(false)
        );
    }

    #[test]
    fn treats_a_missing_started_at_as_distinct_from_an_explicit_zero() {
        // Why: this is the whole reason the raw comparison exists. The
        // normalizer coerces a missing `startedAt` to 0, so comparing NORMALIZED
        // snapshots would answer `true` here and suppress a fanout the twin
        // performs.
        let missing = json!([{ "id": "a1", "state": "working" }]);
        let zero = json!([{ "id": "a1", "state": "working", "startedAt": 0 }]);
        assert_eq!(
            agent_subagents_equal_values(Some(&missing), Some(&zero)),
            Some(false)
        );
        assert_eq!(
            agent_subagents_equal_values(Some(&missing), Some(&missing.clone())),
            Some(true)
        );
        // `undefined !== null` too, and both differ from an absent key.
        let null_type = json!([{ "id": "a1", "state": "working", "agentType": null }]);
        assert_eq!(
            agent_subagents_equal_values(Some(&missing), Some(&null_type)),
            Some(false)
        );
    }

    #[test]
    fn compares_raw_subagent_fields_with_javascript_strict_equality() {
        let number = json!([{ "id": "a1", "startedAt": 1 }]);
        let float = json!([{ "id": "a1", "startedAt": 1.0 }]);
        let text = json!([{ "id": "a1", "startedAt": "1" }]);
        assert_eq!(
            agent_subagents_equal_values(Some(&number), Some(&float)),
            Some(true)
        );
        assert_eq!(
            agent_subagents_equal_values(Some(&number), Some(&text)),
            Some(false)
        );
        // Object/array field values are compared by reference in the twin, and
        // two decoded JSON values are never the same reference.
        let object_field = json!([{ "id": {} }]);
        assert_eq!(
            agent_subagents_equal_values(Some(&object_field), Some(&object_field.clone())),
            Some(false)
        );
        // Unlisted keys are not part of the comparison.
        let extra = json!([{ "id": "a1", "startedAt": 1, "unlisted": 7 }]);
        assert_eq!(
            agent_subagents_equal_values(Some(&number), Some(&extra)),
            Some(true)
        );
        // An empty array is truthy, so it is NOT equal to an absent list.
        let empty = json!([]);
        assert_eq!(agent_subagents_equal_values(Some(&empty), None), Some(false));
        assert_eq!(
            agent_subagents_equal_values(Some(&empty), Some(&Value::Null)),
            Some(false)
        );
        assert_eq!(
            agent_subagents_equal_values(Some(&Value::Null), None),
            Some(true)
        );
    }

    #[test]
    fn refuses_only_the_subagent_shapes_the_twin_duck_types_or_throws_on() {
        // Truthy non-arrays: the twin reads `.length` and `[i]` off them.
        for (a, b) in [
            (json!("abc"), json!("abd")),
            (json!(5), json!(5)),
            (json!(true), json!(true)),
            (json!({}), json!({})),
            (json!({ "length": 1 }), json!({ "length": 1 })),
        ] {
            assert_eq!(agent_subagents_equal_values(Some(&a), Some(&b)), None);
        }
        // A `null` element the twin REACHES (it throws on `null.id`).
        let nulls = json!([Value::Null]);
        assert_eq!(
            agent_subagents_equal_values(Some(&nulls), Some(&nulls.clone())),
            None
        );
        // …but not one it never reaches: the length check short-circuits first,
        // and an earlier unequal pair decides the answer before the null.
        let empty = json!([]);
        assert_eq!(
            agent_subagents_equal_values(Some(&nulls), Some(&empty)),
            Some(false)
        );
        let left = json!([{ "id": "a" }, Value::Null]);
        let right = json!([{ "id": "b" }, Value::Null]);
        assert_eq!(
            agent_subagents_equal_values(Some(&left), Some(&right)),
            Some(false)
        );
    }

    // ─── hasUnsettledOrUnknownDispatch ──────────────────────────────────────

    #[test]
    fn reports_unsettled_dispatch_for_every_in_flight_lifecycle_state() {
        // The case table of src/renderer/src/lib/agent-hibernation-in-flight-work.test.ts,
        // which is where the twin's behaviour is pinned (the planner is the only
        // caller). `waiting_gate` is the fork-only state upstream's literal list
        // would have slept.
        for status in ["pending", "dispatched", "waiting_gate"] {
            assert!(has_unsettled_or_unknown_dispatch(&json!({
                "orchestration": { "taskId": "task-1", "dispatchId": "dispatch-1", "dispatchStatus": status }
            })));
        }
        for status in SETTLED_DISPATCH_STATUSES {
            assert!(!has_unsettled_or_unknown_dispatch(&json!({
                "orchestration": { "taskId": "task-1", "dispatchId": "dispatch-1", "dispatchStatus": status }
            })));
        }
    }

    #[test]
    fn treats_an_absent_dispatch_status_as_unsettled_and_no_orchestration_as_settled() {
        // A hook-only context carries no runtime status; calling "unknown"
        // settled is exactly the mistake that loses work.
        assert!(has_unsettled_or_unknown_dispatch(&json!({
            "orchestration": { "taskId": "task-1", "dispatchId": "dispatch-1" }
        })));
        // No orchestration at all is not a dispatch, so there is nothing unsettled.
        assert!(!has_unsettled_or_unknown_dispatch(&json!({})));
        assert!(!has_unsettled_or_unknown_dispatch(&json!({ "state": "done" })));
        for falsy in [json!(null), json!(0), json!(""), json!(false)] {
            assert!(!has_unsettled_or_unknown_dispatch(
                &json!({ "orchestration": falsy })
            ));
        }
    }

    #[test]
    fn matches_the_settled_dispatch_ids_exactly() {
        for near_miss in [
            json!("Completed"),
            json!("completed "),
            json!(" completed"),
            json!("circuit-broken"),
            json!(""),
            json!(0),
            json!(true),
            json!(["completed"]),
        ] {
            assert!(has_unsettled_or_unknown_dispatch(&json!({
                "orchestration": { "dispatchStatus": near_miss }
            })));
        }
    }

    // ─── isFreshNonDoneAgentStatus ──────────────────────────────────────────

    #[test]
    fn reports_freshness_only_for_non_done_rows_inside_the_window() {
        let now = json!(1_700_000_000_000_i64);
        let fresh = json!({ "state": "working", "updatedAt": 1_700_000_000_000_i64 });
        assert!(is_fresh_non_done_agent_status(Some(&fresh), &now, None));
        for state in ["working", "blocked", "waiting"] {
            let entry = json!({ "state": state, "updatedAt": 1_700_000_000_000_i64 });
            assert!(is_fresh_non_done_agent_status(Some(&entry), &now, None));
        }
        let done = json!({ "state": "done", "updatedAt": 1_700_000_000_000_i64 });
        assert!(!is_fresh_non_done_agent_status(Some(&done), &now, None));
        // A non-`done` state id is anything that is not the literal "done".
        for state in [json!("Done"), json!("done "), json!(42), json!(null)] {
            let entry = json!({ "state": state, "updatedAt": 1_700_000_000_000_i64 });
            assert!(is_fresh_non_done_agent_status(Some(&entry), &now, None));
        }
    }

    #[test]
    fn treats_the_stale_bound_as_inclusive_and_defaults_it_to_the_constant() {
        let now_ms = 1_700_000_000_000_i64;
        let now = json!(now_ms);
        let stale = AGENT_STATUS_STALE_AFTER_MS as i64;
        let at_bound = json!({ "state": "working", "updatedAt": now_ms - stale });
        let past_bound = json!({ "state": "working", "updatedAt": now_ms - stale - 1 });
        assert!(is_fresh_non_done_agent_status(Some(&at_bound), &now, None));
        assert!(!is_fresh_non_done_agent_status(Some(&past_bound), &now, None));
        // The default only fires for an ABSENT argument; an explicit `null`
        // coerces to 0 the way a JS default parameter does not fire for `null`.
        assert!(!is_fresh_non_done_agent_status(
            Some(&at_bound),
            &now,
            Some(&Value::Null)
        ));
        let same_instant = json!({ "state": "working", "updatedAt": now_ms });
        assert!(is_fresh_non_done_agent_status(
            Some(&same_instant),
            &now,
            Some(&Value::Null)
        ));
        assert_eq!(AGENT_STATUS_STALE_AFTER_MS, 1_800_000.0);
    }

    #[test]
    fn reports_stale_when_the_entry_is_absent_or_carries_no_timestamp() {
        let now = json!(1_000);
        assert!(!is_fresh_non_done_agent_status(None, &now, None));
        assert!(!is_fresh_non_done_agent_status(
            Some(&Value::Null),
            &now,
            None
        ));
        // A missing `updatedAt` is NaN, not 0 — coercing it to 0 would report a
        // row that never reported a timestamp as fresh.
        let no_timestamp = json!({ "state": "working" });
        assert!(!is_fresh_non_done_agent_status(
            Some(&no_timestamp),
            &now,
            None
        ));
        for falsy in [json!(0), json!(""), json!(false)] {
            assert!(!is_fresh_non_done_agent_status(Some(&falsy), &now, None));
        }
    }

    #[test]
    fn coerces_freshness_operands_the_way_the_minus_operator_does() {
        let now = json!(1_000);
        let with = |updated_at: Value| json!({ "state": "working", "updatedAt": updated_at });
        // `null` is 0, booleans are 0/1, numeric strings parse, junk is NaN.
        assert!(is_fresh_non_done_agent_status(
            Some(&with(json!(null))),
            &now,
            None
        ));
        assert!(is_fresh_non_done_agent_status(
            Some(&with(json!("  900  "))),
            &now,
            None
        ));
        assert!(is_fresh_non_done_agent_status(
            Some(&with(json!("0x10"))),
            &now,
            None
        ));
        assert!(!is_fresh_non_done_agent_status(
            Some(&with(json!("abc"))),
            &now,
            None
        ));
        // Rust's own float parser accepts these spellings; JS does not.
        for junk in ["infinity", "nan", "1_000"] {
            assert!(!is_fresh_non_done_agent_status(
                Some(&with(json!(junk))),
                &now,
                None
            ));
        }
        // `1000 - Infinity` is -Infinity (inside any window); `1000 - -Infinity`
        // is +Infinity (outside every window).
        assert!(is_fresh_non_done_agent_status(
            Some(&with(json!("Infinity"))),
            &now,
            None
        ));
        assert!(!is_fresh_non_done_agent_status(
            Some(&with(json!("-Infinity"))),
            &now,
            None
        ));
        // `ToPrimitive` on an array joins it; `[900]` is 900, `[1,2]` is NaN.
        assert!(is_fresh_non_done_agent_status(
            Some(&with(json!([900]))),
            &now,
            None
        ));
        assert!(is_fresh_non_done_agent_status(
            Some(&with(json!([]))),
            &now,
            None
        ));
        assert!(!is_fresh_non_done_agent_status(
            Some(&with(json!([1, 2]))),
            &now,
            None
        ));
        assert!(!is_fresh_non_done_agent_status(
            Some(&with(json!({}))),
            &now,
            None
        ));
    }

    #[test]
    fn normalize_skips_the_pre_parse_structure_guard_the_string_entry_point_runs() {
        // Why: `normalizeAgentStatusPayload` takes an already-deserialized
        // object, so the token/depth guard that rejects the JSON TEXT never runs
        // — the two entry points are not interchangeable on this input class.
        let mut deep = json!({ "leaf": true });
        for _ in 0..AGENT_STATUS_JSON_NESTING_DEPTH_LIMIT + 8 {
            deep = json!({ "deep": deep });
        }
        let payload = json!({ "state": "working", "prompt": "p", "extra": deep });
        assert_eq!(
            normalize_agent_status_payload(&payload).map(|parsed| parsed.prompt),
            Some("p".to_string())
        );
        assert_eq!(parse_agent_status_payload(&payload.to_string()), None);
    }
}
