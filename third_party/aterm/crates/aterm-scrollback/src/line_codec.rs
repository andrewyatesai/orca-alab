// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Serialization helpers for scrollback lines.

use super::{CellAttrs, HyperlinkSpan, Line, LineContent, LineFlags};
use aterm_alloc::SmallVec;
use aterm_rle::Rle;
use std::sync::Arc;

impl CellAttrs {
    /// Serialize to bytes (10 bytes).
    #[must_use]
    pub(crate) fn serialize(&self) -> [u8; 10] {
        let mut buf = [0u8; 10];
        buf[0..4].copy_from_slice(&self.fg.to_le_bytes());
        buf[4..8].copy_from_slice(&self.bg.to_le_bytes());
        buf[8..10].copy_from_slice(&self.flags.to_le_bytes());
        buf
    }

    /// Deserialize from bytes.
    #[must_use]
    pub(crate) fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }
        let fg = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let bg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let flags = u16::from_le_bytes([data[8], data[9]]);
        Some(Self { fg, bg, flags })
    }
}

impl Line {
    /// Serialize line to bytes for compression.
    ///
    /// Format v3 (with attrs, hyperlinks, and hyperlink IDs):
    /// ```text
    /// [version:1][flags:1][content_len:4][content:content_len]
    /// [has_attrs:1][if has_attrs: run_count:4 + runs...]
    /// [hyperlink_count:2][foreach hyperlink:
    ///   start_col:2 + end_col:2 + url_len:4 + url + id_len:4 + id]
    /// ```
    ///
    /// Version 0 = legacy format (no attrs)
    /// Version 1 = with RLE attrs (no hyperlinks)
    /// Version 2 = with RLE attrs + hyperlinks (no IDs)
    /// Version 3 = with RLE attrs + hyperlinks + OSC 8 IDs
    ///
    /// # Serialization Limits
    ///
    /// Due to the wire format using fixed-width integers, content exceeding these
    /// limits is silently truncated:
    ///
    /// - **Content length:** max 4 GB (`u32::MAX` bytes)
    /// - **Attribute runs:** max ~4 billion (`u32::MAX` runs)
    /// - **Hyperlinks per line:** max 65,535 (`u16::MAX` links)
    /// - **Hyperlink URL/ID length:** max 4 GB (`u32::MAX` bytes each)
    ///
    /// These limits are orders of magnitude larger than any realistic terminal line.
    /// Truncation would only occur with malformed or malicious input.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let content = self.content.as_bytes();
        let content_len = content.len();

        // Estimate capacity
        let attrs_size = self.attrs.as_ref().map_or(1, |rle| {
            1 + 4 + rle.run_count() * 14 // has_attrs + run_count + runs
        });
        let hyperlinks_size = self.hyperlinks.as_ref().map_or(2, |h| {
            2 + h.iter().map(HyperlinkSpan::serialized_size).sum::<usize>()
        });
        let mut result = Vec::with_capacity(6 + content_len + attrs_size + hyperlinks_size);

        // Version byte
        result.push(3); // Version 3 = with attrs + hyperlinks + IDs

        // Flags
        result.push(self.flags.bits());

        // Content length and content (max 4GB, see Serialization Limits)
        let content_len_u32 = u32::try_from(content_len).unwrap_or(u32::MAX);
        result.extend_from_slice(&content_len_u32.to_le_bytes());
        result.extend_from_slice(content);

        // Attributes (max ~4B runs, see Serialization Limits)
        if let Some(rle) = &self.attrs {
            result.push(1); // has_attrs = true
            let run_count = u32::try_from(rle.run_count()).unwrap_or(u32::MAX);
            result.extend_from_slice(&run_count.to_le_bytes());
            for run in rle.runs() {
                // Each run: [value:10][length:4]
                result.extend_from_slice(&run.value.serialize());
                result.extend_from_slice(&run.length.to_le_bytes());
            }
        } else {
            result.push(0); // has_attrs = false
        }

