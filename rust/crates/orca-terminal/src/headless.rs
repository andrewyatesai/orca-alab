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

use aterm_core::terminal::Terminal;
use aterm_grid::{CellFlags, Grid, PackedColor, PackedColors};
use aterm_types::mouse::{MouseEncoding, MouseMode};

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

/// SGR cell attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellAttrs {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
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
}

impl HeadlessTerminal {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self::with_scrollback(rows, cols, DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(rows: usize, cols: usize, scrollback_limit: usize) -> Self {
        let mut inner = Terminal::new(dim(rows), dim(cols));
        // Mirror the old cap semantics; aterm enforces the line limit on its
        // tiered scrollback rather than a flat Vec.
        inner.set_scrollback_line_limit(Some(scrollback_limit));
        Self { inner }
    }

    /// Number of lines currently held in scrollback (off-screen above the grid).
    pub fn scrollback_len(&self) -> usize {
        self.inner.scrollback().map_or(0, |s| s.line_count())
    }

    /// Text of scrollback line `index` (0 = oldest), trailing blanks trimmed.
    pub fn scrollback_row_text(&self, index: usize) -> String {
        let Some(storage) = self.inner.scrollback() else {
            return String::new();
        };
        match storage.get_line(index) {
            Ok(Some(line)) => line.as_str().map(|s| s.trim_end().to_string()).unwrap_or_default(),
            _ => String::new(),
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
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.contains(CellFlags::UNDERLINE),
            inverse: flags.contains(CellFlags::INVERSE),
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
    fn resize_grow_preserves_content_and_widens() {
        let mut term = HeadlessTerminal::new(2, 5);
        term.process_str("top\r\nbot");
        term.resize(4, 8);
        assert_eq!(term.size(), (4, 8));
        assert_eq!(term.row_text(0), "top");
        assert_eq!(term.row_text(1), "bot");
    }
}
