//! Agent status payload parse/normalize, ported from the parser half of
//! `src/shared/agent-status-types.ts`.
//!
//! Validates an untrusted agent status payload (hook/OSC-9999) into the lean
//! `ParsedAgentStatusPayload`: state allow-list, per-field trim/collapse/cap,
//! and UTF-16-safe truncation that never leaves a lone surrogate. Over vendored
//! `serde_json`.

use serde_json::Value;

pub const AGENT_STATUS_STATES: [&str; 4] = ["working", "blocked", "waiting", "done"];
pub const AGENT_STATUS_MAX_FIELD_LENGTH: usize = 200;
pub const AGENT_STATUS_TOOL_NAME_MAX_LENGTH: usize = 60;
pub const AGENT_STATUS_TOOL_INPUT_MAX_LENGTH: usize = 160;
pub const AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH: usize = 8000;
pub const AGENT_TYPE_MAX_LENGTH: usize = 40;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedAgentStatusPayload {
    pub state: AgentStatusState,
    pub prompt: String,
    pub agent_type: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub last_assistant_message: Option<String>,
    pub interrupted: Option<bool>,
}

/// Truncate to `max_length` UTF-16 code units, dropping a trailing lone high
/// surrogate so the result is always valid UTF-16 (no replacement glyph).
fn truncate_preserving_surrogates(value: &str, max_length: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    if units.len() <= max_length {
        return value.to_string();
    }
    let mut end = max_length;
    if units.get(end - 1).is_some_and(|unit| (0xd800..=0xdbff).contains(unit)) {
        end -= 1;
    }
    String::from_utf16_lossy(&units[..end])
}

/// Collapse `\r`/`\n`/U+2028/U+2029 runs to a single space (single-line invariant).
fn collapse_to_single_line(value: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for ch in value.chars() {
        if matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            if !in_run {
                out.push(' ');
                in_run = true;
            }
        } else {
            out.push(ch);
            in_run = false;
        }
    }
    out
}

/// Fold line/paragraph separators to `\n` and cap blank-line runs (3+ → 2),
/// preserving paragraph structure for `whitespace-pre-wrap` rendering.
fn collapse_blank_lines(value: &str) -> String {
    let folded = value.replace("\r\n", "\n").replace('\r', "\n").replace(['\u{2028}', '\u{2029}'], "\n");
    let mut out = String::new();
    let mut newline_run = 0;
    let flush = |out: &mut String, run: usize| {
        if run >= 3 {
            out.push_str("\n\n");
        } else {
            out.push_str(&"\n".repeat(run));
        }
    };
    for ch in folded.chars() {
        if ch == '\n' {
            newline_run += 1;
        } else {
            flush(&mut out, newline_run);
            newline_run = 0;
            out.push(ch);
        }
    }
    flush(&mut out, newline_run);
    out
}

fn normalize_field(value: Option<&Value>, max_length: usize) -> String {
    match value.and_then(Value::as_str) {
        Some(text) => truncate_preserving_surrogates(&collapse_to_single_line(text.trim()), max_length),
        None => String::new(),
    }
}

fn normalize_optional_field(value: Option<&Value>, max_length: usize) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    let normalized = truncate_preserving_surrogates(&collapse_to_single_line(text.trim()), max_length);
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_optional_multiline_field(value: Option<&Value>, max_length: usize) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    let normalized = truncate_preserving_surrogates(&collapse_blank_lines(text.trim()), max_length);
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_agent_status_object(parsed: &Value) -> Option<ParsedAgentStatusPayload> {
    let object = parsed.as_object()?;
    let state = AgentStatusState::from_id(object.get("state").and_then(Value::as_str)?)?;
    Some(ParsedAgentStatusPayload {
        state,
        prompt: normalize_field(object.get("prompt"), AGENT_STATUS_MAX_FIELD_LENGTH),
        agent_type: normalize_optional_field(object.get("agentType"), AGENT_TYPE_MAX_LENGTH),
        tool_name: normalize_optional_field(object.get("toolName"), AGENT_STATUS_TOOL_NAME_MAX_LENGTH),
        tool_input: normalize_optional_field(object.get("toolInput"), AGENT_STATUS_TOOL_INPUT_MAX_LENGTH),
        last_assistant_message: normalize_optional_multiline_field(
            object.get("lastAssistantMessage"),
            AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH,
        ),
        // Only meaningful on `done`; require a strict boolean `true`.
        interrupted: (object.get("interrupted") == Some(&Value::Bool(true)) && state == AgentStatusState::Done)
            .then_some(true),
    })
}

pub fn normalize_agent_status_payload(payload: &Value) -> Option<ParsedAgentStatusPayload> {
    normalize_agent_status_object(payload)
}

