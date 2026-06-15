// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Small-string-optimized content storage for scrollback lines.

use super::INLINE_SIZE;
use aterm_alloc::SmallVec;

/// Line content storage.
///
/// Uses small-string optimization: lines up to 128 bytes are stored inline,
/// longer lines use heap allocation.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Inline variant is intentionally large — that's the point of SmallVec inline storage
pub(crate) enum LineContent {
    /// Inline storage for short lines.
    Inline(SmallVec<u8, INLINE_SIZE>),
    /// Heap storage for long lines.
    Heap(Vec<u8>),
}

impl Default for LineContent {
    fn default() -> Self {
        Self::Inline(SmallVec::new())
    }
}

impl LineContent {
    /// Create from bytes.
    #[must_use]
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() <= INLINE_SIZE {
            let mut sv = SmallVec::new();
            sv.extend_from_slice(bytes);
            Self::Inline(sv)
        } else {
            Self::Heap(bytes.to_vec())
        }
    }

    /// Get as byte slice.
    #[must_use]
    #[inline]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Inline(sv) => sv.as_slice(),
            Self::Heap(v) => v.as_slice(),
        }
    }

    /// Get the length in bytes.
    #[must_use]
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Inline(sv) => sv.len(),
            Self::Heap(v) => v.len(),
        }
    }

    /// Check if empty.
    #[must_use]
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
