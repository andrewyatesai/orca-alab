//! Parity dispatch for `orca_core::browser_viewport_presets` vs
//! `src/shared/browser-viewport-presets.ts`.
//!
//! `getBrowserViewportPreset` takes the bare id (a string or null) and returns
//! the preset row or null; `browserViewportPresetToOverride` maps a preset row
//! onto a CDP override. JSON keys are emitted in the TS object's declaration
//! order to match `JSON.stringify`.

use orca_core::browser_viewport_presets::{
    browser_viewport_preset_to_override, get_browser_viewport_preset, BrowserViewportOverride,
    BrowserViewportPreset, BrowserViewportPresetId,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "getBrowserViewportPreset" => {
            // A null/unknown id resolves to null, matching the TS `?? null`.
            let id = input.as_str().and_then(BrowserViewportPresetId::from_id);
            match get_browser_viewport_preset(id) {
                Some(preset) => preset_to_json(&preset),
                None => Value::Null,
            }
        }
        "browserViewportPresetToOverride" => match preset_from_json(input) {
            Some(preset) => override_to_json(&browser_viewport_preset_to_override(preset)),
            None => parity_error("invalid preset input"),
        },
        other => parity_error(&format!("unknown function {other}")),
    }
}

fn preset_to_json(preset: &BrowserViewportPreset) -> Value {
    json!({
        "id": preset.id.as_str(),
        "label": preset.label,
        "width": preset.width,
        "height": preset.height,
        "deviceScaleFactor": preset.device_scale_factor,
        "mobile": preset.mobile,
    })
}

fn override_to_json(value: &BrowserViewportOverride) -> Value {
    json!({
        "width": value.width,
        "height": value.height,
        "deviceScaleFactor": value.device_scale_factor,
        "mobile": value.mobile,
    })
}

fn preset_from_json(input: &Value) -> Option<BrowserViewportPreset> {
    let id = input
        .get("id")
        .and_then(Value::as_str)
        .and_then(BrowserViewportPresetId::from_id)?;
    Some(BrowserViewportPreset {
        id,
        // `label` is unused by the override mapping; a static placeholder keeps
        // the `&'static str` field without re-deriving the canonical label.
        label: "",
        width: input.get("width")?.as_u64()? as u32,
        height: input.get("height")?.as_u64()? as u32,
        device_scale_factor: input.get("deviceScaleFactor")?.as_u64()? as u32,
        mobile: input.get("mobile")?.as_bool()?,
    })
}

fn parity_error(message: &str) -> Value {
    json!({ "__parity_error__": message })
}
