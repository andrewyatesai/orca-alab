//! Node-API addon exposing the ATERM-backed `orca_terminal::HeadlessTerminal`
//! to the Electron main/daemon process. Mirrors the surface
//! `src/main/daemon/headless-emulator.ts` needs (write / resize / snapshot /
//! cwd / cursor / mouse-modes / serialize) so it can be swapped in behind the
//! `ORCA_RUST_TERMINAL` flag. This is the real JS -> napi -> aterm path.
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

const DEFAULT_SCROLLBACK: u32 = 5000;

/// One OSC-8 hyperlink run in a snapshot. Field names marshal to camelCase
/// (`startCol`/`endCol`), matching the renderer's `TerminalOscLinkRange`.
/// `endCol` is exclusive.
#[napi(object)]
pub struct JsOscLinkRange {
    pub row: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub uri: String,
}

#[napi(js_name = "HeadlessTerminal")]
pub struct JsHeadlessTerminal {
    // Option so dispose() can drop the engine (grid + tiered scrollback)
    // deterministically instead of waiting for the GC finalizer; disposed
    // calls return empty defaults.
    inner: Option<orca_terminal::HeadlessTerminal>,
}

// Every export carries catch_unwind: a Rust panic unwinding across the extern-C
// napi boundary aborts the whole daemon/Electron-main process (all sessions);
// catch_unwind converts it into a JS exception the caller can contain per-session.
#[napi]
impl JsHeadlessTerminal {
    /// JS passes (cols, rows); the engine takes (rows, cols) internally.
    #[napi(constructor, catch_unwind)]
    pub fn new(cols: u32, rows: u32, scrollback: Option<u32>) -> Self {
        let scrollback = scrollback.unwrap_or(DEFAULT_SCROLLBACK) as usize;
        Self {
            inner: Some(orca_terminal::HeadlessTerminal::with_scrollback(
                rows as usize,
                cols as usize,
                scrollback,
            )),
        }
    }

    #[napi(catch_unwind)]
    pub fn write(&mut self, data: Buffer) {
        if let Some(inner) = self.inner.as_mut() {
            inner.process(&data);
        }
    }

    #[napi(catch_unwind)]
    pub fn resize(&mut self, cols: u32, rows: u32) {
        if let Some(inner) = self.inner.as_mut() {
            inner.resize(rows as usize, cols as usize);
        }
    }

    /// Visible grid rows (trailing blanks trimmed) — the render snapshot.
    #[napi(catch_unwind)]
    pub fn snapshot(&self) -> Vec<String> {
        self.inner.as_ref().map(|t| t.snapshot()).unwrap_or_default()
    }

    #[napi(catch_unwind)]
    pub fn scrollback_len(&self) -> u32 {
        self.inner.as_ref().map_or(0, |t| t.scrollback_len() as u32)
    }

