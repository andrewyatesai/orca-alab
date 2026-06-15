// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Line representation for scrollback storage.
//!
//! Lines can be stored in different formats depending on tier:
//! - Hot: Full Line with content + RLE-compressed attributes
//! - Warm/Cold: Serialized bytes (compressed)
//!
//! ## RLE Attribute Compression
//!
//! Terminal lines often have runs of cells with identical attributes (e.g.,
//! a prompt in one color, then text in another). RLE compression stores
//! `(style, count)` pairs instead of per-cell styles.
//!
//! Typical compression: 80-column line with 3 color regions → 3 runs vs 80 cells.

use aterm_alloc::SmallVec;
use aterm_rle::Rle;
use std::sync::Arc;

/// Maximum inline storage for line content (avoids heap allocation for short lines).
///
/// Tuned down from 128 (perf-memory): the hot tier stores `Line` structs
/// contiguously in a `VecDeque`, so the inline buffer is paid on *every* stored
/// line regardless of its actual length. 128 bytes inline made `Line` 304 bytes
/// even for a 5-char prompt. A 32-byte inline buffer keeps short prompts/words
/// allocation-free while shrinking the stored `Line` struct dramatically; longer
/// lines spill to a right-sized heap `Vec` (one allocation of exactly `len`).
/// This is a pure storage-location change — `as_bytes()`/`len()` and
/// serialization return byte-identical results either way.
const INLINE_SIZE: usize = 32;

// ============================================================================
// Cell Attributes for RLE Compression
// ============================================================================

/// Compressed cell attributes for RLE storage.
///
/// This is a compact representation of cell styling that can be efficiently
/// RLE-encoded. It captures the essential visual attributes:
/// - Foreground color (packed)
/// - Background color (packed)
/// - Cell flags (bold, italic, underline, etc.)
///
/// ## Memory Layout
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────┐
/// │ fg: u32 (4 bytes) - Packed foreground color                 │
/// │   Format: 0xTT_RRGGBB where TT = type (default/indexed/rgb) │
/// ├─────────────────────────────────────────────────────────────┤
/// │ bg: u32 (4 bytes) - Packed background color                 │
/// │   Format: 0xTT_RRGGBB where TT = type (default/indexed/rgb) │
/// ├─────────────────────────────────────────────────────────────┤
/// │ flags: u16 (2 bytes) - Visual attribute flags               │
/// │   Bits 0-7: bold, dim, italic, underline, blink, inverse... │
/// └─────────────────────────────────────────────────────────────┘
/// Total: 10 bytes per unique style (vs 8 bytes per cell uncompressed)
/// ```
///
/// ## Compression Benefit
///
/// An 80-column line with plain text: 80 cells × 8 bytes = 640 bytes
/// With RLE (1 style run): ~15 bytes (10 bytes style + 5 bytes overhead)
///
/// An 80-column prompt line with 3 color regions:
/// - Uncompressed: 640 bytes
/// - RLE: ~45 bytes (3 runs × 10 bytes + overhead)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttrs {
    /// Packed foreground color.
    /// Format: 0xTT_RRGGBB where TT indicates type:
    /// - 0x00: Indexed color (RRGGBB = 0x00_00_XX where XX is index)
    /// - 0x01: True color RGB
    /// - 0xFF: Default color
    pub fg: u32,
    /// Packed background color (same format as fg).
    pub bg: u32,
    /// Cell flags (bold, italic, underline, etc.).
    /// Excludes WIDE/WIDE_CONTINUATION/COMPLEX flags.
    pub flags: u16,
}

/// Default fg color (0xFF_FFFFFF - default type marker + white placeholder).
const DEFAULT_FG: u32 = 0xFF_FF_FF_FF;
/// Default bg color (0xFF_000000 - default type marker + black placeholder).
const DEFAULT_BG: u32 = 0xFF_00_00_00;

impl CellAttrs {
    /// Default cell attributes (default colors, no flags).
    pub const DEFAULT: Self = Self {
        fg: DEFAULT_FG,
        bg: DEFAULT_BG,
        flags: 0,
    };

