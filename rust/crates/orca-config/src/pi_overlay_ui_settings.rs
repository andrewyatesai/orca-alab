//! Pi overlay UI settings merge, ported from `src/shared/pi-overlay-ui-settings.ts`.
//!
//! Merges user Pi settings while force-overriding the Orca-only safety settings
//! (`terminal.clearOnShrink` and `hideThinkingBlock`), tolerating malformed
//! input shapes. Operates on arbitrary JSON over vendored `serde_json`.

use serde_json::{Map, Value};

const PI_OVERLAY_HIDE_THINKING_BLOCK: bool = true;
const PI_OVERLAY_CLEAR_ON_SHRINK: bool = true;

pub fn merge_pi_overlay_ui_settings(settings: &Value) -> Value {
    let mut merged = match settings {
        Value::Object(object) => object.clone(),
        _ => Map::new(),
    };
    let mut terminal = match merged.get("terminal") {
        Some(Value::Object(object)) => object.clone(),
        _ => Map::new(),
    };

    terminal.insert("clearOnShrink".to_string(), Value::Bool(PI_OVERLAY_CLEAR_ON_SHRINK));
    merged.insert("terminal".to_string(), Value::Object(terminal));
    merged.insert("hideThinkingBlock".to_string(), Value::Bool(PI_OVERLAY_HIDE_THINKING_BLOCK));

    Value::Object(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_user_settings_while_forcing_orca_only_pi_ui_safety_settings() {
        let merged = merge_pi_overlay_ui_settings(&json!({
            "defaultProvider": "amazon-bedrock",
            "hideThinkingBlock": false,
            "packages": ["npm:pi-web-access"],
            "terminal": { "showImages": false, "clearOnShrink": false }
        }));
        assert_eq!(
            merged,
            json!({
                "defaultProvider": "amazon-bedrock",
                "hideThinkingBlock": true,
                "packages": ["npm:pi-web-access"],
                "terminal": { "showImages": false, "clearOnShrink": true }
            })
        );
    }

    #[test]
    fn creates_a_valid_settings_object_from_malformed_shapes() {
        let expected = json!({ "hideThinkingBlock": true, "terminal": { "clearOnShrink": true } });
        assert_eq!(merge_pi_overlay_ui_settings(&Value::Null), expected);
        assert_eq!(merge_pi_overlay_ui_settings(&json!({ "terminal": "compact" })), expected);
    }
}
