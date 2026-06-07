//! Parity dispatch for `orca_core::stable_pane_id` vs
//! `src/shared/stable-pane-id.ts`.

use orca_core::stable_pane_id::{
    is_stable_pane_id, is_terminal_leaf_id, parse_legacy_numeric_pane_key, parse_pane_key,
    LegacyNumericPaneKey, ParsedPaneKey,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isStablePaneId" => match input.as_str() {
            Some(s) => Value::Bool(is_stable_pane_id(s)),
            None => json!({ "__parity_error__": "isStablePaneId expects a string" }),
        },
        "isTerminalLeafId" => match input.as_str() {
            Some(s) => Value::Bool(is_terminal_leaf_id(s)),
            None => json!({ "__parity_error__": "isTerminalLeafId expects a string" }),
        },
        "parsePaneKey" => match input.as_str() {
            Some(s) => parsed_pane_key_to_json(parse_pane_key(s)),
            None => json!({ "__parity_error__": "parsePaneKey expects a string" }),
        },
        // TS accepts `unknown` and returns null for non-strings; a non-string
        // Value yields None here, matching that null return.
        "parseLegacyNumericPaneKey" => {
            legacy_to_json(input.as_str().and_then(parse_legacy_numeric_pane_key))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `{ tabId, leafId, stablePaneId } | null`.
fn parsed_pane_key_to_json(parsed: Option<ParsedPaneKey>) -> Value {
    match parsed {
        // TS exposes `stablePaneId` as an alias of the leaf id (same branded string).
        Some(p) => json!({
            "tabId": p.tab_id,
            "leafId": p.leaf_id.clone(),
            "stablePaneId": p.leaf_id,
        }),
        None => Value::Null,
    }
}

/// Match `JSON.stringify` of the TS `{ tabId, numericPaneId, paneKey } | null`.
fn legacy_to_json(parsed: Option<LegacyNumericPaneKey>) -> Value {
    match parsed {
        Some(p) => json!({
            "tabId": p.tab_id,
            "numericPaneId": p.numeric_pane_id,
            "paneKey": p.pane_key,
        }),
        None => Value::Null,
    }
}
