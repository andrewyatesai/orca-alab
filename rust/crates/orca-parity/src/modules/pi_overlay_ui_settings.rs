//! Parity dispatch for `orca_config::pi_overlay_ui_settings` vs
//! `src/shared/pi-overlay-ui-settings.ts`.

use orca_config::merge_pi_overlay_ui_settings;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Returns the merged settings object directly; the port already shapes
        // it as a `serde_json::Value` matching `JSON.stringify` of the TS return.
        "mergePiOverlayUiSettings" => merge_pi_overlay_ui_settings(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
