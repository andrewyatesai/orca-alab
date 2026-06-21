// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

use super::WarmBlock;
use crate::line::{Line, deserialize_lines};
use crate::{ScrollbackError, decompress_lz4_bounded};

impl WarmBlock {
    fn stored_line_count(serialized: &[u8]) -> Result<usize, ScrollbackError> {
        if serialized.len() < 4 {
            return Err(ScrollbackError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "warm block serialized payload too short for count header",
            )));
        }
        Ok(u32::from_le_bytes(serialized[..4].try_into().expect("count header")) as usize)
    }

    fn logical_suffix(
        &self,
        serialized: &[u8],
        stored_line_count: usize,
    ) -> Result<Vec<Line>, ScrollbackError> {
        if stored_line_count < self.line_count {
            return Err(ScrollbackError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "warm block serialized {} lines but metadata claims {}",
                    stored_line_count, self.line_count
                ),
            )));
        }

        let mut lines = deserialize_lines(serialized);
        if lines.len() != stored_line_count {
            return Err(ScrollbackError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "warm block serialized {} lines but decoded {} complete lines",
                    stored_line_count,
                    lines.len()
                ),
            )));
        }

        let trimmed = stored_line_count.saturating_sub(self.line_count);
        if trimmed > 0 {
            // After a corrupt front-offset materialization we preserve the
            // surviving logical suffix by shrinking line_count. If the block
            // later decodes again, drop the consumed prefix lazily here.
            lines = lines.split_off(trimmed);
        }
        self.decompress_failures.set(0);
        Ok(lines)
    }

    /// Decompress and get all lines.
    ///
    /// Increments the failure counter on any error so that read-path callers
    /// (`get_line`, iterators, `to_cold_compressed`) advance a corrupt block
    /// toward quarantine. The success path resets the counter in
    /// `logical_suffix`.
    pub(crate) fn decompress(&self) -> Result<Vec<Line>, ScrollbackError> {
        let result = self.try_decompress();
        if result.is_err() {
            let failures = self.decompress_failures.get().saturating_add(1);
            self.decompress_failures.set(failures);
        }
        result
    }

    fn try_decompress(&self) -> Result<Vec<Line>, ScrollbackError> {
        let decompressed = decompress_lz4_bounded(&self.compressed)?;
        let stored_line_count = Self::stored_line_count(&decompressed)?;
        self.logical_suffix(&decompressed, stored_line_count)
    }
}
