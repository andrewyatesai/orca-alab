//! Parity dispatch for `orca_core::uri_component`. There is no dedicated
//! `src/shared/*` source file: these mirror the JS globals
//! `encodeURIComponent` / `decodeURIComponent` (decode passes through the
//! original on a malformed `%`-escape, like the TS try/catch).

use orca_core::uri_component::{decode_uri_component, encode_uri_component};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Both functions take a single string arg; the vector `input` is that string.
    let s = input.as_str().unwrap_or_default();
    match function {
        "encodeURIComponent" => Value::String(encode_uri_component(s)),
        "decodeURIComponent" => Value::String(decode_uri_component(s)),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
