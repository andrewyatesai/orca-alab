//! Semantic JSON equality used for the in-harness golden self-check.

use serde_json::Value;

/// Compare two JSON values by meaning, not representation: numbers by `f64`
/// value (so `1` == `1.0`, matching JS `JSON.stringify`), object keys
/// order-insensitive, arrays order-sensitive. The TS driver applies the same
/// rule, so a divergence reported by one side is a divergence for both.
pub fn json_semantic_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(p), Some(q)) => p == q,
            _ => x == y,
        },
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_semantic_eq(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| json_semantic_eq(v, w)))
        }
        _ => false,
    }
}
