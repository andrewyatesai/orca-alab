//! Parity dispatch for `orca_config::feature_education_telemetry` vs
//! `src/shared/feature-education-telemetry.ts`.

use orca_config::feature_education_telemetry::{
    normalize_feature_education_source, normalize_setup_guide_source,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single arg: the value to normalize is the input itself. TS accepts
        // `string | null | undefined`; JSON null / non-string → None.
        "normalizeFeatureEducationSource" => {
            Value::String(normalize_feature_education_source(input.as_str()).to_string())
        }
        "normalizeSetupGuideSource" => {
            Value::String(normalize_setup_guide_source(input.as_str()).to_string())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