    #[napi(catch_unwind)]
    pub fn clear_scrollback(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.clear_scrollback();
        }
    }

    /// Replayable ANSI for the snapshot (scrollback + visible grid). `&mut` so
    /// the adapter can memoise the result by content-generation + cursor.
    /// `scrollbackRows` caps the prepended history (omit = all, 0 = viewport-only),
    /// matching `@xterm/addon-serialize`'s `serialize({scrollback})`.
    #[napi(catch_unwind)]
    pub fn serialize_ansi(&mut self, scrollback_rows: Option<u32>) -> String {
        self.inner
            .as_mut()
            .map(|t| t.serialize_ansi(scrollback_rows.map(|n| n as usize)))
            .unwrap_or_default()
    }

    /// Scrollback history only (no grid/cursor framing) — what the daemon stores
    /// in `scrollbackAnsi` so alt-screen sessions restore their scrollback.
    /// `maxRows` caps to the most-recent N history lines (omit = all).
    #[napi(catch_unwind)]
    pub fn serialize_scrollback_ansi(&self, max_rows: Option<u32>) -> String {
        self.inner
            .as_ref()
            .map(|t| t.serialize_scrollback_ansi(max_rows.map(|n| n as usize)))
            .unwrap_or_default()
    }

    /// OSC-8 hyperlink ranges over the serialized window (the same `scrollbackRows`
    /// of history `serializeAnsi` prepends, then the visible grid), so restored
    /// snapshots keep clickable links.
    #[napi(catch_unwind)]
    pub fn osc_link_ranges(&self, scrollback_rows: Option<u32>) -> Vec<JsOscLinkRange> {
        let Some(inner) = self.inner.as_ref() else {
            return Vec::new();
        };
        inner
            .osc_link_ranges(scrollback_rows.map(|n| n as usize))
            .into_iter()
            .map(|r| JsOscLinkRange {
                row: r.row as u32,
                start_col: r.start_col as u32,
                end_col: r.end_col as u32,
                uri: r.uri,
            })
            .collect()
    }

    /// Window title (OSC 0/2), or null when unset — feeds the snapshot's
    /// `lastTitle` for agent detection.
    #[napi(catch_unwind)]
    pub fn title(&self) -> Option<String> {
        self.inner.as_ref().and_then(|t| t.title())
    }

    #[napi(catch_unwind)]
    pub fn cwd(&self) -> Option<String> {
        self.inner
            .as_ref()
            .and_then(|t| t.cwd().map(str::to_string))
    }

    /// `[row, col]` cursor position.
    #[napi(catch_unwind)]
    pub fn cursor(&self) -> Vec<u32> {
        let (r, c) = self.inner.as_ref().map_or((0, 0), |t| t.cursor());
        vec![r as u32, c as u32]
    }

    #[napi(catch_unwind)]
    pub fn mouse_tracking(&self) -> String {
        use orca_terminal::MouseTracking::{Any, Button, Normal, None as MtNone, X10};
        // Capitalised variant names — the daemon factory's RUST_MOUSE_MODE map
        // keys on these (None/X10/Normal/Button/Any).
        match self.inner.as_ref().map(|t| t.mouse_tracking()) {
            None | Some(MtNone) => "None",
            Some(X10) => "X10",
            Some(Normal) => "Normal",
            Some(Button) => "Button",
            Some(Any) => "Any",
        }
        .to_string()
    }

    #[napi(catch_unwind)]
    pub fn sgr_mouse(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.sgr_mouse())
    }

    #[napi(catch_unwind)]
    pub fn sgr_pixels(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.sgr_pixels())
    }

    #[napi(catch_unwind)]
    pub fn is_alternate_screen(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.is_alternate_screen())
    }

    #[napi(catch_unwind)]
    pub fn bracketed_paste(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.bracketed_paste())
    }

    #[napi(catch_unwind)]
    pub fn application_cursor(&self) -> bool {
        self.inner.as_ref().is_some_and(|t| t.application_cursor())
    }

    /// Drop the engine now. The daemon churns through many sessions, so freeing
    /// the multi-MB grid/scrollback must not wait for a GC finalizer.
    #[napi(catch_unwind)]
    pub fn dispose(&mut self) {
        self.inner = None;
    }
}

#[napi(catch_unwind)]
pub fn engine() -> String {
    "aterm".to_string()
}

// --- orca-git: the verified status/numstat/line-count parsers, exposed to JS
// via this same .node so the daemon can prove parity with the TS parsers before
// any cut-over. JSON strings are the marshalling format (the status_result.rs
// builders match the TS shapes verbatim, omitting None fields). ---

/// Streaming `git status --porcelain=v2 --branch` parser — the chunked path the
/// daemon feeds raw stdout bytes. Mirrors `StatusPorcelainParser` in
/// `src/main/git/status-porcelain-parser.ts`.
#[napi(js_name = "GitStatusParser")]
pub struct JsGitStatusParser {
    // Option because into_result consumes the parser; result() take()s it.
    inner: Option<orca_git::status_stream::StatusPorcelainParser>,
}

#[napi]
impl JsGitStatusParser {
    #[napi(constructor, catch_unwind)]
    pub fn new() -> Self {
        Self {
            inner: Some(orca_git::status_stream::StatusPorcelainParser::new()),
        }
    }

    /// Feed one raw chunk. Returns true once the changed-entry count exceeds
    /// `limit` (0 disables the cap), signaling the caller to stop git.
    #[napi(catch_unwind)]
    pub fn update(&mut self, chunk: Buffer, limit: u32) -> bool {
        match self.inner.as_mut() {
            Some(parser) => parser.update(&chunk, limit as usize),
            // Already consumed by result(); nothing more to scan.
            None => false,
        }
    }

    /// Flush a final record with no trailing newline (e.g. when git exits).
    #[napi(catch_unwind)]
    pub fn finish(&mut self) {
        if let Some(parser) = self.inner.as_mut() {
            parser.finish();
        }
    }

