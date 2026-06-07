//! Parity dispatch for `orca_config::repo_icon` vs `src/shared/repo-icon.ts`.

use orca_config::{
    favicon_url_from_website, github_avatar_icon, sanitize_repo_icon, RepoIcon,
    RepoIconImageSource, RepoIconSanitizeResult,
};
use serde_json::{json, Map, Value};

// `sanitizeRepoIcon` is tri-state: TS `undefined` (leave as-is) isn't
// JSON-representable, so both adapters encode it as this sentinel string.
const SANITIZE_UNDEFINED: &str = "__undefined__";

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // The harness can't express a JS `undefined` input, so a null input maps
        // to `Some(Value::Null)` = the explicit reset, matching the TS null case.
        "sanitizeRepoIcon" => match sanitize_repo_icon(Some(input)) {
            RepoIconSanitizeResult::Undefined => Value::String(SANITIZE_UNDEFINED.to_string()),
            RepoIconSanitizeResult::Reset => Value::Null,
            RepoIconSanitizeResult::Icon(icon) => icon_to_json(&icon),
        },
        "faviconUrlFromWebsite" => match favicon_url_from_website(input.as_str().unwrap_or("")) {
            Some(url) => Value::String(url),
            None => Value::Null,
        },
        "githubAvatarIcon" => {
            let owner = input.get("owner").and_then(Value::as_str).unwrap_or("");
            let repo = input.get("repo").and_then(Value::as_str).unwrap_or("");
            icon_to_json(&github_avatar_icon(owner, repo))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `RepoIcon` union (absent `label` omitted).
fn icon_to_json(icon: &RepoIcon) -> Value {
    match icon {
        RepoIcon::Lucide { name } => json!({ "type": "lucide", "name": name }),
        RepoIcon::Emoji { emoji } => json!({ "type": "emoji", "emoji": emoji }),
        RepoIcon::Image { src, source, label } => {
            let mut map = Map::new();
            map.insert("type".to_string(), Value::String("image".to_string()));
            map.insert("src".to_string(), Value::String(src.clone()));
            map.insert("source".to_string(), Value::String(image_source_id(*source).to_string()));
            // Omit `label` when absent, mirroring the TS `...(label ? { label } : {})`.
            if let Some(label) = label {
                map.insert("label".to_string(), Value::String(label.clone()));
            }
            Value::Object(map)
        }
    }
}

/// Serialize the enum to its TS string id (`RepoIconImageSource`).
fn image_source_id(source: RepoIconImageSource) -> &'static str {
    match source {
        RepoIconImageSource::Upload => "upload",
        RepoIconImageSource::File => "file",
        RepoIconImageSource::Favicon => "favicon",
        RepoIconImageSource::Github => "github",
    }
}
