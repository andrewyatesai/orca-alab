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
/// 1019 added the read-only SUBSCRIBER role; 1020 added the OPT-IN binary
/// stream plane (`streamFormat:'binary'` in the stream hello); 1021 adds the
/// federated-search RPCs (`searchSessions`/`searchContext`/`searchReplay`/
/// `searchReplayContext`, see session_search.rs) — all purely additive over 1018.
pub const PROTOCOL_VERSION: u64 = 1021;

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

/// v1021: the protocol version at which the federated-search RPCs exist.
/// Clients feature-detect on this (an older preserved daemon answers
/// "unsupported request type" and the source degrades to unavailable). Must
/// equal `SESSION_SEARCH_PROTOCOL_VERSION` in
/// `src/main/daemon/daemon-protocol-versions.ts`.
pub const SESSION_SEARCH_PROTOCOL_VERSION: u64 = 1021;

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
/// `FRAME_MAX_PAYLOAD` in daemon-frame-types.ts. The client's frame parser
/// DISCARDS any frame whose payload exceeds it, so the daemon never builds one —
/// which also keeps the u32 length field exact (no silent truncation) and every
/// frame allocation bounded.
pub const FRAME_MAX_PAYLOAD: usize = 1024 * 1024;
/// PTY text carried per Data frame. A PTY read (64 KiB) plus the stream
/// coalescer's merge (`STREAM_COALESCE_MAX_BYTES`) stays under it, so today's
/// traffic is one frame per item, byte-identical to before. Anything larger is
/// split across consecutive frames on char boundaries (the client already
/// treats data-chunk boundaries as arbitrary). Sized so the JSON fallback event
/// (≤ 6× escape expansion) still fits `FRAME_MAX_PAYLOAD` for any sane session id.
pub const DATA_FRAME_MAX_TEXT_BYTES: usize = 128 * 1024;

/// Append one `[type][len:u32 BE][payload]` frame to `out`. Returns false — and
/// appends nothing — when the payload exceeds `FRAME_MAX_PAYLOAD` (a frame the
/// client would discard).
fn push_frame(out: &mut Vec<u8>, frame_type: u8, payload_parts: &[&[u8]]) -> bool {
    let Some(payload_len) = payload_parts
        .iter()
        .try_fold(0usize, |sum, part| sum.checked_add(part.len()))
    else {
        return false;
    };
    if payload_len > FRAME_MAX_PAYLOAD {
        return false;
    }
    let Ok(len_field) = u32::try_from(payload_len) else {
        return false;
    };
    out.reserve(FRAME_HEADER_SIZE + payload_len);
    out.push(frame_type);
    out.extend_from_slice(&len_field.to_be_bytes());
    for part in payload_parts {
        out.extend_from_slice(part);
    }
    true
}

/// Consecutive pieces of `text`, each at most `max` bytes (or one char, when a
/// single char is longer than `max`) and ending on a char boundary. Empty text
/// yields one empty piece, so an empty chunk still encodes as one frame.
fn utf8_pieces(text: &str, max: usize) -> impl Iterator<Item = &str> {
    let mut rest = text;
    let mut emitted_any = false;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            if emitted_any {
                return None;
            }
            emitted_any = true;
            return Some("");
        }
        emitted_any = true;
        let mut cut = rest.floor_char_boundary(max);
        if cut == 0 {
            cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        let (piece, tail) = rest.split_at_checked(cut)?;
        rest = tail;
        Some(piece)
    })
}

/// A PTY-output Data frame: the session id (u8 length prefix) followed by the
/// chunk's raw UTF-8 bytes — no JSON, no escape expansion. A session id that
/// cannot fit the u8 prefix (>255 bytes; client-supplied, so possible in
/// theory) falls back to a JSON data event in an Event frame, which the binary
/// client already routes through its normal event path. Text beyond
/// `DATA_FRAME_MAX_TEXT_BYTES` becomes several consecutive frames; a fallback
/// event that still cannot fit `FRAME_MAX_PAYLOAD` (only a pathologically long
/// session id) is not emitted, exactly as the client would discard it.
pub fn data_frame(session_id: &str, data: &str) -> Vec<u8> {
    let sid = session_id.as_bytes();
    let mut out = Vec::new();
    for piece in utf8_pieces(data, DATA_FRAME_MAX_TEXT_BYTES) {
        match u8::try_from(sid.len()) {
            Ok(sid_len) => {
                push_frame(
                    &mut out,
                    FRAME_TYPE_DATA,
                    &[&[sid_len], sid, piece.as_bytes()],
                );
            }
            Err(_) => {
                let event = data_event(session_id, piece);
                push_frame(&mut out, FRAME_TYPE_EVENT, &[event.as_bytes()]);
            }
        }
    }
    out
}

