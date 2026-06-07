//! Parity dispatch for `orca_core::browser_search` vs the self-contained pure
//! search-query parts of `src/shared/browser-url.ts`. The Kagi session-link /
//! navigation normaliser is deferred in the Rust port, so only `buildSearchUrl`
//! (without options) and `looksLikeSearchQuery` are covered here.

use orca_core::browser_search::{
    build_search_url, looks_like_search_query, SearchEngine, DEFAULT_SEARCH_ENGINE,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildSearchUrl" => {
            let query = input.get("query").and_then(Value::as_str).unwrap_or("");
            let engine = parse_engine(input.get("engine"));
            Value::String(build_search_url(query, engine))
        }
        "looksLikeSearchQuery" => Value::Bool(looks_like_search_query(input.as_str().unwrap_or(""))),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Map the TS `SearchEngine` string ids to the Rust enum. Absent/undefined falls
/// back to the default engine, mirroring TS's `engine = DEFAULT_SEARCH_ENGINE`.
fn parse_engine(value: Option<&Value>) -> SearchEngine {
    match value.and_then(Value::as_str) {
        Some("google") => SearchEngine::Google,
        Some("duckduckgo") => SearchEngine::DuckDuckGo,
        Some("bing") => SearchEngine::Bing,
        Some("kagi") => SearchEngine::Kagi,
        _ => DEFAULT_SEARCH_ENGINE,
    }
}
