//! The daemon socket protocol, mirroring `src/main/daemon/types.ts`: the `hello`
//! handshake, id-correlated `RpcResponse`, and the `data`/`exit` stream events.
//! Requests are read as `serde_json::Value` (no derive), matching orca-relay's
//! payload handling; responses/events are built with `json!`. The Rust daemon must
//! be indistinguishable from the Node one at this wire, so these shapes track
//! types.ts exactly.

use serde_json::{json, Value};

/// Must equal `PROTOCOL_VERSION` in `src/main/daemon/types.ts`. A client hello is
/// accepted anywhere in `MIN_SUPPORTED_PROTOCOL_VERSION..=PROTOCOL_VERSION`;
/// anything else is rejected with a `hello` error.
///
/// Why 10xx (not 19): the fork reserves the 1000+ namespace so its daemon
/// endpoints (`daemon-v10xx.*`, keyed off this number) never collide with a
/// public Orca install — a public build (v18, or any future public bump) must
/// never handshake with this daemon, and vice versa (see types.ts).
///
/// 1019 added the read-only SUBSCRIBER role; 1020 adds the OPT-IN binary
/// stream plane (`streamFormat:'binary'` in the stream hello) — both purely
/// additive over 1018.
pub const PROTOCOL_VERSION: u64 = 1020;

/// Oldest hello still accepted. 1019/1020 only ADD behavior, so a 1018 client
/// (an app build predating the subscriber rev, or the parity harness'
/// back-compat leg) stays fully functional against this daemon.
pub const MIN_SUPPORTED_PROTOCOL_VERSION: u64 = 1018;

/// v1020: a stream-role hello may carry `streamFormat:'binary'`. When granted
/// (echoed in the hello_ok), every daemon→client stream message is a binary
/// frame — PTY data as raw bytes (no JSON escape expansion), other events as
/// JSON wrapped in an Event frame. Must equal the TS constant in
/// `src/main/daemon/daemon-binary-stream-protocol.ts`.
pub const BINARY_STREAM_PROTOCOL_VERSION: u64 = 1020;

/// The `streamFormat` value requesting/granting binary stream frames.
pub const STREAM_FORMAT_BINARY: &str = "binary";

/// Typed error-code prefix for subscriber write/resize denial (v1019).
/// Clients match on this prefix; must equal `SUBSCRIBER_READ_ONLY_ERROR` in
/// `src/main/daemon/types.ts`.
pub const SUBSCRIBER_READ_ONLY_ERROR: &str = "subscriber-read-only";

/// The first line on every socket: `{ type:'hello', version, token, clientId, role }`.
pub struct Hello {
    pub version: u64,
    /// Validated against the daemon's published token when one is configured
    /// (see `connection::handle_connection`).
    pub token: String,
    pub client_id: String,
    /// `"control"` (RPC) or `"stream"` (events). Each client opens one of each.
    pub role: String,
    /// v1020 stream hellos: `Some("binary")` requests binary stream frames.
    /// Absent (older clients, control sockets) means NDJSON — the default.
    pub stream_format: Option<String>,
}

