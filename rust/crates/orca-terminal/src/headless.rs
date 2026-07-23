//! A headless terminal grid — the `@xterm/headless` replacement: a server-side
//! screen + cursor that tracks the working directory via OSC-7, suitable for
//! snapshot/replay across reconnect and SSH.
//!
//! This is a thin **adapter** over the `aterm` engine (`aterm-core`), which owns
//! the real VT pipeline: a differential-tested parser, the 8-byte-cell grid,
//! tiered scrollback, the full SGR/colour model, OSC-7 cwd, and mouse modes.
//! `orca-terminal` keeps Orca's stable surface (`HeadlessTerminal`, `Cell`,
//! `Color`, `TerminalSnapshot`, …) so `orca-ffi`, `orca-session`, and the native
//! shells need no changes — only the engine underneath them is upgraded.

use aterm_core::terminal::{Terminal, TerminalBuilder};
use aterm_grid::{CellFlags, Grid, PackedColor, PackedColors};
use aterm_types::mouse::{MouseEncoding, MouseMode};

/// A run of columns on one row that share an OSC-8 hyperlink URL. Mirrors the
/// renderer's `TerminalOscLinkRange` (`src/shared/terminal-osc-link-ranges.ts`)
/// so restored snapshots keep clickable links. `end_col` is EXCLUSIVE, matching
/// the TS/xterm convention. `row` is 0-based from the top of the serialized
/// window (prepended scrollback rows first, then the visible grid).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OscLinkRange {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub uri: String,
}

/// Mouse-reporting mode set via DECSET (tracked for remote/SSH replay, like
/// `headless-emulator.ts`'s `mouseTrackingMode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseTracking {
    #[default]
    None,
    /// X10 compatibility (DECSET 9).
    X10,
    /// VT200 normal tracking (DECSET 1000).
    Normal,
    /// Button-event tracking (DECSET 1002).
    Button,
    /// Any-event tracking (DECSET 1003).
    Any,
}

/// Default scrollback line cap (matches `headless-emulator.ts DEFAULT_SCROLLBACK`).
pub const DEFAULT_SCROLLBACK: usize = 5000;

/// A terminal color: terminal default, a 256-palette index, or 24-bit truecolor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// SGR cell attributes (the full xterm-conformance set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub inverse: bool,
    pub conceal: bool,
    pub strike: bool,
    pub overline: bool,
    pub fg: Color,
    pub bg: Color,
}

/// A grid cell: a character plus its SGR attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub attrs: CellAttrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self { ch: ' ', attrs: CellAttrs::default() }
    }
}

/// A serializable snapshot of the terminal state, for reconnect / SSH replay
/// (the role of `@xterm/addon-serialize`). `lines` holds the visible grid, one
/// trailing-trimmed string per row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub rows: usize,
    pub cols: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cwd: Option<String>,
    pub lines: Vec<String>,
}

/// Clamp a `usize` dimension into the engine's `u16` grid space (min 1).
fn dim(v: usize) -> u16 {
    v.clamp(1, u16::MAX as usize) as u16
}

/// Headless terminal: feed it PTY output bytes, read back the grid / cursor /
/// cwd. Backed by `aterm`'s `Terminal`.
pub struct HeadlessTerminal {
    inner: Terminal,
    /// Cache for `serialize_ansi`, keyed by (grid content-generation, cursor,
    /// scrollback-row cap). The checkpoint (every 5s/session) and reconnect
    /// paths call it repeatedly; an idle pane hits the cache and skips the
    /// grid+scrollback walk. The cap is part of the key so a viewport-only
    /// (cap=0) request and a full-history request don't alias.
    serialize_cache: Option<(u64, (usize, usize), Option<usize>, String)>,
}

