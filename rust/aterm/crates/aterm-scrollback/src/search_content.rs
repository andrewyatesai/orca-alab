// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Search-content adapters for scrollback types.

use std::borrow::Cow;

#[cfg(feature = "disk-tier")]
use crate::DiskBackedScrollback;
use crate::{Scrollback, ScrollbackStorage};
use aterm_types::SearchContent;

fn read_search_row_text(
    source: &str,
    row: usize,
    line: Result<Option<Cow<'_, crate::Line>>, crate::ScrollbackError>,
) -> Option<String> {
    match line {
        Ok(Some(line)) => Some(line.to_string()),
        Ok(None) => None,
        Err(error) => {
            aterm_log::warn!("{source}::get_row_text({row}) failed: {error}");
            None
        }
    }
}

fn read_is_row_wrapped(line: Result<Option<Cow<'_, crate::Line>>, crate::ScrollbackError>) -> bool {
    matches!(line, Ok(Some(l)) if l.is_wrapped())
}

impl SearchContent for Scrollback {
    fn row_count(&self) -> usize {
        self.line_count()
    }

    fn get_row_text(&mut self, row: usize) -> Option<String> {
        read_search_row_text("Scrollback", row, self.get_line(row))
    }

    fn is_row_wrapped(&self, row: usize) -> bool {
        read_is_row_wrapped(self.get_line(row))
    }
}

impl SearchContent for ScrollbackStorage {
    fn row_count(&self) -> usize {
        self.line_count()
    }

    fn get_row_text(&mut self, row: usize) -> Option<String> {
        read_search_row_text("ScrollbackStorage", row, self.get_line(row))
    }

    fn is_row_wrapped(&self, row: usize) -> bool {
        read_is_row_wrapped(self.get_line(row))
    }
}

#[cfg(feature = "disk-tier")]
impl SearchContent for DiskBackedScrollback {
    fn row_count(&self) -> usize {
        self.line_count()
    }

    fn get_row_text(&mut self, row: usize) -> Option<String> {
        read_search_row_text("DiskBackedScrollback", row, self.get_line(row))
    }

    fn is_row_wrapped(&self, row: usize) -> bool {
        read_is_row_wrapped(self.get_line(row))
    }
}