/// A non-data stream event (exit today; any tolerated additive event later),
/// carried as its NDJSON-identical JSON text inside an Event frame so the
/// binary stream needs exactly one parser. `None` when the JSON exceeds
/// `FRAME_MAX_PAYLOAD` — a frame the client would discard unread.
pub fn event_frame(event_json: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    push_frame(&mut out, FRAME_TYPE_EVENT, &[event_json.as_bytes()]).then_some(out)
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
        let f = event_frame(&json).expect("a small event fits a frame");
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

    /// Walk a byte stream of frames → `(type, payload)` per frame, checking each
    /// header's length is exact and within the client's `FRAME_MAX_PAYLOAD`.
    fn split_frames(mut bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut frames = Vec::new();
        while !bytes.is_empty() {
            let len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
            assert!(
                len <= FRAME_MAX_PAYLOAD,
                "frame payload {len} over the client cap"
            );
            frames.push((
                bytes[0],
                bytes[FRAME_HEADER_SIZE..FRAME_HEADER_SIZE + len].to_vec(),
            ));
            bytes = &bytes[FRAME_HEADER_SIZE + len..];
        }
        frames
    }

    #[test]
    fn oversized_data_splits_into_capped_frames_on_char_boundaries() {
        // 3-byte chars so a naive byte cut would land mid-char.
        let text = "€".repeat(DATA_FRAME_MAX_TEXT_BYTES); // 3× the per-frame text cap
        let frames = split_frames(&data_frame("sess-1", &text));
        assert!(
            frames.len() >= 3,
            "split into several frames, got {}",
            frames.len()
        );
        let mut rejoined = String::new();
        for (ty, payload) in &frames {
            assert_eq!(*ty, FRAME_TYPE_DATA);
            assert_eq!(payload[0] as usize, "sess-1".len());
            assert_eq!(&payload[1..7], b"sess-1");
            let piece = std::str::from_utf8(&payload[7..]).expect("each piece is whole UTF-8");
            assert!(piece.len() <= DATA_FRAME_MAX_TEXT_BYTES);
            rejoined.push_str(piece);
        }
        assert_eq!(
            rejoined, text,
            "the pieces concatenate to the original text"
        );
    }

    #[test]
    fn data_at_the_text_cap_and_empty_data_are_one_frame_each() {
        let at_cap = "x".repeat(DATA_FRAME_MAX_TEXT_BYTES);
        assert_eq!(split_frames(&data_frame("s", &at_cap)).len(), 1);
        let empty = split_frames(&data_frame("s", ""));
        assert_eq!(empty.len(), 1, "an empty chunk still encodes as one frame");
        assert_eq!(empty[0].1, vec![1u8, b's']);
    }

    #[test]
    fn oversized_fallback_data_is_split_into_capped_json_events() {
        let sid = "s".repeat(300);
        let text = "y".repeat(DATA_FRAME_MAX_TEXT_BYTES * 2 + 1);
        let frames = split_frames(&data_frame(&sid, &text));
        assert_eq!(frames.len(), 3);
        let mut rejoined = String::new();
        for (ty, payload) in &frames {
            assert_eq!(*ty, FRAME_TYPE_EVENT);
            let v: serde_json::Value = serde_json::from_slice(payload).expect("valid JSON");
            rejoined.push_str(v["payload"]["data"].as_str().unwrap());
        }
        assert_eq!(rejoined, text);
    }

    #[test]
    fn event_over_the_client_cap_is_refused_not_truncated() {
        let at_cap = "e".repeat(FRAME_MAX_PAYLOAD);
        let f = event_frame(&at_cap).expect("exactly the cap is a legal frame");
        assert_eq!(f.len(), FRAME_HEADER_SIZE + FRAME_MAX_PAYLOAD);
        assert_eq!(event_frame(&"e".repeat(FRAME_MAX_PAYLOAD + 1)), None);
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