impl HeadlessTerminal {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self::with_scrollback(rows, cols, DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(rows: usize, cols: usize, scrollback_limit: usize) -> Self {
        // Orca reads scrollback as TEXT (+ OSC-8 links), never its colour. Enable
        // aterm's headless scrollback-text-only fast path so the scroll hot path
        // skips per-cell COLOUR/STYLE extraction (faster on colour-heavy floods)
        // while still retaining hyperlink spans; the visible grid keeps full
        // colour. Global + idempotent.
        aterm_grid::set_scrollback_text_only(true);
        // E1 (Wave-3 3A): attach the engine-default tiered store (fixed hot
        // ring + hot/warm compressed tiers; cold codec follows the BUILD —
        // zstd/disk here, never copied into the wasm ctor) instead of a bare
        // uncompressed ring (~640 B/line, content-independent).
        let mut inner = TerminalBuilder::new()
            .size(dim(rows), dim(cols))
            .tiered_scrollback_defaults()
            .build();
        // ONE total retention limit (Codex-corrected E1): the P4-forwarded
        // value caps ring + staged + store TOGETHER — never `limit + ring`,
        // and never the store's 100k `DEFAULT_LINE_LIMIT` (#7929).
        inner.set_scrollback_line_limit(Some(scrollback_limit.max(1)));
        Self { inner, serialize_cache: None }
    }

    /// The unified scrollback retention total (ring + staged + store), as set
    /// at construction from the P4-forwarded rows value. `None` = unlimited.
    pub fn scrollback_line_limit(&self) -> Option<usize> {
        self.inner.scrollback_line_limit()
    }

    /// THRU-5: attach/detach the off-thread compression worker. While active,
    /// PTY-feed ingest defers tier promotion to whoever drives
    /// [`drain_lazy_bounded`](Self::drain_lazy_bounded) — the daemon must pair
    /// this with a drain worker or the staged backlog only drains at the
    /// engine's backpressure cap. Inactive (default), ingest promotes inline
    /// in bounded batches on the feeding thread.
    pub fn set_compress_offload_active(&mut self, active: bool) {
        self.inner.set_compress_offload_active(active);
    }

    /// THRU-5: lines staged awaiting off-thread tier promotion (the compression
    /// worker's backlog; 0 when offload is inactive or drained).
    pub fn lazy_backlog_len(&self) -> usize {
        self.inner.lazy_backlog_len()
    }

    /// THRU-5: promote up to `max_lines` staged lines into the compressed
    /// tiers (one bounded LZ4/zstd batch under the caller's lock hold);
    /// returns the lines still staged so a worker can loop.
    pub fn drain_lazy_bounded(&mut self, max_lines: usize) -> usize {
        self.inner.drain_lazy_bounded(max_lines)
    }

    /// Number of lines currently held in scrollback (off-screen above the grid).
    pub fn scrollback_len(&self) -> usize {
        // Total history = ring buffer + tiered scrollback. (`Terminal::scrollback()`
        // alone sees only the tiered tier, which is empty under a ring config.)
        self.inner.grid().scrollback_lines()
    }

    /// Text of scrollback line `index` (0 = oldest), trailing blanks trimmed.
    pub fn scrollback_row_text(&self, index: usize) -> String {
        match self.inner.grid().get_history_line(index) {
            Some(line) => line.as_str().map(|s| s.trim_end().to_string()).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Feed raw output bytes through the parser into the grid.
    pub fn process(&mut self, bytes: &[u8]) {
        self.inner.process(bytes);
    }

    pub fn process_str(&mut self, text: &str) {
        self.process(text.as_bytes());
    }

    /// A row's text with trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        self.inner.row_text(row).unwrap_or_default().trim_end().to_string()
    }

    /// The cell at `(row, col)`, including its SGR attributes. Out-of-bounds → None.
    pub fn cell(&self, row: usize, col: usize) -> Option<Cell> {
        let (r, c) = (u16::try_from(row).ok()?, u16::try_from(col).ok()?);
        resolve_cell(self.inner.grid(), r, c)
    }

    /// Compact fingerprint of a cell's SGR attributes + colours, for the xterm
    /// conformance harness (`examples/conformance.rs`). Format matches the goldens.
    pub fn cell_attr_fingerprint(&self, row: usize, col: usize) -> String {
        let cell = self.cell(row, col).unwrap_or_default();
        let a = cell.attrs;
        let color = |c: Color| match c {
            Color::Default => "d".to_string(),
            Color::Indexed(n) => format!("p{n}"),
            Color::Rgb(r, g, b) => format!("r{r},{g},{b}"),
        };
        let mut f = String::new();
        if a.bold {
            f.push('b');
        }
        if a.dim {
            f.push('d');
        }
        if a.italic {
            f.push('i');
        }
        if a.underline {
            f.push('u');
        }
        if a.blink {
            f.push('k');
        }
        if a.inverse {
            f.push('v');
        }
        if a.conceal {
            f.push('c');
        }
        if a.strike {
            f.push('s');
        }
        if a.overline {
            f.push('o');
        }
        format!("{f}/{}/{}", color(a.fg), color(a.bg))
    }

    /// Current mouse-reporting mode (set via DECSET).
    pub fn mouse_tracking(&self) -> MouseTracking {
        match self.inner.mouse_mode() {
            MouseMode::None => MouseTracking::None,
            MouseMode::X10 => MouseTracking::X10,
            MouseMode::Normal => MouseTracking::Normal,
            MouseMode::ButtonEvent => MouseTracking::Button,
            MouseMode::AnyEvent => MouseTracking::Any,
            // `MouseMode` is #[non_exhaustive]; unknown future modes read as off.
            _ => MouseTracking::None,
        }
    }
    /// Whether SGR mouse encoding (DECSET 1006) is on.
    pub fn sgr_mouse(&self) -> bool {
        matches!(self.inner.mouse_encoding(), MouseEncoding::Sgr)
    }
    /// Whether SGR pixel mouse encoding (DECSET 1016) is on.
    pub fn sgr_pixels(&self) -> bool {
        matches!(self.inner.mouse_encoding(), MouseEncoding::SgrPixel)
    }

    /// All visible rows, trailing blanks trimmed (a minimal snapshot).
    pub fn snapshot(&self) -> Vec<String> {
        (0..self.inner.rows() as usize).map(|row| self.row_text(row)).collect()
    }

    /// `(row, col)` cursor position.
    pub fn cursor(&self) -> (usize, usize) {
        let c = self.inner.cursor();
        (c.row as usize, c.col as usize)
    }

    pub fn cwd(&self) -> Option<&str> {
        self.inner.current_working_directory()
    }

    pub fn size(&self) -> (usize, usize) {
        (self.inner.rows() as usize, self.inner.cols() as usize)
    }

    /// Capture a serializable snapshot for reconnect / SSH replay.
    pub fn capture(&self) -> TerminalSnapshot {
        let (rows, cols) = self.size();
        let (cursor_row, cursor_col) = self.cursor();
        TerminalSnapshot {
            rows,
            cols,
            cursor_row,
            cursor_col,
            cwd: self.cwd().map(str::to_string),
            lines: self.snapshot(),
        }
    }

    /// Rebuild a terminal from a snapshot (parser starts fresh). Visible text,
    /// cursor, and cwd are restored by replaying the captured rows; SGR
    /// attributes are not part of the persisted snapshot.
    pub fn from_snapshot(snapshot: &TerminalSnapshot) -> Self {
        let rows = dim(snapshot.rows);
        let cols = dim(snapshot.cols);
        let mut term = Self::new(rows as usize, cols as usize);
        for (i, line) in snapshot.lines.iter().take(rows as usize).enumerate() {
            if i > 0 {
                term.process(b"\r\n");
            }
            term.process(line.as_bytes());
        }
        // Restore the cursor with an absolute CUP (1-based); the engine clamps
        // to the grid. Restore cwd directly.
        let target_row = snapshot.cursor_row.min(rows as usize - 1) + 1;
        let target_col = snapshot.cursor_col.min(cols as usize - 1) + 1;
        term.process(format!("\x1b[{target_row};{target_col}H").as_bytes());
        // Restore cwd through the production OSC-7 path (empty-host file URI),
        // so the engine decodes it exactly as a live shell would have set it.
        if let Some(cwd) = &snapshot.cwd {
            term.process(format!("\x1b]7;file://{cwd}\x07").as_bytes());
        }
        term
    }

    /// Resize the grid (client viewport change).
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.inner.resize(dim(rows), dim(cols));
    }

    /// Drop all scrollback history (keeps the visible grid).
    pub fn clear_scrollback(&mut self) {
        self.inner.clear_scrollback();
    }

    /// Whether the alternate screen buffer (DECSET 1049) is active.
    pub fn is_alternate_screen(&self) -> bool {
        self.inner.is_alternate_screen()
    }

    /// Current Kitty keyboard enhancement flags (0 = protocol inactive). The
    /// daemon carries this in the snapshot modes so a reattach re-anchors CSI-u
    /// keyboard state, matching the Node daemon's TerminalKittyKeyboardModeTracker.
    pub fn kitty_keyboard_flags(&self) -> u8 {
        self.inner.kitty_keyboard_flags().bits()
    }

    /// Whether bracketed-paste mode (DECSET 2004) is on.
    pub fn bracketed_paste(&self) -> bool {
        self.inner.modes().bracketed_paste()
    }

    /// Whether application-cursor-keys mode (DECCKM) is on.
    pub fn application_cursor(&self) -> bool {
        self.inner.modes().application_cursor_keys()
    }

    /// Replayable ANSI for the snapshot: scrollback history (as flowing text)
    /// then each visible row placed with absolute CUP + erase-line so a
    /// full-width row can't autowrap on replay, then the cursor restored. The
    /// visible grid is emitted via aterm's `Grid::row_ansi_text_screen` — which handles
    /// wide-char (CJK/emoji) continuation correctly and emits minimal,
    /// change-based SGR (vs a full reset+colour per cell). Scrollback is
    /// text-only (the headless scrollback-text-only mode drops history colour).
    /// `scrollback_rows` caps how many of the most-recent active-history rows are
    /// prepended before the visible grid: `None` prepends ALL history (the
    /// session/reconnect default), `Some(0)` is viewport-only, `Some(n)` keeps the
    /// last `n` rows — matching `@xterm/addon-serialize`'s `serialize({scrollback})`.
    pub fn serialize_ansi(&mut self, scrollback_rows: Option<usize>) -> String {
        let key = (self.inner.grid().content_gen(), self.cursor());
        if let Some((gen, cursor, cap, ref cached)) = self.serialize_cache {
            if (gen, cursor) == key && cap == scrollback_rows {
                return cached.clone();
            }
        }
        let out = self.serialize_ansi_uncached(scrollback_rows);
        self.serialize_cache = Some((key.0, key.1, scrollback_rows, out.clone()));
        out
    }

    /// Scrollback history ONLY (the off-screen lines above the viewport), as
    /// flowing text + CRLF — no cursor/grid framing. The daemon stores this in
    /// `scrollbackAnsi`; on cold-restore of an alternate-screen session (vim,
    /// htop, less) it is the ONLY recoverable history, since the visible
    /// `snapshotAnsi` is the TUI/alt buffer, not the user's scrollback.
    ///
    /// Reads the MAIN buffer's scrollback (`aterm` keeps it in the inactive grid
    /// while the alt screen is active), so an in-alt-screen snapshot still
    /// recovers the pre-TUI history. Empty when there is no scrollback.
    /// `max_rows` caps the history to its last `n` lines (`None` = all). Used by
    /// the daemon to bound the persisted `scrollbackAnsi` payload.
    pub fn serialize_scrollback_ansi(&self, max_rows: Option<usize>) -> String {
        let grid = self.inner.main_grid();
        let history = grid.scrollback_lines();

        // In the alternate screen the main buffer's VISIBLE rows are recoverable
        // history too: on `?1049l` exit the user returns to them, yet they never
        // scrolled into `scrollback_lines()` (a short pre-TUI shell screen has
        // nothing off-viewport). Capture them (trailing blanks trimmed) so an
        // in-alt-screen cold-restore recovers the whole pre-TUI screen, not just
        // the lines that happened to scroll away. Outside the alt screen the
        // visible grid IS the live snapshot (`serialize_ansi` frames it), so
        // scrollback stays history-only.
        let mut visible: Vec<String> = Vec::new();
        if self.inner.is_alternate_screen() {
            visible = (0..self.inner.rows())
                .map(|r| grid.row_text(r).unwrap_or_default().trim_end().to_string())
                .collect();
            while visible.last().is_some_and(|l| l.is_empty()) {
                visible.pop();
            }
        }

        if history == 0 && visible.is_empty() {
            return String::new();
        }

        // `max_rows` bounds the COMBINED payload (history + preserved visible
        // rows), keeping the most recent lines. Read only the tail of history so
        // a large scrollback isn't fully materialized.
        let total = history + visible.len();
        let take = max_rows.map_or(total, |n| n.min(total));
        let hist_take = take.saturating_sub(visible.len()).min(history);
        let vis_skip = visible.len().saturating_sub(take);

        let mut out = String::new();
        for i in (history - hist_take)..history {
            let line = grid
                .get_history_line(i)
                .and_then(|l| l.as_str().map(|s| s.trim_end().to_string()))
                .unwrap_or_default();
            out.push_str(&line);
            out.push_str("\r\n");
        }
        for line in visible.into_iter().skip(vis_skip) {
            out.push_str(&line);
            out.push_str("\r\n");
        }
        out
    }

    /// OSC-8 hyperlink ranges over the serialized window (the same `scrollback_rows`
    /// of history that `serialize_ansi` prepends, then the visible grid), so a
    /// restored snapshot keeps clickable links. Mirrors the TS
    /// `collectHeadlessOscLinkRanges`: consecutive cells sharing a URL coalesce
    /// into one run; `row` is 0-based from the window top; `end_col` is exclusive.
    ///
    /// Covers BOTH the visible grid and scrollback history — aterm retains
    /// hyperlink spans on scroll even under the headless text-only fast path.
    pub fn osc_link_ranges(&self, scrollback_rows: Option<usize>) -> Vec<OscLinkRange> {
        let mut ranges = Vec::new();
        let hist = self.scrollback_len();
        let take = scrollback_rows.map_or(hist, |n| n.min(hist));
        let first_hist = hist - take;

        let main = self.inner.main_grid();
        for i in first_hist..hist {
            let Some(line) = main.get_history_line(i) else { continue };
            let Some(spans) = line.hyperlinks() else { continue };
            for span in spans {
                ranges.push(OscLinkRange {
                    row: i - first_hist,
                    start_col: span.start_col as usize,
                    end_col: span.end_col as usize,
                    uri: span.url.to_string(),
                });
            }
        }

        let grid = self.inner.grid();
        let rows = self.inner.rows();
        let cols = self.inner.cols();
        for r in 0..rows {
            let mut run_start: Option<usize> = None;
            let mut run_uri: Option<&str> = None;
            for c in 0..cols {
                let uri = grid.cell_extra(r, c).and_then(|e| e.hyperlink()).map(|u| u.as_ref());
                if uri != run_uri {
                    if let (Some(start), Some(prev)) = (run_start, run_uri) {
                        ranges.push(OscLinkRange {
                            row: take + r as usize,
                            start_col: start,
                            end_col: c as usize,
                            uri: prev.to_string(),
                        });
                    }
                    run_start = uri.map(|_| c as usize);
                    run_uri = uri;
                }
            }
            if let (Some(start), Some(prev)) = (run_start, run_uri) {
                ranges.push(OscLinkRange {
                    row: take + r as usize,
                    start_col: start,
                    end_col: cols as usize,
                    uri: prev.to_string(),
                });
            }
        }
        ranges
    }

    /// The window title (OSC 0/2). `None` when unset — feeds the snapshot's
    /// `lastTitle`, which Orca uses for agent detection.
    pub fn title(&self) -> Option<String> {
        let title = self.inner.title();
        if title.is_empty() { None } else { Some(title.to_string()) }
    }

    fn serialize_ansi_uncached(&self, scrollback_rows: Option<usize>) -> String {
        let mut out = String::from("\x1b[0m");
        // The full visible snapshot uses the ACTIVE grid's scrollback (which for
        // an alt-screen session is empty — the alt buffer has no scrollback);
        // the main-buffer history is carried separately by
        // `serialize_scrollback_ansi` for the alt cold-restore path. `scrollback_rows`
        // caps the prepended history to its last `n` lines (the most recent).
        let active_history = self.scrollback_len();
        let take = scrollback_rows.map_or(active_history, |n| n.min(active_history));
        for i in (active_history - take)..active_history {
            out.push_str(&self.scrollback_row_text(i));
            out.push_str("\r\n");
        }
        if take > 0 {
            // Scroll the printed history OFF the screen so it lands in the replay
            // target's scrollback: the trailing printed lines are still on the
            // visible grid here, and the absolute-CUP viewport paint below would
            // ERASE them — losing a viewport-sized chunk of history on every
            // replay (all of it when take < rows). One LF per resident text line
            // (at most rows-1: the final CRLF left the bottom row blank) from the
            // bottom row scrolls each top line into history and leaves a clean
            // screen for the viewport paint.
            let rows = self.inner.rows() as usize;
            out.push_str(&format!("\x1b[{};1H", rows));
            for _ in 0..take.min(rows.saturating_sub(1)) {
                out.push('\n');
            }
        }
        out.push_str("\x1b[H");
        let grid = self.inner.grid();
        for r in 0..self.inner.rows() {
            out.push_str(&format!("\x1b[{};1H\x1b[K", r + 1));
            // Read the LIVE screen row (offset-independent): serialize captures state,
            // not the user's scroll view, so a scrolled-back emoji/RGB row is not
            // re-emitted from history with mismatched extras.
            if let Some(row_ansi) = grid.row_ansi_text_screen(r) {
                out.push_str(&row_ansi);
            }
            out.push_str("\x1b[0m");
        }
        let (cr, cc) = self.cursor();
        out.push_str(&format!("\x1b[{};{}H", cr + 1, cc + 1));
        out
    }
}

/// Resolve a grid cell into Orca's `Cell` (char + SGR attrs + colour kind).
///
/// Mirrors aterm's own render resolution (`render_cells.rs`) — style-interned
/// cells rehydrate via the style table, inline cells read their packed colours
/// plus the RGB overflow table — but yields Orca's `Color` enum so the
/// Default/Indexed/Rgb distinction survives instead of being flattened to RGB.
fn resolve_cell(grid: &Grid, row: u16, col: u16) -> Option<Cell> {
    let grid_row = grid.row(row)?;
    if col >= grid_row.len() {
        return None;
    }
    let cell = grid_row.get(col)?;
    let ch = grid
        .resolved_char(row, col)
        .map(|c| if c == '\0' { ' ' } else { c })
        .unwrap_or(' ');

    let (fg, bg, flags) = if cell.uses_style_id() {
        let extra = cell.flags().difference(CellFlags::USES_STYLE_ID);
        let (fg_pc, bg_pc, merged) = grid.resolve_style_to_colors(cell.style_id(), extra);
        (legacy_color(fg_pc), legacy_color(bg_pc), merged)
    } else {
        let colors = cell.colors();
        let fg = packed_color(colors, true, grid.fg_rgb_at(row, col));
        let bg = packed_color(colors, false, grid.bg_rgb_at(row, col));
        (fg, bg, cell.flags())
    };

    Some(Cell {
        ch,
        attrs: CellAttrs {
            bold: flags.contains(CellFlags::BOLD),
            dim: flags.contains(CellFlags::DIM),
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.contains(CellFlags::UNDERLINE),
            blink: flags.contains(CellFlags::BLINK),
            inverse: flags.contains(CellFlags::INVERSE),
            conceal: flags.contains(CellFlags::HIDDEN),
            strike: flags.contains(CellFlags::STRIKETHROUGH),
            overline: flags.contains(CellFlags::OVERLINE),
            fg,
            bg,
        },
    })
}

/// Map a legacy `PackedColor` (the style-table resolution format) to `Color`.
fn legacy_color(p: PackedColor) -> Color {
    if p.is_rgb() {
        let (r, g, b) = p.rgb_components();
        Color::Rgb(r, g, b)
    } else if p.is_indexed() {
        Color::Indexed(p.index())
    } else {
        Color::Default
    }
}

/// Map an inline cell's `PackedColors` field (`fg` or `bg`) to `Color`. RGB
/// cells keep their triple in the grid's overflow table, passed in as `rgb`.
fn packed_color(colors: PackedColors, fg: bool, rgb: Option<[u8; 3]>) -> Color {
    let (is_rgb, is_indexed, index) = if fg {
        (colors.fg_is_rgb(), colors.fg_is_indexed(), colors.fg_index())
    } else {
        (colors.bg_is_rgb(), colors.bg_is_indexed(), colors.bg_index())
    };
    if is_rgb {
        let [r, g, b] = rgb.unwrap_or([0, 0, 0]);
        Color::Rgb(r, g, b)
    } else if is_indexed {
        Color::Indexed(index)
    } else {
        Color::Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_text_across_crlf() {
        let mut term = HeadlessTerminal::new(24, 80);
        term.process_str("hello\r\nworld");
        assert_eq!(term.row_text(0), "hello");
        assert_eq!(term.row_text(1), "world");
        assert_eq!(term.cursor(), (1, 5));
    }

    #[test]
    fn carriage_return_overwrites_from_column_zero() {
        let mut term = HeadlessTerminal::new(4, 10);
        term.process_str("abc\rX");
        assert_eq!(term.row_text(0), "Xbc");
    }

    #[test]
    fn backspace_moves_cursor_back() {
        let mut term = HeadlessTerminal::new(4, 10);
        term.process_str("abc\x08X");
        assert_eq!(term.row_text(0), "abX");
    }

    #[test]
    fn line_feed_scrolls_when_at_bottom() {
        let mut term = HeadlessTerminal::new(2, 4);
        term.process_str("a\r\nb\r\nc");
        assert_eq!(term.snapshot(), vec!["b".to_string(), "c".to_string()]);
        assert_eq!(term.cursor(), (1, 1));
    }

    #[test]
    fn tracks_cwd_via_osc7() {
        let mut term = HeadlessTerminal::new(24, 80);
        term.process_str("\x1b]7;file://hostname/Users/me/project\x07");
        assert_eq!(term.cwd(), Some("/Users/me/project"));
    }

    #[test]
    fn osc7_decodes_percent_escapes_and_empty_host() {
        let mut term = HeadlessTerminal::new(24, 80);
        term.process_str("\x1b]7;file:///Users/me/my%20repo\x07");
        assert_eq!(term.cwd(), Some("/Users/me/my repo"));
    }

    #[test]
    fn snapshot_round_trips_grid_cursor_and_cwd() {
        let mut term = HeadlessTerminal::new(4, 12);
        term.process_str("\x1b]7;file:///srv/app\x07first\r\nsecond");
        let snapshot = term.capture();

        let restored = HeadlessTerminal::from_snapshot(&snapshot);
        assert_eq!(restored.capture(), snapshot);
        assert_eq!(restored.row_text(0), "first");
        assert_eq!(restored.row_text(1), "second");
        assert_eq!(restored.cursor(), (1, 6));
        assert_eq!(restored.cwd(), Some("/srv/app"));
    }

    #[test]
    fn sgr_attributes_apply_to_printed_cells() {
        let mut term = HeadlessTerminal::new(2, 10);
        // bold + red fg, print 'E', reset, print 'N'
        term.process_str("\x1b[1;31mE\x1b[0mN");
        let e = term.cell(0, 0).unwrap();
        assert_eq!(e.ch, 'E');
        assert!(e.attrs.bold);
        assert_eq!(e.attrs.fg, Color::Indexed(1));
        let n = term.cell(0, 1).unwrap();
        assert_eq!(n.ch, 'N');
        assert_eq!(n.attrs, CellAttrs::default());
    }

    #[test]
    fn sgr_256_and_truecolor_and_bright() {
        let mut term = HeadlessTerminal::new(1, 10);
        // 256-palette fg 200, truecolor bg, bright-green fg
        term.process_str("\x1b[38;5;200mA\x1b[48;2;10;20;30mB\x1b[92mC");
        assert_eq!(term.cell(0, 0).unwrap().attrs.fg, Color::Indexed(200));
        let b = term.cell(0, 1).unwrap().attrs;
        assert_eq!(b.fg, Color::Indexed(200)); // fg persists
        assert_eq!(b.bg, Color::Rgb(10, 20, 30));
        assert_eq!(term.cell(0, 2).unwrap().attrs.fg, Color::Indexed(10)); // bright green = 8+2
    }

    #[test]
    fn bare_sgr_resets_pen() {
        let mut term = HeadlessTerminal::new(1, 10);
        term.process_str("\x1b[4mU\x1b[mP"); // underline U, bare reset, plain P
        assert!(term.cell(0, 0).unwrap().attrs.underline);
        assert_eq!(term.cell(0, 1).unwrap().attrs, CellAttrs::default());
    }

    #[test]
    fn decset_mouse_modes_are_tracked() {
        let mut term = HeadlessTerminal::new(4, 10);
        assert_eq!(term.mouse_tracking(), MouseTracking::None);

        term.process_str("\x1b[?1000h"); // normal tracking on
        assert_eq!(term.mouse_tracking(), MouseTracking::Normal);
        term.process_str("\x1b[?1002h"); // button-event tracking
        assert_eq!(term.mouse_tracking(), MouseTracking::Button);
        term.process_str("\x1b[?1003h"); // any-event tracking
        assert_eq!(term.mouse_tracking(), MouseTracking::Any);
        term.process_str("\x1b[?1003l"); // reset
        assert_eq!(term.mouse_tracking(), MouseTracking::None);

        assert!(!term.sgr_mouse());
        term.process_str("\x1b[?1006h");
        assert!(term.sgr_mouse());
        term.process_str("\x1b[?1016h");
        assert!(term.sgr_pixels());
    }

    #[test]
    fn scrollback_retains_scrolled_off_lines() {
        // Regression: the engine must actually keep history scrolled above the
        // viewport (an earlier adapter wired the wrong accessor and reported 0).
        let mut term = HeadlessTerminal::with_scrollback(2, 16, 100);
        for i in 0..10 {
            term.process_str(&format!("line{i}\r\n"));
        }
        assert!(term.scrollback_len() >= 7, "scrollback_len={}", term.scrollback_len());
        assert_eq!(term.scrollback_row_text(0), "line0"); // oldest retained
    }

    #[test]
    fn scrollback_is_bounded_by_the_limit() {
        let mut term = HeadlessTerminal::with_scrollback(1, 8, 3);
        for i in 0..20 {
            term.process_str(&format!("L{i}\r\n"));
        }
        assert!(term.scrollback_len() <= 3, "exceeded cap: {}", term.scrollback_len());
    }

    #[test]
    fn tiered_attach_keeps_one_total_retention_limit() {
        // E1 wiring (Wave-3 3A / 3BC prep): the P4-forwarded rows value is the
        // WHOLE budget — the unified getter must round-trip it exactly, never
        // the store's 100k DEFAULT_LINE_LIMIT (#7929) and never `value + ring`.
        let term = HeadlessTerminal::with_scrollback(24, 80, 7000);
        assert_eq!(term.scrollback_line_limit(), Some(7000));
        let small = HeadlessTerminal::with_scrollback(24, 80, DEFAULT_SCROLLBACK);
        assert_eq!(small.scrollback_line_limit(), Some(DEFAULT_SCROLLBACK));
    }

    #[test]
    fn tiered_scrollback_text_and_links_match_ring_only() {
        // 3BC prep oracle: the daemon reads scrollback as text + OSC-8 spans
        // under the global scrollback-text-only mode; the tiered store's warm
        // codec must preserve that contract exactly as the raw ring did.
        let feed = |term: &mut HeadlessTerminal| {
            term.process_str("\x1b[1;31mred alert\x1b[0m\r\n");
            term.process_str("a \x1b]8;;https://example.com/t\x07link\x1b]8;;\x07 z\r\n");
            for i in 0..40 {
                term.process_str(&format!("bulk line {i}\r\n"));
            }
            term.process_str("last");
        };
        let mut tiered = HeadlessTerminal::with_scrollback(2, 40, 1000);
        feed(&mut tiered);
        // Ring-only reference at identical content (the pre-E1 construction).
        let inner = TerminalBuilder::new().size(2, 40).ring_buffer_size(1000).build();
        let mut ring = HeadlessTerminal { inner, serialize_cache: None };
        feed(&mut ring);

        assert_eq!(tiered.scrollback_len(), ring.scrollback_len());
        for i in 0..ring.scrollback_len() {
            assert_eq!(
                tiered.scrollback_row_text(i),
                ring.scrollback_row_text(i),
                "history row {i} diverged between tiered and ring-only stores"
            );
        }
        assert_eq!(tiered.osc_link_ranges(None), ring.osc_link_ranges(None));
    }

    #[test]
    fn offload_drain_promotes_staged_backlog_without_losing_rows() {
        // The daemon pairs the tiered store with a compress-offload drain
        // worker (3A integrator note): with offload active, ingest stages
        // scrolled-off lines and a bounded drain promotes them without
        // reordering or dropping history.
        // Past the fixed hot-ring cap so overflow actually stages (the ring
        // absorbs the first ~1000 history lines before the lazy buffer fills).
        let mut term = HeadlessTerminal::with_scrollback(2, 20, 3000);
        term.set_compress_offload_active(true);
        for i in 0..2200 {
            term.process_str(&format!("row {i}\r\n"));
        }
        assert!(term.lazy_backlog_len() > 0, "offload-active ingest should stage lines");
        let mut guard = 0;
        while term.drain_lazy_bounded(64) > 0 {
            guard += 1;
            assert!(guard < 100, "bounded drain must make progress");
        }
        assert_eq!(term.lazy_backlog_len(), 0);
        assert_eq!(term.scrollback_row_text(0), "row 0");
        let len = term.scrollback_len();
        assert!(len >= 2100, "history retained through the drain, got {len}");
        assert!(len <= 3000, "the ONE total limit bounds ring+staged+store");
    }

    #[test]
    fn serialize_ansi_round_trips_visible_grid_with_attrs() {
        let mut term = HeadlessTerminal::new(4, 20);
        term.process_str("\x1b[1;32mhello\x1b[0m\r\nworld");
        let ansi = term.serialize_ansi(None);

        let mut restored = HeadlessTerminal::new(4, 20);
        restored.process_str(&ansi);
        assert_eq!(restored.row_text(0), "hello");
        assert_eq!(restored.row_text(1), "world");
        // visible-grid SGR survives the replay
        let h = restored.cell(0, 0).unwrap();
        assert!(h.attrs.bold);
        assert_eq!(h.attrs.fg, Color::Indexed(2));
        assert_eq!(restored.cursor(), term.cursor());
    }

    #[test]
    fn serialize_ansi_preserves_wide_chars() {
        // Regression: the old per-cell loop indexed physical columns by logical
        // char count, so a CJK row "日本X" (3 chars / 5 cols) replayed as "日 本".
        // Delegating to Grid::row_ansi_text_screen handles wide-continuation correctly.
        let mut term = HeadlessTerminal::new(2, 20);
        term.process_str("日本X");
        let ansi = term.serialize_ansi(None);
        let mut restored = HeadlessTerminal::new(2, 20);
        restored.process_str(&ansi);
        assert_eq!(restored.row_text(0), "日本X");
    }

    #[test]
    fn serialize_scrollback_ansi_is_history_only() {
        // 1-row grid: each newline evicts the prior line into scrollback.
        let mut term = HeadlessTerminal::with_scrollback(1, 20, 100);
        term.process_str("alpha\r\nbravo\r\ncharlie");
        let sb = term.serialize_scrollback_ansi(None);
        // History (alpha, bravo) is present; the VISIBLE line (charlie) is not,
        // and there's no cursor/grid framing.
        assert!(sb.contains("alpha") && sb.contains("bravo"), "sb={sb:?}");
        assert!(!sb.contains("charlie"), "scrollback must exclude the visible row");
        assert!(!sb.contains('\x1b'), "scrollback is plain text, no escapes: {sb:?}");
        // Empty when there is no scrollback.
        let mut fresh = HeadlessTerminal::new(24, 80);
        fresh.process_str("just one line");
        assert_eq!(fresh.serialize_scrollback_ansi(None), "");
    }

    #[test]
    fn serialize_ansi_caches_and_invalidates_on_change() {
        let mut term = HeadlessTerminal::new(4, 20);
        term.process_str("hello");
        let a = term.serialize_ansi(None);
        assert_eq!(a, term.serialize_ansi(None), "unchanged grid -> cache hit, identical output");
        term.process_str("X"); // content_gen bumps
        let b = term.serialize_ansi(None);
        assert_ne!(a, b, "content change -> cache miss, fresh serialization");
        // replay fidelity is preserved through the cache path
        let mut restored = HeadlessTerminal::new(4, 20);
        restored.process_str(&b);
        assert_eq!(restored.row_text(0), "helloX");
    }

    #[test]
    fn serialize_scrollback_ansi_preserves_main_visible_rows_in_alt_screen() {
        // A short pre-TUI shell screen (2 lines, 6-row grid) has NO off-screen
        // scrollback, so history-only serialization loses it when a TUI enters
        // the alt screen. The main buffer's visible rows must be preserved.
        let mut term = HeadlessTerminal::new(6, 40);
        term.process_str("shell history one\r\nshell history two");
        term.process_str("\x1b[?1049h\x1b[2J\x1b[HTUI frame");
        assert!(term.is_alternate_screen());
        let sb = term.serialize_scrollback_ansi(None);
        assert!(sb.contains("shell history one"), "sb={sb:?}");
        assert!(sb.contains("shell history two"), "sb={sb:?}");
        // Trailing blank main rows are trimmed and the alt content is excluded.
        assert!(!sb.contains("TUI frame"), "alt content must not leak: {sb:?}");
        assert_eq!(sb, "shell history one\r\nshell history two\r\n", "sb={sb:?}");
    }

    #[test]
    fn mode_getters_track_decset() {
        let mut term = HeadlessTerminal::new(4, 10);
        assert!(!term.is_alternate_screen() && !term.bracketed_paste() && !term.application_cursor());
        term.process_str("\x1b[?1049h\x1b[?2004h\x1b[?1h");
        assert!(term.is_alternate_screen());
        assert!(term.bracketed_paste());
        assert!(term.application_cursor());
    }

    #[test]
    fn resize_grow_preserves_content_and_widens() {
        let mut term = HeadlessTerminal::new(2, 5);
        term.process_str("top\r\nbot");
        term.resize(4, 8);
        assert_eq!(term.size(), (4, 8));
        assert_eq!(term.row_text(0), "top");
        assert_eq!(term.row_text(1), "bot");
    }

    #[test]
    fn osc_link_ranges_captures_visible_hyperlinks() {
        let mut term = HeadlessTerminal::new(2, 40);
        // OSC-8: ESC]8;;URL BEL  text  ESC]8;; BEL
        term.process_str("a \x1b]8;;https://example.com\x07link\x1b]8;;\x07 b");
        let ranges = term.osc_link_ranges(None);
        assert_eq!(ranges.len(), 1, "ranges={ranges:?}");
        let r = &ranges[0];
        assert_eq!(r.uri, "https://example.com");
        assert_eq!(r.row, 0);
        assert_eq!(r.start_col, 2); // after "a "
        assert_eq!(r.end_col, 6); // "link" is [2, 6), end exclusive
    }

    #[test]
    fn osc_link_ranges_captures_scrolled_off_history_links() {
        // A linked word that scrolls into history stays recoverable (aterm keeps
        // hyperlink spans on scroll even in the headless text-only mode), but is
        // excluded when scrollback rows are clipped to the viewport.
        let mut term = HeadlessTerminal::with_scrollback(2, 80, 10);
        term.process_str("\x1b]8;;https://example.com/old\x07old\x1b]8;;\x07\r\nplain\r\nvisible");
        let all = term.osc_link_ranges(None);
        assert!(
            all.iter().any(|r| r.uri == "https://example.com/old"),
            "scrolled-off link should be captured: {all:?}"
        );
        let viewport = term.osc_link_ranges(Some(0));
        assert!(
            !viewport.iter().any(|r| r.uri == "https://example.com/old"),
            "viewport-only must exclude the scrolled-off link: {viewport:?}"
        );
    }

    #[test]
    fn serialize_scrollback_ansi_respects_max_rows() {
        let mut term = HeadlessTerminal::with_scrollback(1, 20, 100);
        term.process_str("a\r\nb\r\nc\r\nd"); // a,b,c -> history; d visible
        let all = term.serialize_scrollback_ansi(None);
        assert!(all.contains('a') && all.contains('b') && all.contains('c'));
        let last_two = term.serialize_scrollback_ansi(Some(2));
        assert!(last_two.contains('b') && last_two.contains('c'), "last_two={last_two:?}");
        assert!(!last_two.contains('a'), "max_rows=2 keeps only the last 2 lines: {last_two:?}");
    }

    #[test]
    fn serialize_ansi_scrollback_rows_caps_prepended_history() {
        let mut term = HeadlessTerminal::with_scrollback(1, 20, 100);
        term.process_str("h0\r\nh1\r\nh2\r\nvis");
        let viewport_only = term.serialize_ansi(Some(0));
        assert!(viewport_only.contains("vis"));
        assert!(!viewport_only.contains("h0") && !viewport_only.contains("h2"));
        let with_one = term.serialize_ansi(Some(1));
        assert!(with_one.contains("h2"), "1 prepended history row expected: {with_one:?}");
        assert!(!with_one.contains("h1"));
    }
}
