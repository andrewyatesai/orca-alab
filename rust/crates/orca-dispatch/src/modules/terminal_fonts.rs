//! Parity dispatch for `orca_core::terminal_fonts` vs
//! `src/shared/terminal-fonts.ts`.

use orca_core::terminal_fonts::{normalize_terminal_font_weight, resolve_terminal_font_weights};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Single numeric arg; JSON null (TS null/undefined) maps to None -> default.
    let font_weight = input.as_f64();
    match function {
        "normalizeTerminalFontWeight" => json!(normalize_terminal_font_weight(font_weight)),
        "resolveTerminalFontWeights" => {
            let weights = resolve_terminal_font_weights(font_weight);
            json!({
                "fontWeight": weights.font_weight,
                "fontWeightBold": weights.font_weight_bold,
            })
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