pub fn parse_agent_status_payload(json: &str) -> Option<ParsedAgentStatusPayload> {
    let parsed: Value = serde_json::from_str(json).ok()?;
    normalize_agent_status_object(&parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentStatusState::Working;

    fn utf16_len(value: &str) -> usize {
        value.encode_utf16().count()
    }

    #[test]
    fn parses_a_valid_working_payload() {
        assert_eq!(
            parse_agent_status_payload(r#"{"state":"working","prompt":"Fix the flaky assertion","agentType":"codex"}"#),
            Some(ParsedAgentStatusPayload {
                state: Working,
                prompt: "Fix the flaky assertion".to_string(),
                agent_type: Some("codex".to_string()),
                tool_name: None,
                tool_input: None,
                last_assistant_message: None,
                interrupted: None,
            })
        );
    }

    #[test]
    fn parses_all_agent_status_states() {
        for state in AGENT_STATUS_STATES {
            let result = parse_agent_status_payload(&format!(r#"{{"state":"{state}"}}"#)).unwrap();
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
    fn returns_none_for_non_object_json() {
        assert_eq!(parse_agent_status_payload(r#""just a string""#), None);
        assert_eq!(parse_agent_status_payload("42"), None);
        assert_eq!(parse_agent_status_payload("null"), None);
        assert_eq!(parse_agent_status_payload("[]"), None);
    }

    #[test]
    fn normalizes_multiline_and_crlf_prompts_to_single_line() {
        assert_eq!(
            parse_agent_status_payload(r#"{"state":"working","prompt":"line one\nline two\nline three"}"#).unwrap().prompt,
            "line one line two line three"
        );
        assert_eq!(
            parse_agent_status_payload(r#"{"state":"working","prompt":"line one\r\nline two\r\nline three"}"#).unwrap().prompt,
            "line one line two line three"
        );
    }

    #[test]
    fn trims_and_truncates_and_defaults_the_prompt() {
        assert_eq!(parse_agent_status_payload(r#"{"state":"working","prompt":"  padded  "}"#).unwrap().prompt, "padded");
        let long = format!(r#"{{"state":"working","prompt":"{}"}}"#, "x".repeat(300));
        assert_eq!(utf16_len(&parse_agent_status_payload(&long).unwrap().prompt), AGENT_STATUS_MAX_FIELD_LENGTH);
        assert_eq!(parse_agent_status_payload(r#"{"state":"done"}"#).unwrap().prompt, "");
        assert_eq!(parse_agent_status_payload(r#"{"state":"working","prompt":42}"#).unwrap().prompt, "");
    }

    #[test]
    fn handles_agent_type_field() {
        assert_eq!(parse_agent_status_payload(r#"{"state":"working","agentType":"cursor"}"#).unwrap().agent_type.as_deref(), Some("cursor"));
        let long = serde_json::json!({ "state": "working", "agentType": "a".repeat(AGENT_TYPE_MAX_LENGTH + 20) }).to_string();
        assert_eq!(utf16_len(parse_agent_status_payload(&long).unwrap().agent_type.as_deref().unwrap()), AGENT_TYPE_MAX_LENGTH);
        assert_eq!(parse_agent_status_payload(r#"{"state":"working","agentType":"   "}"#).unwrap().agent_type, None);
        assert_eq!(parse_agent_status_payload(r#"{"state":"working","agentType":"claude\nrogue"}"#).unwrap().agent_type.as_deref(), Some("claude rogue"));
    }

    #[test]
    fn parses_and_caps_optional_fields() {
        let result = parse_agent_status_payload(r#"{"state":"working","toolName":"Edit","toolInput":"/path/to/file.ts","lastAssistantMessage":"Here is the edit I made."}"#).unwrap();
        assert_eq!(result.tool_name.as_deref(), Some("Edit"));
        assert_eq!(result.tool_input.as_deref(), Some("/path/to/file.ts"));
        assert_eq!(result.last_assistant_message.as_deref(), Some("Here is the edit I made."));

        let long = serde_json::json!({
            "state": "working",
            "toolName": "n".repeat(AGENT_STATUS_TOOL_NAME_MAX_LENGTH + 50),
            "toolInput": "i".repeat(AGENT_STATUS_TOOL_INPUT_MAX_LENGTH + 50),
            "lastAssistantMessage": "m".repeat(AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH + 500),
        })
        .to_string();
        let result = parse_agent_status_payload(&long).unwrap();
        assert_eq!(utf16_len(result.tool_name.as_deref().unwrap()), AGENT_STATUS_TOOL_NAME_MAX_LENGTH);
        assert_eq!(utf16_len(result.tool_input.as_deref().unwrap()), AGENT_STATUS_TOOL_INPUT_MAX_LENGTH);
        assert_eq!(utf16_len(result.last_assistant_message.as_deref().unwrap()), AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH);
    }

    #[test]
    fn treats_missing_empty_and_non_string_optional_fields_as_none() {
        let omitted = parse_agent_status_payload(r#"{"state":"working"}"#).unwrap();
        assert_eq!((omitted.tool_name, omitted.tool_input, omitted.last_assistant_message), (None, None, None));
        let non_string = parse_agent_status_payload(r#"{"state":"working","toolName":42,"toolInput":null,"lastAssistantMessage":[]}"#).unwrap();
        assert_eq!((non_string.tool_name, non_string.tool_input, non_string.last_assistant_message), (None, None, None));
        let empty = parse_agent_status_payload(r#"{"state":"working","toolName":"   ","toolInput":"","lastAssistantMessage":"   "}"#).unwrap();
        assert_eq!((empty.tool_name, empty.tool_input, empty.last_assistant_message), (None, None, None));
    }

    #[test]
    fn collapses_newlines_in_tool_input_single_line_field() {
        assert_eq!(
            parse_agent_status_payload(r#"{"state":"working","toolInput":"line one\nline two"}"#).unwrap().tool_input.as_deref(),
            Some("line one line two")
        );
    }

    #[test]
    fn preserves_and_caps_paragraph_breaks_in_last_assistant_message() {
        assert_eq!(
            parse_agent_status_payload(r#"{"state":"done","lastAssistantMessage":"Summary line.\n\nDetails paragraph."}"#).unwrap().last_assistant_message.as_deref(),
            Some("Summary line.\n\nDetails paragraph.")
        );
        assert_eq!(
            parse_agent_status_payload(r#"{"state":"done","lastAssistantMessage":"a\r\nb\n\n\n\nc"}"#).unwrap().last_assistant_message.as_deref(),
            Some("a\nb\n\nc")
        );
    }

    #[test]
    fn folds_unicode_separators_and_caps_blank_line_runs() {
        let line_sep = parse_agent_status_payload("{\"state\":\"done\",\"lastAssistantMessage\":\"a\u{2028}\u{2028}\u{2028}\u{2028}b\"}").unwrap();
        assert_eq!(line_sep.last_assistant_message.as_deref(), Some("a\n\nb"));
        let para_sep = parse_agent_status_payload("{\"state\":\"done\",\"lastAssistantMessage\":\"a\u{2029}\u{2029}\u{2029}\u{2029}b\"}").unwrap();
        assert_eq!(para_sep.last_assistant_message.as_deref(), Some("a\n\nb"));
        let mixed = parse_agent_status_payload("{\"state\":\"done\",\"lastAssistantMessage\":\"a\u{2028}\u{2029}\\n\u{2028}\u{2029}b\"}").unwrap();
        assert_eq!(mixed.last_assistant_message.as_deref(), Some("a\n\nb"));
    }

    #[test]
    fn respects_prompt_cap_independent_of_other_fields() {
        let json = serde_json::json!({ "state": "working", "prompt": "p".repeat(300), "toolInput": "xxxxx" }).to_string();
        let result = parse_agent_status_payload(&json).unwrap();
        assert_eq!(utf16_len(&result.prompt), AGENT_STATUS_MAX_FIELD_LENGTH);
        assert_eq!(result.tool_input.as_deref(), Some("xxxxx"));
    }

    #[test]
    fn handles_interrupted_with_strict_boolean_and_done_state() {
        assert_eq!(parse_agent_status_payload(r#"{"state":"done","interrupted":true}"#).unwrap().interrupted, Some(true));
        for state in ["working", "blocked", "waiting"] {
            assert_eq!(parse_agent_status_payload(&format!(r#"{{"state":"{state}","interrupted":true}}"#)).unwrap().interrupted, None);
        }
        assert_eq!(parse_agent_status_payload(r#"{"state":"done","interrupted":"true"}"#).unwrap().interrupted, None);
        assert_eq!(parse_agent_status_payload(r#"{"state":"done","interrupted":1}"#).unwrap().interrupted, None);
        assert_eq!(parse_agent_status_payload(r#"{"state":"done","interrupted":"yes"}"#).unwrap().interrupted, None);
    }

    #[test]
    fn never_leaves_a_lone_high_surrogate_when_truncating() {
        let prompt = format!("x{}", "😀".repeat(AGENT_STATUS_MAX_FIELD_LENGTH));
        let json = serde_json::json!({ "state": "working", "prompt": prompt }).to_string();
        let units: Vec<u16> = parse_agent_status_payload(&json).unwrap().prompt.encode_utf16().collect();
        assert!(units.len() <= AGENT_STATUS_MAX_FIELD_LENGTH);
        assert!(units.len() >= AGENT_STATUS_MAX_FIELD_LENGTH - 1);
        let last = *units.last().unwrap();
        assert!(!(0xd800..=0xdbff).contains(&last));
        if (0xdc00..=0xdfff).contains(&last) {
            assert!((0xd800..=0xdbff).contains(&units[units.len() - 2]));
        }
    }

    #[test]
    fn never_leaves_a_lone_high_surrogate_in_last_assistant_message() {
        let pairs = AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH / 2 + 1;
        let message = format!("x{}", "😀".repeat(pairs));
        let json = serde_json::json!({ "state": "done", "lastAssistantMessage": message }).to_string();
        let units: Vec<u16> = parse_agent_status_payload(&json).unwrap().last_assistant_message.unwrap().encode_utf16().collect();
        assert!(units.len() <= AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH);
        assert!(units.len() >= AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH - 1);
        let last = *units.last().unwrap();
        assert!(!(0xd800..=0xdbff).contains(&last));
        if (0xdc00..=0xdfff).contains(&last) {
            assert!((0xd800..=0xdbff).contains(&units[units.len() - 2]));
        }
    }
}
