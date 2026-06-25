// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! **aterm-observe** — layer L0.5 of the RFC "The Reactive Surface": the
//! *semantic* surface predicates, built on the core
//! [Observation Kernel](aterm_core::terminal::WatcherSet).
//!
//! The point of this crate is a dependency boundary. The kernel in `aterm-core`
//! exposes a primitive that latches when a *content_seq* advances, a quiescence
//! deadline elapses, or an opaque [`RowMatch`](aterm_core::terminal::RowMatch)
//! fires — but it carries **no vocabulary**: it cannot turn a pattern string into
//! a matcher. That vocabulary (`regex`) lives **here**, so `aterm-core` takes no
//! **direct** `regex` dependency (RFC requirement R2; enforced by
//! [`tests::regex_is_not_in_aterm_core_production_deps`], which checks the core's
//! direct production deps — `regex` does still appear in the workspace's
//! transitive closure via `aterm-search`'s `regex` feature). The agent layer
//! (`aterm-agent`, L2) composes these predicates into turn-completion; it never
//! reaches into the core enum directly.

use std::sync::Arc;
use std::time::Duration;

use aterm_core::terminal::{RowMatch, RowRange, WatcherSpec};

/// Re-export the regex compile error so dependents (e.g. `aterm-agent`) can name
/// it without taking a direct `regex` dependency — the regex boundary stays at
/// this crate.
pub mod regex_compile_error {
    pub use regex::Error;
}

/// A pre-compiled regular-expression row matcher — the one place `regex` is used
/// in the watcher stack. The core stores it behind `dyn RowMatch` and can only
/// *evaluate* it, never construct it.
#[derive(Debug)]
pub struct RegexRowMatch {
    re: regex::Regex,
}

impl RowMatch for RegexRowMatch {
    #[inline]
    fn matches(&self, row: &str) -> bool {
        self.re.is_match(row)
    }
}

/// Compile a regex row matcher. The returned `Arc<dyn RowMatch>` is what
/// [`Terminal::watch_rows`](aterm_core::terminal::Terminal::watch_rows) takes —
/// the core receives only the opaque handle.
///
/// # Errors
/// Returns the [`regex::Error`] if `pattern` does not compile.
pub fn row_matcher(pattern: &str) -> Result<Arc<dyn RowMatch>, regex::Error> {
    Ok(Arc::new(RegexRowMatch {
        re: regex::Regex::new(pattern)?,
    }))
}

/// `IdleFor(dur)` — latch after `dur` of no content mutation (quiescence).
#[must_use]
pub fn idle_for(dur: Duration) -> WatcherSpec {
    WatcherSpec::IdleFor { dur }
}

/// `SeqAdvanced(after)` — latch once the content clock passes `after`.
#[must_use]
pub fn seq_advanced(after: u64) -> WatcherSpec {
    WatcherSpec::SeqAdvanced { after }
}

/// `BlockComplete` — latch on a completed/prompt-ready shell-integration block.
#[must_use]
pub fn block_complete() -> WatcherSpec {
    WatcherSpec::BlockComplete
}

/// The whole visible surface (every row) — the common [`RowRange`] for row
/// matching.
#[must_use]
pub fn anywhere() -> RowRange {
    RowRange::All
}

/// The inclusive visible-row span `start..=end`.
#[must_use]
pub fn rows(start: usize, end: usize) -> RowRange {
    RowRange::Span { start, end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_core::terminal::{ClockReading, Terminal};
    use std::time::Instant;

    fn clock_at(base: Instant, off_ms: u64) -> ClockReading {
        ClockReading {
            monotonic: base + Duration::from_millis(off_ms),
            wall_ms: Some(off_ms),
        }
    }

    #[test]
    fn row_matcher_latches_on_real_engine_output() {
        // Bind aterm-observe -> aterm-core -> real engine: arm a regex row
        // matcher, paint a matching row through the real pipeline, assert latch.
        let base = Instant::now();
        let mut t = Terminal::new(24, 80);
        let m = row_matcher(r"PROMPT-READY").expect("compile");
        let id = t.watch_rows(m, anywhere(), base).expect("arm");
        assert!(t.watch_poll(id).is_none(), "pending before the row appears");

        t.process_at(b"working...\r\n", clock_at(base, 10));
        assert!(
            t.watch_poll(id).is_none(),
            "non-matching output does not latch"
        );

        t.process_at(b"PROMPT-READY\r\n", clock_at(base, 20));
        let sat = t
            .watch_poll(id)
            .expect("matching row latched on the real surface");
        assert!(sat.seq > 0);
    }

    #[test]
    fn row_matcher_latches_immediately_if_already_matching() {
        // Arm against a surface that ALREADY shows the row — must latch at arm
        // (the `watch_rows` immediate eval), not wait for the next change.
        let base = Instant::now();
        let mut t = Terminal::new(24, 80);
        t.process_at(b"ALREADY-HERE\r\n", clock_at(base, 5));
        let m = row_matcher("ALREADY-HERE").expect("compile");
        let id = t.watch_rows(m, anywhere(), base).expect("arm");
        assert!(
            t.watch_poll(id).is_some(),
            "an already-matching row latches at arm time"
        );
    }

    #[test]
    fn bad_pattern_is_a_clean_error_not_a_panic() {
        assert!(row_matcher(r"(unclosed").is_err());
    }

    /// RFC R2 purity, made a checkable invariant: `regex` must NOT appear in
    /// `aterm-core`'s **production** `[dependencies]` — only its
    /// `[dev-dependencies]`. The kernel stays vocabulary-free; the regex lives
    /// here. If someone adds `regex` to the core's production deps, this fails.
    #[test]
    fn regex_is_not_in_aterm_core_production_deps() {
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../aterm-core/Cargo.toml");
        let toml = std::fs::read_to_string(manifest).expect("read aterm-core Cargo.toml");
        // Slice the production [dependencies] section (up to the next table header).
        let deps = toml
            .split_once("\n[dependencies]\n")
            .map(|(_, rest)| rest.split("\n[").next().unwrap_or(rest))
            .unwrap_or("");
        assert!(
            !deps.lines().any(|l| l.trim_start().starts_with("regex")),
            "regex leaked into aterm-core's PRODUCTION dependencies — it must stay \
             in aterm-observe (RFC R2 purity). Production [dependencies]:\n{deps}"
        );
    }
}
