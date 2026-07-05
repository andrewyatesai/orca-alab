//! The daemon socket protocol, mirroring `src/main/daemon/types.ts`: the `hello`
//! handshake, id-correlated `RpcResponse`, and the `data`/`exit` stream events.
//! Requests are read as `serde_json::Value` (no derive), matching orca-relay's
//! payload handling; responses/events are built with `json!`. The Rust daemon must
//! be indistinguishable from the Node one at this wire, so these shapes track
//! types.ts exactly.

use serde_json::{json, Value};

/// Must equal `PROTOCOL_VERSION` in `src/main/daemon/types.ts`. A client hello at
/// a different version is rejected with a `hello` error.
///
/// Why 1018 (not 19): the fork reserves the 1000+ namespace so its daemon
/// endpoints (`daemon-v1018.*`, keyed off this number) never collide with a
/// public Orca install — a public build (v18, or any future public bump) must
/// never handshake with this daemon, and vice versa (see types.ts).
pub const PROTOCOL_VERSION: u64 = 1018;

/// The first line on every socket: `{ type:'hello', version, token, clientId, role }`.
pub struct Hello {
    pub version: u64,
    /// Validated against the daemon's published token when one is configured
    /// (see `connection::handle_connection`).
    pub token: String,
    pub client_id: String,
    /// `"control"` (RPC) or `"stream"` (events). Each client opens one of each.
    pub role: String,
}

pub fn parse_hello(v: &Value) -> Option<Hello> {
    if v.get("type")?.as_str()? != "hello" {
        return None;
    }
    Some(Hello {
        version: v.get("version")?.as_u64()?,
        token: v
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        client_id: v.get("clientId")?.as_str()?.to_string(),
        role: v.get("role")?.as_str()?.to_string(),
    })
}

pub fn hello_ok() -> String {
    json!({ "type": "hello", "ok": true }).to_string()
}

pub fn hello_err(error: &str) -> String {
    json!({ "type": "hello", "ok": false, "error": error }).to_string()
}

/// `RpcResponseOk` — `{ id, ok:true, payload }`.
pub fn rpc_ok(id: &str, payload: Value) -> String {
    json!({ "id": id, "ok": true, "payload": payload }).to_string()
}

/// `RpcResponseError` — `{ id, ok:false, error }`.
pub fn rpc_err(id: &str, error: &str) -> String {
    json!({ "id": id, "ok": false, "error": error }).to_string()
}

/// `DataEvent` — `{ type:'event', event:'data', sessionId, payload:{ data } }`.
pub fn data_event(session_id: &str, data: &str) -> String {
    json!({
        "type": "event",
        "event": "data",
        "sessionId": session_id,
        "payload": { "data": data }
    })
    .to_string()
}

/// `ExitEvent` — `{ type:'event', event:'exit', sessionId, payload:{ code } }`.
pub fn exit_event(session_id: &str, code: i64) -> String {
    json!({
        "type": "event",
        "event": "exit",
        "sessionId": session_id,
        "payload": { "code": code }
    })
    .to_string()
}