impl Hello {
    /// True when this hello negotiates the v1020 binary stream plane: a
    /// stream-role socket, at a version that knows the format, explicitly
    /// asking for it. Everything else stays NDJSON (additive opt-in).
    pub fn requests_binary_stream(&self) -> bool {
        self.role == "stream"
            && self.version >= BINARY_STREAM_PROTOCOL_VERSION
            && self.stream_format.as_deref() == Some(STREAM_FORMAT_BINARY)
    }
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
        stream_format: v
            .get("streamFormat")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub fn hello_ok() -> String {
    json!({ "type": "hello", "ok": true }).to_string()
}

/// hello_ok that GRANTS the binary stream plane by echoing `streamFormat`.
/// The client only switches its parser on this echo, so a daemon that ignores
/// the request (or this build answering a legacy hello) keeps NDJSON safely.
pub fn hello_ok_binary_stream() -> String {
    json!({ "type": "hello", "ok": true, "streamFormat": STREAM_FORMAT_BINARY }).to_string()
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

// ─── v1020 binary stream frames ──────────────────────────────────────────────
// Mirrors `src/main/daemon/binary-frame.ts`: [type:u8][len:u32 BE][payload].
// Data-frame payload: [sidLen:u8][sessionId utf8][raw pty bytes] — see
// `src/main/daemon/daemon-binary-stream-protocol.ts`.

/// `[type:1][length:4 BE]` — must equal `FRAME_HEADER_SIZE` in types.ts.
pub const FRAME_HEADER_SIZE: usize = 5;
/// `FrameType.Data` in types.ts.
pub const FRAME_TYPE_DATA: u8 = 0x01;
/// `FrameType.Event` in types.ts — a JSON stream-event line as frame payload.
pub const FRAME_TYPE_EVENT: u8 = 0x07;

fn frame(frame_type: u8, payload_parts: &[&[u8]]) -> Vec<u8> {
    let payload_len: usize = payload_parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(FRAME_HEADER_SIZE + payload_len);
    out.push(frame_type);
    out.extend_from_slice(&(payload_len as u32).to_be_bytes());
    for part in payload_parts {
        out.extend_from_slice(part);
    }
    out
}

/// A PTY-output Data frame: the session id (u8 length prefix) followed by the
/// chunk's raw UTF-8 bytes — no JSON, no escape expansion. A session id that
/// cannot fit the u8 prefix (>255 bytes; client-supplied, so possible in
/// theory) falls back to a JSON data event in an Event frame, which the binary
/// client already routes through its normal event path.
pub fn data_frame(session_id: &str, data: &str) -> Vec<u8> {
    let sid = session_id.as_bytes();
    if sid.len() > u8::MAX as usize {
        return event_frame(&data_event(session_id, data));
    }
    frame(FRAME_TYPE_DATA, &[&[sid.len() as u8], sid, data.as_bytes()])
}

/// A non-data stream event (exit today; any tolerated additive event later),
/// carried as its NDJSON-identical JSON text inside an Event frame so the
/// binary stream needs exactly one parser.
pub fn event_frame(event_json: &str) -> Vec<u8> {
    frame(FRAME_TYPE_EVENT, &[event_json.as_bytes()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_frame_layout_is_header_sid_prefix_raw_bytes() {
        let f = data_frame("sess-1", "a\x1b[32mb");
        assert_eq!(f[0], FRAME_TYPE_DATA);
        let len = u32::from_be_bytes([f[1], f[2], f[3], f[4]]) as usize;
        assert_eq!(len, f.len() - FRAME_HEADER_SIZE);
        assert_eq!(f[FRAME_HEADER_SIZE] as usize, "sess-1".len());
        let sid_end = FRAME_HEADER_SIZE + 1 + "sess-1".len();
        assert_eq!(&f[FRAME_HEADER_SIZE + 1..sid_end], b"sess-1");
        // The payload bytes are RAW — the ESC survives unexpanded (the whole
        // point vs NDJSON's  six-byte escape).
        assert_eq!(&f[sid_end..], "a\x1b[32mb".as_bytes());
    }

    #[test]
    fn event_frame_carries_the_exact_json_text() {
        let json = exit_event("s", 0);
        let f = event_frame(&json);
        assert_eq!(f[0], FRAME_TYPE_EVENT);
        assert_eq!(&f[FRAME_HEADER_SIZE..], json.as_bytes());
    }

    #[test]
    fn oversized_session_id_falls_back_to_an_event_frame() {
        let sid = "s".repeat(300);
        let f = data_frame(&sid, "x");
        assert_eq!(f[0], FRAME_TYPE_EVENT, "unencodable sid → JSON data event");
        let v: serde_json::Value =
            serde_json::from_slice(&f[FRAME_HEADER_SIZE..]).expect("valid JSON");
        assert_eq!(v["event"], "data");
        assert_eq!(v["sessionId"].as_str().unwrap(), sid);
    }

    #[test]
    fn binary_stream_negotiation_requires_role_version_and_format() {
        let hello = |version: u64, role: &str, fmt: Option<&str>| Hello {
            version,
            token: String::new(),
            client_id: "c".into(),
            role: role.into(),
            stream_format: fmt.map(str::to_string),
        };
        assert!(hello(1020, "stream", Some("binary")).requests_binary_stream());
        assert!(!hello(1020, "control", Some("binary")).requests_binary_stream());
        assert!(!hello(1019, "stream", Some("binary")).requests_binary_stream());
        assert!(!hello(1020, "stream", None).requests_binary_stream());
        assert!(!hello(1020, "stream", Some("ndjson")).requests_binary_stream());
    }
}