    /// Create new cell attributes.
    #[must_use]
    pub const fn new(fg: u32, bg: u32, flags: u16) -> Self {
        Self { fg, bg, flags }
    }

    /// Check if these are default attributes.
    #[must_use]
    #[inline]
    pub const fn is_default(&self) -> bool {
        self.fg == DEFAULT_FG && self.bg == DEFAULT_BG && self.flags == 0
    }

    /// Mask for visual flags we care about in scrollback.
    /// Excludes WIDE (bit 9), WIDE_CONTINUATION/PROTECTED (bit 10),
    /// USES_STYLE_ID (bit 14), and COMPLEX (bit 15) which are cell-specific.
    /// Includes bits 0-8 (bold through double_underline) plus
    /// bits 11-13 (superscript, subscript, curly_underline).
    const VISUAL_FLAGS_MASK: u16 = 0x39FF; // bits 0-8, 11-13

    /// Create from raw cell values, filtering to visual-only flags.
    #[must_use]
    #[inline]
    pub const fn from_raw(fg: u32, bg: u32, flags: u16) -> Self {
        Self {
            fg,
            bg,
            flags: flags & Self::VISUAL_FLAGS_MASK,
        }
    }
}

#[path = "line_codec.rs"]
mod line_codec;
#[path = "line_codec_block.rs"]
mod line_codec_block;
#[cfg(not(any(fuzzing, feature = "fuzz")))]
pub(crate) use line_codec::{deserialize_lines, serialize_lines};
#[cfg(any(fuzzing, feature = "fuzz"))]
pub use line_codec::{deserialize_lines, serialize_lines};

#[path = "line_content.rs"]
mod line_content;
pub(crate) use line_content::LineContent;

#[path = "hyperlink_span.rs"]
mod hyperlink_span;
pub use hyperlink_span::HyperlinkSpan;

/// A scrollback line.
///
/// Contains the text content, RLE-compressed attributes, and metadata.
///
/// ## Attribute Compression
///
/// When lines scroll off the visible grid into scrollback, we preserve their
/// styling via RLE compression. This stores runs of identical attributes
/// instead of per-cell data.
///
/// Example: A line with "Hello " (green) + "World" (default):
/// - Text: "Hello World" (11 bytes)
/// - Attrs: [(green, 6), (default, 5)] (~24 bytes for 2 runs)
/// - vs uncompressed: 11 cells × 8 bytes = 88 bytes
#[derive(Debug, Clone, Default)]
pub struct Line {
    /// Line content (UTF-8 text).
    content: LineContent,
    /// RLE-compressed cell attributes (colors and flags per character).
    /// None if all cells have default attributes (optimization for plain text).
    ///
    /// Boxed: plain-text lines have no attrs, so the common path pays an 8-byte
    /// niche pointer instead of carrying the 56-byte `Rle` inline. Styled lines
    /// (the rarer case) pay one extra heap allocation. The hot tier stores
    /// `Line` structs in a `VecDeque`, so shrinking the struct directly reduces
    /// resident scrollback memory.
    attrs: Option<Box<Rle<CellAttrs>>>,
    /// Line flags.
    flags: LineFlags,
    /// Hyperlink spans (typically None or 1-3 spans per line).
    ///
    /// Boxed: most lines have no hyperlinks, so the common path pays an 8-byte
    /// niche pointer instead of an 88-byte inline `SmallVec`. When present, the
    /// boxed `SmallVec<HyperlinkSpan, 2>` still keeps ≤2 spans inline.
    hyperlinks: Option<Box<SmallVec<HyperlinkSpan, 2>>>,
}

aterm_types::bitflags! {
    /// Line flags for metadata.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct LineFlags: u8 {
        /// Line is wrapped (continuation of previous line).
        const WRAPPED = 1 << 0;
        /// Line contains search match.
        const HAS_MATCH = 1 << 1;
        /// Line has been modified.
        const DIRTY = 1 << 2;
    }
}

