//! Browser screencast binary framing, ported from
//! `src/shared/browser-screencast-protocol.ts`.
//!
//! A fixed 16-byte little-endian header (`kind | version | opcode | format`,
//! then a u32 seq, a u32 metadata length, and a u32 reserved word that must be
//! zero) followed by a compact-JSON metadata blob and the raw image bytes. This
//! mirrors the `terminal_stream` framing style; the relay multiplexes CDP
//! screencast frames over it for the remote/mobile browser view.

use serde_json::{Map, Value};

const BROWSER_SCREENCAST_KIND: u8 = 0x62;
const BROWSER_SCREENCAST_VERSION: u8 = 1;
const HEADER_BYTES: usize = 16;

/// Numeric metadata fields kept on decode — anything else (string values,
/// non-finite numbers, unknown keys) is dropped.
const METADATA_KEYS: [&str; 9] = [
    "offsetTop",
    "pageScaleFactor",
    "deviceWidth",
    "deviceHeight",
    "imageWidth",
    "imageHeight",
    "scrollOffsetX",
    "scrollOffsetY",
    "timestamp",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BrowserScreencastOpcode {
    Frame = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserScreencastFormat {
    Jpeg,
    Png,
}

impl BrowserScreencastFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
        }
    }
}

/// A decoded/encodable screencast frame. `metadata` is a JSON object whose
/// numeric fields survive a round trip (the encode path serializes whatever is
/// supplied; decode filters to the known finite-number keys).
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserScreencastFrame {
    pub opcode: BrowserScreencastOpcode,
    pub seq: u32,
    pub format: BrowserScreencastFormat,
    pub metadata: Value,
    pub image: Vec<u8>,
}

fn format_to_byte(format: BrowserScreencastFormat) -> u8 {
    match format {
        BrowserScreencastFormat::Png => 2,
        BrowserScreencastFormat::Jpeg => 1,
    }
}

fn byte_to_format(value: u8) -> Option<BrowserScreencastFormat> {
    match value {
        1 => Some(BrowserScreencastFormat::Jpeg),
        2 => Some(BrowserScreencastFormat::Png),
        _ => None,
    }
}

fn is_finite_number(value: &Value) -> bool {
    // JSON has no NaN/Infinity, so any JSON number is finite; `as_f64` is `None`
    // for strings/bools/null, matching TS `typeof === 'number' && isFinite`.
    value.as_f64().is_some_and(f64::is_finite)
}

fn decode_json(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes).ok()
}

fn decode_frame_metadata(bytes: &[u8]) -> Option<Value> {
    // Reject parse failures, null, arrays, and non-object scalars (TS checks
    // `!raw || typeof raw !== 'object' || Array.isArray(raw)`).
    let object = match decode_json(bytes) {
        Some(Value::Object(map)) => map,
        _ => return None,
    };
    let mut metadata = Map::new();
    for key in METADATA_KEYS {
        if let Some(value) = object.get(key) {
            if is_finite_number(value) {
                metadata.insert(key.to_string(), value.clone());
            }
        }
    }
    Some(Value::Object(metadata))
}

// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the frame always carries the full fixed-width header.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<u8>| out.len() >= HEADER_BYTES))]
pub fn encode_browser_screencast_frame(frame: &BrowserScreencastFrame) -> Vec<u8> {
    // serde_json only fails on cyclic/non-string-key values, unreachable for the
    // plain metadata objects we frame; empty buffer keeps this panic-free.
    let metadata = serde_json::to_vec(&frame.metadata).unwrap_or_default();
    let mut out = Vec::with_capacity(HEADER_BYTES + metadata.len() + frame.image.len());
    out.push(BROWSER_SCREENCAST_KIND);
    out.push(BROWSER_SCREENCAST_VERSION);
    out.push(frame.opcode as u8);
    out.push(format_to_byte(frame.format));
    out.extend_from_slice(&frame.seq.to_le_bytes());
    out.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&metadata);
    out.extend_from_slice(&frame.image);
    out
}

