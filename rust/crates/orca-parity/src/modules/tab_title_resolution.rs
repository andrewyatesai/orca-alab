//! Parity dispatch for `orca_core::tab_title_resolution` vs
//! `src/shared/tab-title-resolution.ts`.

use orca_core::tab_title_resolution::{
    resolve_terminal_tab_title, resolve_unified_tab_label, TerminalTabTitleParts,
    UnifiedTabLabelParts,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "resolveTerminalTabTitle" => {
            let tab = input.get("tab");
            let parts = TerminalTabTitleParts {
                custom_title: str_field(tab, "customTitle"),
                generated_title: str_field(tab, "generatedTitle"),
                title: str_field(tab, "title"),
            };
            Value::String(resolve_terminal_tab_title(
                &parts,
                generated_enabled(input),
                fallback(input),
            ))
        }
        "resolveUnifiedTabLabel" => {
            let tab = input.get("tab");
            // TS accepts `tab | undefined`; a null/absent tab (optional chaining
            // short-circuits) maps to `None`.
            let parts = tab.filter(|t| !t.is_null()).map(|t| UnifiedTabLabelParts {
                custom_label: str_field(Some(t), "customLabel"),
                generated_label: str_field(Some(t), "generatedLabel"),
                label: str_field(Some(t), "label"),
            });
            Value::String(resolve_unified_tab_label(
                parts.as_ref(),
                generated_enabled(input),
                fallback(input),
            ))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Trimmed-string fields are read raw; non-strings/null map to `None`, matching
/// TS where `null?.trim()` is falsy.
fn str_field<'a>(obj: Option<&'a Value>, key: &str) -> Option<&'a str> {
    obj.and_then(|o| o.get(key)).and_then(Value::as_str)
}

fn generated_enabled(input: &Value) -> bool {
    input
        .get("generatedTitlesEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn fallback(input: &Value) -> &str {
    input.get("fallback").and_then(Value::as_str).unwrap_or("")
}
