//! Terminal binary-stream framing, ported from `src/shared/terminal-stream-protocol.ts`.
//!
//! A fixed 16-byte little-endian header (`kind | version | opcode | reserved`,
//! then a u32 stream id and a 64-bit sequence split high/low) followed by an
//! opaque payload. The payload is either UTF-8 text or a JSON document; helpers
//! cover both. This is the wire format the remote/mobile relay multiplexes
//! terminal output, input, resizes, and snapshot chunks over.

use serde::{de::DeserializeOwned, Serialize};

const TERMINAL_STREAM_KIND: u8 = 0x74;
const TERMINAL_STREAM_VERSION: u8 = 1;
const HEADER_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminalStreamOpcode {
    Output = 1,
    SnapshotStart = 2,
    SnapshotChunk = 3,
    SnapshotEnd = 4,
    Resized = 5,
    Error = 6,
    Input = 7,
    Resize = 8,
    Subscribe = 9,
    Unsubscribe = 10,
}

impl TerminalStreamOpcode {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Output),
            2 => Some(Self::SnapshotStart),
            3 => Some(Self::SnapshotChunk),
            4 => Some(Self::SnapshotEnd),
            5 => Some(Self::Resized),
            6 => Some(Self::Error),
            7 => Some(Self::Input),
            8 => Some(Self::Resize),
            9 => Some(Self::Subscribe),
            10 => Some(Self::Unsubscribe),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalStreamFrame {
    pub opcode: TerminalStreamOpcode,
    pub stream_id: u32,
    pub seq: u64,
    pub payload: Vec<u8>,
}

pub fn encode_terminal_stream_frame(frame: &TerminalStreamFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + frame.payload.len());
    out.push(TERMINAL_STREAM_KIND);
    out.push(TERMINAL_STREAM_VERSION);
    out.push(frame.opcode as u8);
    out.push(0);
    out.extend_from_slice(&frame.stream_id.to_le_bytes());
    // 64-bit seq split into two little-endian u32 words (high then low), matching
    // the JS DataView writes that work around its 32-bit integer ops.
    out.extend_from_slice(&((frame.seq >> 32) as u32).to_le_bytes());
    out.extend_from_slice(&(frame.seq as u32).to_le_bytes());
    out.extend_from_slice(&frame.payload);
    out
}

pub fn decode_terminal_stream_frame(bytes: &[u8]) -> Option<TerminalStreamFrame> {
    if bytes.len() < HEADER_BYTES {
        return None;
    }
    if bytes[0] != TERMINAL_STREAM_KIND || bytes[1] != TERMINAL_STREAM_VERSION {
        return None;
    }
    let opcode = TerminalStreamOpcode::from_u8(bytes[2])?;
    let stream_id = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let high = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let low = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    Some(TerminalStreamFrame {
        opcode,
        stream_id,
        seq: (u64::from(high) << 32) | u64::from(low),
        payload: bytes[HEADER_BYTES..].to_vec(),
    })
}

/// Encode any serializable value as a JSON payload. Returns an empty buffer if
/// serialization fails (mirrors JS where only cyclic values throw — unreachable
/// for the plain data we frame).
pub fn encode_terminal_stream_json<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

/// Decode a JSON payload, returning `None` on malformed JSON (matches the JS
/// try/catch that yields `null`).
pub fn decode_terminal_stream_json<T: DeserializeOwned>(payload: &[u8]) -> Option<T> {
    serde_json::from_slice(payload).ok()
}

pub fn encode_terminal_stream_text(value: &str) -> Vec<u8> {
    value.as_bytes().to_vec()
}

/// Decode a UTF-8 payload, replacing invalid sequences with U+FFFD to match the
/// browser `TextDecoder` default.
pub fn decode_terminal_stream_text(payload: &[u8]) -> String {
    String::from_utf8_lossy(payload).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn round_trips_fixed_width_binary_frame_headers_and_payloads() {
        let payload = encode_terminal_stream_text("hello terminal");
        let encoded = encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::Output,
            stream_id: 42,
            seq: 9,
            payload,
        });

        let decoded = decode_terminal_stream_frame(&encoded).unwrap();

        assert_eq!(decoded.opcode, TerminalStreamOpcode::Output);
        assert_eq!(decoded.stream_id, 42);
        assert_eq!(decoded.seq, 9);
        assert_eq!(decode_terminal_stream_text(&decoded.payload), "hello terminal");
    }

    #[test]
    fn round_trips_snapshot_metadata_json_payloads() {
        let encoded = encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::SnapshotStart,
            stream_id: 7,
            seq: 1,
            payload: encode_terminal_stream_json(&json!({ "kind": "scrollback", "cols": 49, "rows": 28 })),
        });

        let decoded = decode_terminal_stream_frame(&encoded).unwrap();

        assert_eq!(
            decode_terminal_stream_json::<Value>(&decoded.payload),
            Some(json!({ "kind": "scrollback", "cols": 49, "rows": 28 }))
        );
    }

    #[test]
    fn round_trips_terminal_input_and_resize_frames() {
        let input = decode_terminal_stream_frame(&encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::Input,
            stream_id: 11,
            seq: 1,
            payload: encode_terminal_stream_text("a"),
        }))
        .unwrap();
        let resize = decode_terminal_stream_frame(&encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::Resize,
            stream_id: 11,
            seq: 2,
            payload: encode_terminal_stream_json(&json!({ "cols": 120, "rows": 40 })),
        }))
        .unwrap();

        assert_eq!(input.opcode, TerminalStreamOpcode::Input);
        assert_eq!(decode_terminal_stream_text(&input.payload), "a");
        assert_eq!(resize.opcode, TerminalStreamOpcode::Resize);
        assert_eq!(
            decode_terminal_stream_json::<Value>(&resize.payload),
            Some(json!({ "cols": 120, "rows": 40 }))
        );
    }

    #[test]
    fn round_trips_multiplex_subscribe_and_unsubscribe_frames() {
        let subscribe = decode_terminal_stream_frame(&encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::Subscribe,
            stream_id: 0,
            seq: 1,
            payload: encode_terminal_stream_json(&json!({
                "streamId": 12,
                "terminal": "terminal-1",
                "viewport": { "cols": 120, "rows": 40 }
            })),
        }))
        .unwrap();
        let unsubscribe = decode_terminal_stream_frame(&encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::Unsubscribe,
            stream_id: 12,
            seq: 2,
            payload: Vec::new(),
        }))
        .unwrap();

        assert_eq!(subscribe.opcode, TerminalStreamOpcode::Subscribe);
        let payload = decode_terminal_stream_json::<Value>(&subscribe.payload).unwrap();
        assert_eq!(payload.get("streamId"), Some(&json!(12)));
        assert_eq!(payload.get("terminal"), Some(&json!("terminal-1")));
        assert_eq!(unsubscribe.opcode, TerminalStreamOpcode::Unsubscribe);
        assert_eq!(unsubscribe.stream_id, 12);
    }

    #[test]
    fn rejects_unknown_frame_versions_and_opcodes() {
        let encoded = encode_terminal_stream_frame(&TerminalStreamFrame {
            opcode: TerminalStreamOpcode::Output,
            stream_id: 1,
            seq: 1,
            payload: Vec::new(),
        });

        let mut bad_version = encoded.clone();
        bad_version[1] = 99;
        assert_eq!(decode_terminal_stream_frame(&bad_version), None);

        let mut bad_opcode = encoded.clone();
        bad_opcode[2] = 99;
        assert_eq!(decode_terminal_stream_frame(&bad_opcode), None);
    }
}
