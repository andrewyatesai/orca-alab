//! Parity dispatch for `orca_relay::terminal_stream` vs
//! `src/shared/terminal-stream-protocol.ts`.
//!
//! Byte buffers (`Vec<u8>` / `Uint8Array`) cross the vector boundary as plain
//! number arrays so the goldens stay valid JSON and compare structurally
//! against the TS side; this adapter converts at the edge.

use orca_relay::{
    decode_terminal_stream_frame, decode_terminal_stream_json, decode_terminal_stream_text,
    encode_terminal_stream_frame, encode_terminal_stream_json, encode_terminal_stream_text,
    TerminalStreamFrame, TerminalStreamOpcode,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "encodeTerminalStreamFrame" => match frame_from_json(input) {
            Some(frame) => bytes_to_json(&encode_terminal_stream_frame(&frame)),
            // Vectors only carry known opcodes; an unknown one is a vector bug.
            None => parity_error("invalid frame input"),
        },
        "decodeTerminalStreamFrame" => {
            let bytes = bytes_from_json(input.get("bytes"));
            match decode_terminal_stream_frame(&bytes) {
                Some(frame) => frame_to_json(&frame),
                None => Value::Null,
            }
        }
        "encodeTerminalStreamJson" => {
            let value = input.get("value").cloned().unwrap_or(Value::Null);
            // serde_json is unified with `preserve_order` in this binary, so key
            // order matches JS `JSON.stringify` insertion order byte-for-byte.
            bytes_to_json(&encode_terminal_stream_json(&value))
        }
        "decodeTerminalStreamJson" => {
            let payload = bytes_from_json(input.get("payload"));
            decode_terminal_stream_json::<Value>(&payload).unwrap_or(Value::Null)
        }
        "encodeTerminalStreamText" => {
            let value = input.get("value").and_then(Value::as_str).unwrap_or_default();
            bytes_to_json(&encode_terminal_stream_text(value))
        }
        "decodeTerminalStreamText" => {
            let payload = bytes_from_json(input.get("payload"));
            Value::String(decode_terminal_stream_text(&payload))
        }
        other => parity_error(&format!("unknown function {other}")),
    }
}

/// Match `JSON.stringify` of the decoded TS frame: numeric `opcode` (the enum is
/// numeric in TS), `streamId`, `seq`, and `payload` as a byte-number array.
fn frame_to_json(frame: &TerminalStreamFrame) -> Value {
    json!({
        "opcode": frame.opcode as u8,
        "streamId": frame.stream_id,
        "seq": frame.seq,
        "payload": bytes_to_json(&frame.payload),
    })
}

fn frame_from_json(input: &Value) -> Option<TerminalStreamFrame> {
    let opcode = opcode_from_u8(input.get("opcode")?.as_u64()? as u8)?;
    let stream_id = input.get("streamId")?.as_u64()? as u32;
    let seq = input.get("seq")?.as_u64()?;
    let payload = bytes_from_json(input.get("payload"));
    Some(TerminalStreamFrame { opcode, stream_id, seq, payload })
}

/// `TerminalStreamOpcode::from_u8` is private to the relay crate, so the dispatch
/// reproduces the numeric-id → variant mapping from the public enum.
fn opcode_from_u8(value: u8) -> Option<TerminalStreamOpcode> {
    match value {
        1 => Some(TerminalStreamOpcode::Output),
        2 => Some(TerminalStreamOpcode::SnapshotStart),
        3 => Some(TerminalStreamOpcode::SnapshotChunk),
        4 => Some(TerminalStreamOpcode::SnapshotEnd),
        5 => Some(TerminalStreamOpcode::Resized),
        6 => Some(TerminalStreamOpcode::Error),
        7 => Some(TerminalStreamOpcode::Input),
        8 => Some(TerminalStreamOpcode::Resize),
        9 => Some(TerminalStreamOpcode::Subscribe),
        10 => Some(TerminalStreamOpcode::Unsubscribe),
        _ => None,
    }
}

fn bytes_to_json(bytes: &[u8]) -> Value {
    Value::Array(bytes.iter().map(|b| Value::from(*b)).collect())
}

fn bytes_from_json(value: Option<&Value>) -> Vec<u8> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect())
        .unwrap_or_default()
}

fn parity_error(message: &str) -> Value {
    json!({ "__parity_error__": message })
}