        // Hyperlinks (v3: includes OSC 8 ID, max 65535 links)
        if let Some(hyperlinks) = &self.hyperlinks {
            let count = u16::try_from(hyperlinks.len()).unwrap_or(u16::MAX);
            result.extend_from_slice(&count.to_le_bytes());
            for span in hyperlinks.iter() {
                result.extend_from_slice(&span.start_col.to_le_bytes());
                result.extend_from_slice(&span.end_col.to_le_bytes());
                let url_len = u32::try_from(span.url.len()).unwrap_or(u32::MAX);
                result.extend_from_slice(&url_len.to_le_bytes());
                result.extend_from_slice(span.url.as_bytes());
                // v3: hyperlink ID
                let id_len = span
                    .id
                    .as_ref()
                    .map_or(0u32, |id| u32::try_from(id.len()).unwrap_or(u32::MAX));
                result.extend_from_slice(&id_len.to_le_bytes());
                if let Some(id) = &span.id {
                    result.extend_from_slice(id.as_bytes());
                }
            }
        } else {
            result.extend_from_slice(&0u16.to_le_bytes()); // 0 hyperlinks
        }

        result
    }

    /// Deserialize line from bytes.
    #[must_use]
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        // Check version
        let version = data[0];
        if version == 0 {
            // Legacy format (version 0 or old format without version byte)
            return Self::deserialize_legacy(data);
        }

        if data.len() < 7 {
            return None;
        }

        // Version 1, 2, and 3 share the same base format
        let flags = LineFlags::from_bits_truncate(data[1]);
        let content_len = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;

        let content_end = 6usize.checked_add(content_len)?;
        if data.len() < content_end.checked_add(1)? {
            return None;
        }

        let content = LineContent::from_bytes(&data[6..content_end]);

        let (attrs, offset) = Self::deserialize_attrs(data, content_end)?;
        let hyperlinks = if version >= 3 {
            Self::deserialize_hyperlinks_v3(data, offset)
        } else if version >= 2 {
            Self::deserialize_hyperlinks(data, offset)
        } else {
            None
        };

        // Box the rare/absent attrs + hyperlinks fields to keep `Line` small
        // (see Line struct docs) — the deserialized values are byte-identical.
        Some(Self {
            content,
            attrs: attrs.map(Box::new),
            flags,
            hyperlinks: hyperlinks.map(Box::new),
        })
    }

    /// Deserialize RLE attributes starting at `content_end` in `data`.
    ///
    /// Returns `(attrs, next_offset)` or `None` if data is truncated.
    fn deserialize_attrs(
        data: &[u8],
        content_end: usize,
    ) -> Option<(Option<Rle<CellAttrs>>, usize)> {
        if content_end >= data.len() {
            return None;
        }
        if data[content_end] == 0 {
            return Some((None, content_end + 1));
        }

        let attrs_start = content_end + 1;
        if data.len() < attrs_start + 4 {
            return None;
        }
        let run_count = u32::from_le_bytes([
            data[attrs_start],
            data[attrs_start + 1],
            data[attrs_start + 2],
            data[attrs_start + 3],
        ]) as usize;

        // Clamp loop bound: each RLE run requires exactly 14 bytes,
        // so we can't have more runs than remaining data allows.
        let remaining = data.len().saturating_sub(attrs_start + 4);
        let max_runs = remaining / 14;
        let clamped_count = run_count.min(max_runs);

        let mut rle = Rle::new();
        let mut offset = attrs_start + 4;
        for _ in 0..clamped_count {
            if offset + 14 > data.len() {
                break;
            }
            if let Some(value) = CellAttrs::deserialize(&data[offset..]) {
                let length = u32::from_le_bytes([
                    data[offset + 10],
                    data[offset + 11],
                    data[offset + 12],
                    data[offset + 13],
                ]);
                rle.extend_with(value, length);
            }
            offset += 14;
        }
        Some((Some(rle), offset))
    }

    /// Deserialize hyperlink spans starting at `offset` in `data`.
    ///
    /// Returns `None` if there are no hyperlinks or data is truncated.
    fn deserialize_hyperlinks(data: &[u8], offset: usize) -> Option<SmallVec<HyperlinkSpan, 2>> {
        let header_end = offset.checked_add(2)?;
        if header_end > data.len() {
            return None;
        }
        let count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        if count == 0 {
            return None;
        }

        // Clamp capacity: each span requires at least 8 bytes of header,
        // so we can't have more spans than remaining data allows.
        let remaining = data.len().saturating_sub(header_end);
        let max_spans = remaining / 8;
        let mut spans = SmallVec::with_capacity(count.min(max_spans));
        let mut pos = header_end;
        for _ in 0..count {
            if pos.checked_add(8).is_none_or(|end| end > data.len()) {
                break;
            }
            let start_col = u16::from_le_bytes([data[pos], data[pos + 1]]);
            let end_col = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
            let url_len =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;
            pos += 8;
            let url_end = match pos.checked_add(url_len) {
                Some(end) if end <= data.len() => end,
                _ => break,
            };
            if let Ok(url) = std::str::from_utf8(&data[pos..url_end]) {
                spans.push(HyperlinkSpan::new(start_col, end_col, Arc::from(url)));
            }
            pos = url_end;
        }
        if spans.is_empty() { None } else { Some(spans) }
    }

    /// Deserialize v3 hyperlink spans (with OSC 8 IDs) starting at `offset`.
    ///
    /// Returns `None` if there are no hyperlinks or data is truncated.
    fn deserialize_hyperlinks_v3(data: &[u8], offset: usize) -> Option<SmallVec<HyperlinkSpan, 2>> {
        let header_end = offset.checked_add(2)?;
        if header_end > data.len() {
            return None;
        }
        let count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        if count == 0 {
            return None;
        }

        // Clamp capacity: each v3 span requires at least 12 bytes of header
        // (start_col:2 + end_col:2 + url_len:4 + id_len:4).
        let remaining = data.len().saturating_sub(header_end);
        let max_spans = remaining / 12;
        let mut spans = SmallVec::with_capacity(count.min(max_spans));
        let mut pos = header_end;
        for _ in 0..count {
            if pos.checked_add(8).is_none_or(|end| end > data.len()) {
                break;
            }
            let start_col = u16::from_le_bytes([data[pos], data[pos + 1]]);
            let end_col = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
            let url_len =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;
            pos += 8;
            let url_end = match pos.checked_add(url_len) {
                Some(end) if end <= data.len() => end,
                _ => break,
            };
            let url_bytes = &data[pos..url_end];
            pos = url_end;

            // v3: read id_len + id
            if pos.checked_add(4).is_none_or(|end| end > data.len()) {
                break;
            }
            let id_len =
                u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                    as usize;
            pos += 4;
            let id = if id_len > 0 {
                let id_end = match pos.checked_add(id_len) {
                    Some(end) if end <= data.len() => end,
                    _ => break,
                };
                let id_str = std::str::from_utf8(&data[pos..id_end]).ok();
                pos = id_end;
                id_str.map(Arc::from)
            } else {
                None
            };

            if let Ok(url) = std::str::from_utf8(url_bytes) {
                spans.push(HyperlinkSpan::with_id(
                    start_col,
                    end_col,
                    Arc::from(url),
                    id,
                ));
            }
        }
        if spans.is_empty() { None } else { Some(spans) }
    }

    /// Deserialize legacy format (without version byte or attrs).
    fn deserialize_legacy(data: &[u8]) -> Option<Self> {
        if data.len() < 5 {
            return None;
        }

        let flags = LineFlags::from_bits_truncate(data[0]);
        let len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;

        let end = 5usize.checked_add(len)?;
        if data.len() < end {
            return None;
        }

        let content = LineContent::from_bytes(&data[5..end]);
        Some(Self {
            content,
            attrs: None,
            flags,
            hyperlinks: None,
        })
    }
}

// Block-level serialization (serialize_lines, deserialize_lines) is in line_codec_block.rs.
#[cfg(not(any(fuzzing, feature = "fuzz")))]
pub(crate) use super::line_codec_block::{deserialize_lines, serialize_lines};
#[cfg(any(fuzzing, feature = "fuzz"))]
pub use super::line_codec_block::{deserialize_lines, serialize_lines};
