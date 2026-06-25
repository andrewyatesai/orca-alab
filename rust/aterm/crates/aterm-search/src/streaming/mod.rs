// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

//! Streaming search with memory-bounded results.
//!
//! ## Design (from TLA+ spec: StreamingSearch.tla)
//!
//! This module implements a streaming search system that:
//! - Searches through content incrementally (row by row)
//! - Bounds memory usage with configurable result limits
//! - Supports multiple filter modes: Literal, Regex, Fuzzy
//! - Provides navigation with optional wraparound
//! - Handles dynamic content changes (additions/invalidations)
//!
//! ## Safety Invariants (from TLA+ specification)
//!
//! | ID | Invariant | Description |
//! |----|-----------|-------------|
//! | INV-SEARCH-1 | `CurrentIndexValid` | Current match index always valid |
//! | INV-SEARCH-2 | `ResultPositionsValid` | All result positions are valid grid coords |
//! | INV-SEARCH-3 | `MemoryBounded` | Result count never exceeds MaxResults |
//! | INV-SEARCH-4 | `NoDuplicateResults` | No duplicate results in result set |
//! | INV-SEARCH-5 | `ScanProgressConsistent` | Scan progress consistent with state |
//! | INV-SEARCH-6 | `TotalMatchesConsistent` | Total matches >= stored results |
//!
//! ## State Machine
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                                                             │
//! │  ┌──────┐  StartSearch   ┌───────────┐  ScanComplete       │
//! │  │ Idle │ ─────────────▶ │ Searching │ ──────────────┐     │
//! │  └──────┘                └───────────┘               │     │
//! │      ▲                        │                      ▼     │
//! │      │     Cancel             │ ScanComplete    ┌─────────┐│
//! │      ├────────────────────────┤                 │HasResult││
//! │      │                        │                 └─────────┘│
//! │      │     Cancel             ▼                      │     │
//! │      ├───────────────── ┌───────────┐               │     │
//! │      │                  │ NoResults │ ◀─────────────┘     │
//! │      │                  └───────────┘  (results empty)    │
//! │      │                        │                            │
//! │      └────────────────────────┘                            │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod content;
mod engine;
mod error;
mod types;

pub use content::SearchContent;
pub use engine::StreamingSearch;
pub use error::{SearchError, SearchResult};
pub use types::{FilterMode, SearchState, StreamingMatch, StreamingSearchConfig};

#[cfg(test)]
mod test_content {
    use super::SearchContent;

    /// Simple content provider shared by streaming search tests.
    pub(super) struct TestContent {
        lines: Vec<String>,
    }

    impl TestContent {
        pub(super) fn new(lines: Vec<&str>) -> Self {
            Self {
                lines: lines.into_iter().map(String::from).collect(),
            }
        }
    }

    impl SearchContent for TestContent {
        fn row_count(&self) -> usize {
            self.lines.len()
        }

        fn get_row_text(&mut self, row: usize) -> Option<String> {
            self.lines.get(row).cloned()
        }
    }

    /// Content provider with configurable wrapped-row flags for testing
    /// wrapped-line search coordinate remapping (#7572).
    pub(super) struct WrappedTestContent {
        lines: Vec<String>,
        /// Which rows are continuations of the previous row (soft wrap).
        wrapped: Vec<bool>,
    }

    impl WrappedTestContent {
        /// Create content where `wrapped[i]` indicates row `i` is a continuation.
        pub(super) fn new(lines: Vec<&str>, wrapped: Vec<bool>) -> Self {
            assert_eq!(lines.len(), wrapped.len());
            Self {
                lines: lines.into_iter().map(String::from).collect(),
                wrapped,
            }
        }
    }

    impl SearchContent for WrappedTestContent {
        fn row_count(&self) -> usize {
            self.lines.len()
        }

        fn get_row_text(&mut self, row: usize) -> Option<String> {
            self.lines.get(row).cloned()
        }

        fn is_row_wrapped(&self, row: usize) -> bool {
            self.wrapped.get(row).copied().unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "regex"))]
mod regex_tests;

#[cfg(kani)]
mod proofs;

#[cfg(kani)]
mod proofs_gaps;
