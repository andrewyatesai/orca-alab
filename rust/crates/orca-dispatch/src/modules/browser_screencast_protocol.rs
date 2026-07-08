//! Parity dispatch for `orca_relay::browser_screencast_protocol` vs
//! `src/shared/browser-screencast-protocol.ts`.
//!
//! Image bytes (`Vec<u8>` / `Uint8Array`) cross the vector boundary as plain
//! number arrays so the goldens stay valid JSON; this adapter converts at the
//! edge. The decoded frame's numeric opcode and string format match
//! `JSON.stringify` of the TS return.

use orca_relay::{
    decode_browser_screencast_frame, encode_browser_screencast_frame, BrowserScreencastFormat,
    BrowserScreencastFrame, BrowserScreencastOpcode,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "encodeBrowserScreencastFrame" => match frame_from_json(input) {
            Some(frame) => bytes_to_json(&encode_browser_screencast_frame(&frame)),
            None => parity_error("invalid frame input"),
        },
        "decodeBrowserScreencastFrame" => {
            let bytes = bytes_from_json(input.get("bytes"));
            match decode_browser_screencast_frame(&bytes) {
                Some(frame) => frame_to_json(&frame),
                None => Value::Null,
            }
        }
        other => parity_error(&format!("unknown function {other}")),
    }
}

/// Match `JSON.stringify` of the decoded TS frame: numeric `opcode` (the enum is
/// numeric in TS), `seq`, string `format`, the filtered `metadata` object, and
/// `image` as a byte-number array.
fn frame_to_json(frame: &BrowserScreencastFrame) -> Value {
    json!({
        "opcode": frame.opcode as u8,
        "seq": frame.seq,
        "format": frame.format.as_str(),
        "metadata": frame.metadata.clone(),
        "image": bytes_to_json(&frame.image),
    })
}

fn frame_from_json(input: &Value) -> Option<BrowserScreencastFrame> {
    let seq = input.get("seq")?.as_u64()? as u32;
    let format = format_from_str(input.get("format")?.as_str()?)?;
    let metadata = input.get("metadata").cloned().unwrap_or(Value::Null);
    let image = bytes_from_json(input.get("image"));
    Some(BrowserScreencastFrame {
        // The TS dispatch hardcodes the Frame opcode, mirroring the encode caller.
        opcode: BrowserScreencastOpcode::Frame,
        seq,
        format,
        metadata,
        image,
    })
}

/// The format string → enum mapping is private to the relay crate, so the
/// dispatch reproduces it from the public `as_str` variants.
fn format_from_str(value: &str) -> Option<BrowserScreencastFormat> {
    match value {
        "jpeg" => Some(BrowserScreencastFormat::Jpeg),
        "png" => Some(BrowserScreencastFormat::Png),
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