impl Line {
    /// Create a new empty line.
    ///
    /// ENSURES: self.is_empty()
    /// ENSURES: !self.has_attrs()
    /// ENSURES: !self.has_hyperlinks()
    /// ENSURES: !self.is_wrapped()
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a line from bytes (no attributes).
    ///
    /// ENSURES: self.len() == bytes.len()
    /// ENSURES: !self.has_attrs()
    /// ENSURES: !self.has_hyperlinks()
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            content: LineContent::from_bytes(bytes),
            attrs: None,
            flags: LineFlags::empty(),
            hyperlinks: None,
        }
    }

    /// Create a line with text and RLE-compressed attributes.
    ///
    /// This is the primary constructor when converting from grid Row to scrollback Line.
    /// The attrs RLE should have the same length as the character count in text.
    #[must_use]
    pub fn with_attrs(text: &str, attrs: Rle<CellAttrs>) -> Self {
        // Optimization: if empty or all attrs are default, don't store them
        let is_all_default = attrs.run_count() == 0
            || (attrs.run_count() == 1
                && attrs.runs().first().is_some_and(|r| r.value.is_default()));

        let attrs = if is_all_default {
            None
        } else {
            Some(Box::new(attrs))
        };

        Self {
            content: LineContent::from_bytes(text.as_bytes()),
            attrs,
            flags: LineFlags::empty(),
            hyperlinks: None,
        }
    }

    /// Create a line with text, attributes, and hyperlinks.
    ///
    /// This is the full constructor for preserving hyperlinks from the visible grid
    /// when lines scroll into scrollback.
    #[must_use]
    pub fn with_hyperlinks(
        text: &str,
        attrs: Rle<CellAttrs>,
        hyperlinks: Vec<HyperlinkSpan>,
    ) -> Self {
        // Optimization: if empty or all attrs are default, don't store them
        let is_all_default = attrs.run_count() == 0
            || (attrs.run_count() == 1
                && attrs.runs().first().is_some_and(|r| r.value.is_default()));

        let attrs = if is_all_default {
            None
        } else {
            Some(Box::new(attrs))
        };

        // Optimization: if no hyperlinks, don't allocate
        let hyperlinks = if hyperlinks.is_empty() {
            None
        } else {
            Some(Box::new(SmallVec::from_vec(hyperlinks)))
        };

        Self {
            content: LineContent::from_bytes(text.as_bytes()),
            attrs,
            flags: LineFlags::empty(),
            hyperlinks,
        }
    }

    /// Get the RLE-compressed attributes, if any.
    #[must_use]
    #[inline]
    pub fn attrs(&self) -> Option<&Rle<CellAttrs>> {
        self.attrs.as_deref()
    }

    /// Get the attribute for a specific character index.
    ///
    /// Returns default attributes if the line has no stored attributes
    /// or if the index is out of bounds.
    ///
    /// ENSURES: !self.has_attrs() implies result == CellAttrs::DEFAULT
    #[must_use]
    pub fn get_attr(&self, char_idx: usize) -> CellAttrs {
        match &self.attrs {
            Some(rle) => {
                let idx = u32::try_from(char_idx).unwrap_or(u32::MAX);
                rle.get(idx).unwrap_or(CellAttrs::DEFAULT)
            }
            None => CellAttrs::DEFAULT,
        }
    }

    /// Check if this line has styled content (non-default attributes).
    #[must_use]
    #[inline]
    pub fn has_attrs(&self) -> bool {
        self.attrs.is_some()
    }

    /// Get hyperlink URL at column, if any.
    ///
    /// Returns the URL of the hyperlink at the given column position,
    /// or None if no hyperlink exists at that column.
    #[must_use]
    pub fn get_hyperlink(&self, col: u16) -> Option<&Arc<str>> {
        self.hyperlinks
            .as_ref()?
            .iter()
            .find(|span| span.contains(col))
            .map(|span| &span.url)
    }

    /// Get the full hyperlink span at column, if any.
    ///
    /// Returns the span (including URL and OSC 8 ID) at the given column,
    /// or None if no hyperlink exists at that column.
    #[must_use]
    pub fn get_hyperlink_span(&self, col: u16) -> Option<&HyperlinkSpan> {
        self.hyperlinks
            .as_ref()?
            .iter()
            .find(|span| span.contains(col))
    }

    /// Check if this line has any hyperlinks.
    #[must_use]
    #[inline]
    pub fn has_hyperlinks(&self) -> bool {
        self.hyperlinks.as_ref().is_some_and(|h| !h.is_empty())
    }

    /// Get the number of hyperlink spans.
    #[must_use]
    #[inline]
    pub fn hyperlink_count(&self) -> usize {
        self.hyperlinks.as_ref().map_or(0, |h| h.len())
    }

    /// Get the hyperlink spans.
    #[must_use]
    #[inline]
    pub fn hyperlinks(&self) -> Option<&[HyperlinkSpan]> {
        self.hyperlinks.as_deref().map(SmallVec::as_slice)
    }

    /// Get the content as bytes.
    #[must_use]
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.content.as_bytes()
    }

    /// Get the length in bytes.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Check if empty.
    ///
    /// ENSURES: result == (self.len() == 0)
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Check if wrapped.
    #[must_use]
    #[inline]
    pub fn is_wrapped(&self) -> bool {
        self.flags.contains(LineFlags::WRAPPED)
    }

    /// Set wrapped flag.
    ///
    /// ENSURES: self.is_wrapped() == wrapped
    #[inline]
    pub fn set_wrapped(&mut self, wrapped: bool) {
        if wrapped {
            self.flags |= LineFlags::WRAPPED;
        } else {
            self.flags -= LineFlags::WRAPPED;
        }
    }

    /// Get content as a string slice (returns None if not valid UTF-8).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(self.as_bytes()).ok()
    }

    /// Calculate memory used by this line.
    #[must_use]
    pub fn memory_used(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let content_mem = match &self.content {
            LineContent::Inline(_) => 0, // Already counted in size_of
            LineContent::Heap(v) => v.capacity(),
        };
        let attrs_mem = self.attrs.as_ref().map_or(0, |rle| {
            // Boxed: the Rle struct now lives on the heap (no longer counted in
            // size_of::<Line>), plus the runs Vec it owns.
            std::mem::size_of::<Rle<CellAttrs>>()
                + rle.run_count() * std::mem::size_of::<aterm_rle::Run<CellAttrs>>()
        });
        let hyperlinks_mem = self.hyperlinks.as_ref().map_or(0, |spans| {
            // Boxed: the SmallVec now lives on the heap (no longer counted in
            // size_of::<Line>). We additionally count:
            // - Spilled heap allocation (if > 2 spans)
            // - Arc<str> heap allocations for URLs
            let boxed = std::mem::size_of::<SmallVec<HyperlinkSpan, 2>>();
            let heap_spans = if spans.len() > 2 {
                spans.len() * std::mem::size_of::<HyperlinkSpan>()
            } else {
                0
            };
            let url_mem: usize = spans.iter().map(|s| s.url.len()).sum();
            let id_mem: usize = spans
                .iter()
                .filter_map(|s| s.id.as_ref())
                .map(|id| id.len())
                .sum();
            boxed + heap_spans + url_mem + id_mem
        });
        base + content_mem + attrs_mem + hyperlinks_mem
    }

    /// Calculate the number of attribute runs (for compression stats).
    #[must_use]
    pub fn attr_run_count(&self) -> usize {
        self.attrs.as_ref().map_or(0, |rle| rle.run_count())
    }
}

impl From<&str> for Line {
    fn from(s: &str) -> Self {
        Self {
            content: LineContent::from_bytes(s.as_bytes()),
            attrs: None,
            flags: LineFlags::empty(),
            hyperlinks: None,
        }
    }
}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.as_bytes()))
    }
}

#[cfg(test)]
#[path = "line_tests.rs"]
mod tests;