pub fn decode_browser_screencast_frame(bytes: &[u8]) -> Option<BrowserScreencastFrame> {
    if bytes.len() < HEADER_BYTES {
        return None;
    }
    if bytes[0] != BROWSER_SCREENCAST_KIND || bytes[1] != BROWSER_SCREENCAST_VERSION {
        return None;
    }
    if bytes[2] != BrowserScreencastOpcode::Frame as u8 {
        return None;
    }
    let format = byte_to_format(bytes[3])?;
    let seq = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let metadata_length = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) != 0 {
        return None;
    }
    let payload_start = HEADER_BYTES;
    // checked_add keeps an oversized metadata length from overflowing usize.
    let image_start = payload_start.checked_add(metadata_length)?;
    if image_start > bytes.len() {
        return None;
    }
    let metadata = decode_frame_metadata(&bytes[payload_start..image_start])?;
    Some(BrowserScreencastFrame {
        opcode: BrowserScreencastOpcode::Frame,
        seq,
        format,
        metadata,
        image: bytes[image_start..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_frame_metadata_and_image_bytes() {
        let encoded = encode_browser_screencast_frame(&BrowserScreencastFrame {
            opcode: BrowserScreencastOpcode::Frame,
            seq: 42,
            format: BrowserScreencastFormat::Jpeg,
            metadata: json!({
                "deviceWidth": 1280,
                "deviceHeight": 720,
                "pageScaleFactor": 1,
                "timestamp": 123
            }),
            image: vec![1, 2, 3, 4],
        });

        let decoded = decode_browser_screencast_frame(&encoded);

        assert_eq!(
            decoded,
            Some(BrowserScreencastFrame {
                opcode: BrowserScreencastOpcode::Frame,
                seq: 42,
                format: BrowserScreencastFormat::Jpeg,
                metadata: json!({
                    "deviceWidth": 1280,
                    "deviceHeight": 720,
                    "pageScaleFactor": 1,
                    "timestamp": 123
                }),
                image: vec![1, 2, 3, 4],
            })
        );
    }

    #[test]
    fn rejects_unrelated_binary_frames() {
        assert_eq!(decode_browser_screencast_frame(&[0, 1, 2, 3]), None);
    }

    #[test]
    fn rejects_frames_with_an_unsupported_header_byte() {
        // Mirrors the TS it.each over version/opcode/format bytes.
        for (offset, value) in [(1usize, 2u8), (2, 9), (3, 9)] {
            let mut encoded = encode_browser_screencast_frame(&BrowserScreencastFrame {
                opcode: BrowserScreencastOpcode::Frame,
                seq: 1,
                format: BrowserScreencastFormat::Jpeg,
                metadata: json!({}),
                image: vec![1],
            });
            encoded[offset] = value;

            assert_eq!(decode_browser_screencast_frame(&encoded), None);
        }
    }

    #[test]
    fn rejects_frames_whose_metadata_length_exceeds_the_payload() {
        let mut encoded = encode_browser_screencast_frame(&BrowserScreencastFrame {
            opcode: BrowserScreencastOpcode::Frame,
            seq: 1,
            format: BrowserScreencastFormat::Jpeg,
            metadata: json!({}),
            image: vec![1],
        });
        let total = encoded.len() as u32;
        encoded[8..12].copy_from_slice(&total.to_le_bytes());

        assert_eq!(decode_browser_screencast_frame(&encoded), None);
    }

    #[test]
    fn rejects_frames_with_nonzero_reserved_header_bytes() {
        let mut encoded = encode_browser_screencast_frame(&BrowserScreencastFrame {
            opcode: BrowserScreencastOpcode::Frame,
            seq: 1,
            format: BrowserScreencastFormat::Jpeg,
            metadata: json!({}),
            image: vec![1],
        });
        encoded[12] = 1;

        assert_eq!(decode_browser_screencast_frame(&encoded), None);
    }

    #[test]
    fn rejects_non_object_metadata() {
        let encoded = encode_browser_screencast_frame(&BrowserScreencastFrame {
            opcode: BrowserScreencastOpcode::Frame,
            seq: 1,
            format: BrowserScreencastFormat::Jpeg,
            metadata: json!([]),
            image: vec![1],
        });

        assert_eq!(decode_browser_screencast_frame(&encoded), None);
    }

    #[test]
    fn keeps_only_finite_numeric_metadata_fields() {
        // NaN stringifies to null in JS, so it rides the wire as null here; the
        // string value, the null, and the unknown `extra` key are all dropped.
        let encoded = encode_browser_screencast_frame(&BrowserScreencastFrame {
            opcode: BrowserScreencastOpcode::Frame,
            seq: 1,
            format: BrowserScreencastFormat::Jpeg,
            metadata: json!({
                "deviceWidth": "1280",
                "deviceHeight": 720,
                "pageScaleFactor": null,
                "scrollOffsetX": 15,
                "extra": 42
            }),
            image: vec![1],
        });

        assert_eq!(
            decode_browser_screencast_frame(&encoded).map(|frame| frame.metadata),
            Some(json!({ "deviceHeight": 720, "scrollOffsetX": 15 }))
        );
    }
}