    /// Consume the parser and return the status-result JSON. After the first call
    /// the parser is gone; a second call returns a valid empty result, never a panic.
    #[napi(catch_unwind)]
    pub fn result(&mut self, limit: u32) -> String {
        let result = match self.inner.take() {
            Some(parser) => parser.into_result(limit as usize),
            None => orca_git::status_stream::StatusPorcelainParser::new().into_result(limit as usize),
        };
        orca_git::status_result::status_parse_result_to_json(&result).to_string()
    }
}

/// One-shot status scan (the relay entry point): the cap is applied DURING the
/// scan, so `entries` is bounded by `limit` instead of materialize-then-truncate.
#[napi(catch_unwind)]
pub fn parse_status_porcelain(stdout: Buffer, limit: u32) -> String {
    let result = orca_git::status_stream::parse_status_porcelain(&stdout, limit as usize);
    orca_git::status_result::status_parse_result_to_json(&result).to_string()
}

/// `git diff --numstat` (text or `-z`) parsed to `{path: {added?, removed?}}`.
#[napi(catch_unwind)]
pub fn parse_numstat(stdout: Buffer) -> String {
    let entries = orca_git::numstat::parse_numstat(&stdout);
    orca_git::status_result::numstat_to_json(&entries).to_string()
}

/// Count additions for an untracked file's contents: null for binary, 0 for empty,
/// else the trailing-newline-aware line count.
#[napi(catch_unwind)]
pub fn count_additions_in_buffer(bytes: Buffer) -> Option<u32> {
    orca_git::line_count::count_additions_in_buffer(&bytes)
}

/// Approximate added/removed line counts; returns the line-stats JSON, or null
/// for the large-input guard.
#[napi(catch_unwind)]
pub fn compute_line_stats(original: String, modified: String, status: String) -> Option<String> {
    orca_git::line_count::compute_line_stats(&original, &modified, &status)
        .map(|stats| orca_git::status_result::line_stats_to_json(Some(stats)).to_string())
}

/// Decode a git C-quoted (octal-escaped) path. Raw (unquoted) input passes through.
/// js_name keeps the capital-Q the TS `decodeGitCQuotedPath` uses (napi would
/// otherwise lowercase "cquoted").
#[napi(js_name = "decodeGitCQuotedPath", catch_unwind)]
pub fn decode_git_cquoted_path(value: String) -> String {
    orca_core::git_cquoted_path::decode_git_cquoted_path(&value)
}

#[napi(catch_unwind)]
pub fn git_engine() -> &'static str {
    "orca-git"
}

/// Result of feeding a chunk to [`NdjsonParser`]: the complete lines to JSON-parse
/// (in order) plus the observed byte sizes of any oversized lines that were dropped.
#[napi(object)]
pub struct NdjsonFeedResult {
    /// Complete lines (newline-stripped, non-empty) in arrival order.
    pub lines: Vec<String>,
    /// Byte sizes of dropped oversized lines (one per oversized report).
    pub oversized: Vec<u32>,
}

/// Stateful NDJSON byte-budget line splitter (orca_net::NdjsonSplitter) — the OOM
/// guard for the daemon socket. `feed` returns complete lines for the caller to
/// JSON.parse; oversized lines are dropped + the stream resyncs at the next newline.
#[napi(js_name = "NdjsonParser")]
pub struct JsNdjsonParser {
    inner: orca_net::NdjsonSplitter,
}

#[napi]
impl JsNdjsonParser {
    #[napi(constructor, catch_unwind)]
    pub fn new(max_line_bytes: Option<u32>) -> Self {
        let max = max_line_bytes
            .map(|n| n as usize)
            .unwrap_or(orca_net::NDJSON_MAX_LINE_BYTES);
        Self {
            inner: orca_net::NdjsonSplitter::new(max),
        }
    }

    #[napi(catch_unwind)]
    pub fn feed(&mut self, chunk: String) -> NdjsonFeedResult {
        let mut lines = Vec::new();
        let mut oversized = Vec::new();
        for event in self.inner.feed_collect(&chunk) {
            match event {
                orca_net::NdjsonEvent::Line(line) => lines.push(line),
                orca_net::NdjsonEvent::Oversized { observed_bytes } => {
                    oversized.push(u32::try_from(observed_bytes).unwrap_or(u32::MAX));
                }
            }
        }
        NdjsonFeedResult { lines, oversized }
    }

    #[napi(catch_unwind)]
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}
