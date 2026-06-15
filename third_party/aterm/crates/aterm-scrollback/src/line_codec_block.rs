// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Block-level serialization for multiple scrollback lines.
//!
//! Used by warm and cold tiers to serialize/deserialize blocks of lines
//! for compressed storage. Handles legacy (v0), v1, v2, and v3 line formats.
//!
//! All deserialized lengths use checked arithmetic to prevent overflow on
//! malicious or corrupt input (#4950).

use super::Line;

/// Serialize multiple lines for block compression.
#[must_use]
pub fn serialize_lines(lines: &[Line]) -> Vec<u8> {
    // Format: [count:4][line0][line1]...
    //
    // Pre-allocate from content sizes to avoid repeated Vec doublings
    // on the warm-tier compaction hot path (#5860).
    // Per v3 line: 9 bytes fixed overhead (version + flags + content_len
    // + has_attrs + hyperlink_count) plus content bytes.
    let content_bytes: usize = lines.iter().map(Line::len).sum();
    let mut result = Vec::with_capacity(4 + content_bytes + lines.len() * 9);
    // Block size is bounded by warm tier settings (typically 256-4096 lines)
    // Saturate at u32::MAX for safety
    let count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    result.extend_from_slice(&count.to_le_bytes());
    for line in lines {
        let serialized = line.serialize();
        result.extend_from_slice(&serialized);
    }
    result
}

/// Compute the byte size of a legacy (v0) line record.
///
/// Format: `[flags:1][len:4][content:len]`
/// Returns `None` if `data` is truncated or size overflows.
fn line_size_v0(data: &[u8]) -> Option<usize> {
    if data.len() < 5 {
        return None;
    }
    let content_len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    5usize.checked_add(content_len)
}

/// Compute the byte size of a v1/v2/v3 line record.
///
/// Format: `[version:1][flags:1][len:4][content:len][has_attrs:1][attrs...][hyperlinks]`
/// Returns `None` if `data` is truncated or any size computation overflows.
fn line_size_v1v2(data: &[u8], version: u8) -> Option<usize> {
    if data.len() < 7 {
        return None;
    }
    let content_len = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
    let attrs_start = 6usize.checked_add(content_len)?;
    if attrs_start >= data.len() {
        return None;
    }
    let has_attrs = data[attrs_start] != 0;
    let attrs_size = if has_attrs {
        if attrs_start + 5 > data.len() {
            return None;
        }
        let run_count = u32::from_le_bytes([
            data[attrs_start + 1],
            data[attrs_start + 2],
            data[attrs_start + 3],
            data[attrs_start + 4],
        ]) as usize;
        let runs_size = run_count.checked_mul(14)?;
        runs_size.checked_add(5)?
    } else {
        1
    };

    let base_size = 6usize.checked_add(content_len)?.checked_add(attrs_size)?;

    if version >= 3 {
        hyperlinks_size_v3(data, base_size).and_then(|hl| base_size.checked_add(hl))
    } else if version >= 2 {
        hyperlinks_size_v2(data, base_size).and_then(|hl| base_size.checked_add(hl))
    } else {
        Some(base_size)
    }
}

/// Compute the byte size of the v2 hyperlinks section (no IDs).
fn hyperlinks_size_v2(data: &[u8], base_size: usize) -> Option<usize> {
    if base_size.checked_add(2)? > data.len() {
        return None;
    }
    let count = u16::from_le_bytes([data[base_size], data[base_size + 1]]) as usize;
    let mut size = 2usize;
    let mut pos = base_size.checked_add(2)?;
    for _ in 0..count {
        if pos.checked_add(8)? > data.len() {
            break;
        }
        let url_len =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        let advance = 8usize.checked_add(url_len)?;
        size = size.checked_add(advance)?;
        pos = pos.checked_add(advance)?;
    }
    Some(size)
}

/// Compute the byte size of the v3 hyperlinks section (with IDs).
fn hyperlinks_size_v3(data: &[u8], base_size: usize) -> Option<usize> {
    if base_size.checked_add(2)? > data.len() {
        return None;
    }
    let count = u16::from_le_bytes([data[base_size], data[base_size + 1]]) as usize;
    let mut size = 2usize;
    let mut pos = base_size.checked_add(2)?;
    for _ in 0..count {
        if pos.checked_add(8)? > data.len() {
            break;
        }
        let url_len =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        pos = pos.checked_add(8)?.checked_add(url_len)?;
        if pos.checked_add(4)? > data.len() {
            break;
        }
        let id_len =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let span_size = 8usize
            .checked_add(url_len)?
            .checked_add(4)?
            .checked_add(id_len)?;
        size = size.checked_add(span_size)?;
        pos = pos.checked_add(4)?.checked_add(id_len)?;
    }
    Some(size)
}

/// Deserialize multiple lines from block.
///
/// Handles legacy (v0), v1, v2, and v3 line formats by computing
/// line size dynamically from the serialized data.
#[must_use]
pub fn deserialize_lines(data: &[u8]) -> Vec<Line> {
    if data.len() < 4 {
        return Vec::new();
    }

    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    // Clamp pre-allocation to what the data can actually contain.
    // Minimum serialized line is 5 bytes (v0: flags + content_len + empty content).
    let max_possible = (data.len() - 4) / 5;
    let mut lines = Vec::with_capacity(count.min(max_possible));
    let mut offset = 4;

    while offset < data.len() && lines.len() < count {
        let version = data[offset];
        let record = &data[offset..];

        let line_size = if version == 0 {
            line_size_v0(record)
        } else {
            line_size_v1v2(record, version)
        };

        let Some(size) = line_size else { break };
        let Some(line_end) = offset.checked_add(size) else {
            break;
        };
        if line_end > data.len() {
            break;
        }

        if let Some(line) = Line::deserialize(&data[offset..line_end]) {
            lines.push(line);
        }
        offset = line_end;
    }

    lines
}
