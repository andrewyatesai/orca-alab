//! A headless terminal grid driven by the vte ANSI parser — the core of the
//! `@xterm/headless` replacement: a server-side screen + cursor that tracks the
//! working directory via OSC-7, suitable for snapshot/replay across reconnect
//! and SSH.
//!
//! This is intentionally a focused subset (print, CR/LF/BS/HT, line scroll,
//! OSC-7 cwd) — the foundation the full `aterm` engine extends (scrollback,
//! SGR attributes, mouse modes, full DECSET handling).

use vte::{Params, Parser, Perform};

/// Maintains the visible grid + cursor + cwd. Implements vte's `Perform`.
struct Screen {
    rows: usize,
    cols: usize,
    grid: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    cwd: Option<String>,
    pen: CellAttrs,
    /// Lines that have scrolled off the top, oldest first, bounded to
    /// `scrollback_limit`.
    scrollback: Vec<Vec<Cell>>,
    scrollback_limit: usize,
    mouse_tracking: MouseTracking,
    sgr_mouse: bool,
    sgr_pixels: bool,
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

impl Screen {
    fn new(rows: usize, cols: usize, scrollback_limit: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Self {
            rows,
            cols,
            grid: vec![vec![Cell::default(); cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
            cwd: None,
            pen: CellAttrs::default(),
            scrollback: Vec::new(),
            scrollback_limit,
            mouse_tracking: MouseTracking::None,
            sgr_mouse: false,
            sgr_pixels: false,
        }
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        if self.scrollback_limit == 0 {
            return;
        }
        if self.scrollback.len() >= self.scrollback_limit {
            self.scrollback.remove(0); // drop oldest
        }
        self.scrollback.push(line);
    }

    fn line_feed(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            // Scroll up: the top line moves into scrollback; append a blank one.
            let evicted = self.grid.remove(0);
            self.push_scrollback(evicted);
            self.grid.push(vec![Cell::default(); self.cols]);
            self.cursor_row = self.rows - 1;
        } else {
            self.cursor_row += 1;
        }
    }

    fn capture(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            rows: self.rows,
            cols: self.cols,
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            cwd: self.cwd.clone(),
            lines: self.grid.iter().map(|row| row.iter().map(|cell| cell.ch).collect()).collect(),
        }
    }

    fn from_snapshot(snapshot: &TerminalSnapshot) -> Self {
        let rows = snapshot.rows.max(1);
        let cols = snapshot.cols.max(1);
        let mut grid = vec![vec![Cell::default(); cols]; rows];
        for (i, line) in snapshot.lines.iter().take(rows).enumerate() {
            for (j, ch) in line.chars().take(cols).enumerate() {
                grid[i][j] = Cell { ch, attrs: CellAttrs::default() };
            }
        }
        Self {
            rows,
            cols,
            grid,
            cursor_row: snapshot.cursor_row.min(rows - 1),
            cursor_col: snapshot.cursor_col.min(cols),
            cwd: snapshot.cwd.clone(),
            pen: CellAttrs::default(),
            scrollback: Vec::new(),
            scrollback_limit: DEFAULT_SCROLLBACK,
            mouse_tracking: MouseTracking::None,
            sgr_mouse: false,
            sgr_pixels: false,
        }
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        for row in &mut self.grid {
            row.resize(cols, Cell::default());
        }
        if self.grid.len() > rows {
            // Shrinking: keep the most recent (bottom) lines.
            let excess = self.grid.len() - rows;
            self.grid.drain(0..excess);
            self.cursor_row = self.cursor_row.saturating_sub(excess);
        } else {
            while self.grid.len() < rows {
                self.grid.push(vec![Cell::default(); cols]);
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows - 1);
        self.cursor_col = self.cursor_col.min(cols);
    }
}

/// A serializable snapshot of the terminal state, for reconnect / SSH replay
/// (the role of `@xterm/addon-serialize`). `lines` holds the full grid, one
/// `cols`-wide string per row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub rows: usize,
    pub cols: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cwd: Option<String>,
    pub lines: Vec<String>,
}

impl Perform for Screen {
    fn print(&mut self, c: char) {
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.line_feed();
        }
        self.grid[self.cursor_row][self.cursor_col] = Cell { ch: c, attrs: self.pen };
        self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.line_feed(),       // LF
            b'\r' => self.cursor_col = 0,    // CR
            0x08 => self.cursor_col = self.cursor_col.saturating_sub(1), // BS
            b'\t' => {
                // Advance to the next 8-column tab stop, clamped to the width.
                let next = ((self.cursor_col / 8) + 1) * 8;
                self.cursor_col = next.min(self.cols - 1);
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // OSC 7 ; file://<host>/<path>  → current working directory.
        if params.len() >= 2 && params[0] == b"7" {
            if let Some(path) = parse_osc7_file_uri(params[1]) {
                self.cwd = Some(path);
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        if action == 'm' && intermediates.is_empty() {
            self.apply_sgr(params);
            return;
        }
        // DECSET/DECRST: `CSI ? <modes> h|l`.
        if intermediates.first() == Some(&b'?') && (action == 'h' || action == 'l') {
            let set = action == 'h';
            for sub in params.iter() {
                if let Some(&mode) = sub.first() {
                    self.apply_private_mode(mode, set);
                }
            }
        }
    }
}

impl Screen {
    /// Apply an SGR (`CSI … m`) sequence to the current pen: reset,
    /// bold/italic/underline/inverse (+ resets), 16-color and bright fg/bg,
    /// and extended `38/48;5;n` (256) / `38/48;2;r;g;b` (truecolor). Values are
    /// flattened across both `;`-params and `:`-subparams, so either separator
    /// form parses identically. No params == reset.
    fn apply_sgr(&mut self, params: &Params) {
        let mut codes: Vec<u16> = Vec::new();
        for sub in params.iter() {
            if sub.is_empty() {
                codes.push(0);
            } else {
                codes.extend_from_slice(sub);
            }
        }
        if codes.is_empty() {
            self.pen = CellAttrs::default(); // bare `CSI m` == `CSI 0 m`
            return;
        }

        let mut i = 0;
        while i < codes.len() {
            match codes[i] {
                0 => self.pen = CellAttrs::default(),
                1 => self.pen.bold = true,
                3 => self.pen.italic = true,
                4 => self.pen.underline = true,
                7 => self.pen.inverse = true,
                22 => self.pen.bold = false,
                23 => self.pen.italic = false,
                24 => self.pen.underline = false,
                27 => self.pen.inverse = false,
                code @ 30..=37 => self.pen.fg = Color::Indexed((code - 30) as u8),
                38 => {
                    if let Some((color, consumed)) = parse_extended_color(&codes[i + 1..]) {
                        self.pen.fg = color;
                        i += consumed;
                    }
                }
                39 => self.pen.fg = Color::Default,
                code @ 40..=47 => self.pen.bg = Color::Indexed((code - 40) as u8),
                48 => {
                    if let Some((color, consumed)) = parse_extended_color(&codes[i + 1..]) {
                        self.pen.bg = color;
                        i += consumed;
                    }
                }
                49 => self.pen.bg = Color::Default,
                code @ 90..=97 => self.pen.fg = Color::Indexed((code - 90 + 8) as u8),
                code @ 100..=107 => self.pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }

    /// Apply a DECSET/DECRST private mode (mouse-reporting subset).
    fn apply_private_mode(&mut self, mode: u16, set: bool) {
        match mode {
            9 => self.mouse_tracking = if set { MouseTracking::X10 } else { MouseTracking::None },
            1000 => self.mouse_tracking = if set { MouseTracking::Normal } else { MouseTracking::None },
            1002 => self.mouse_tracking = if set { MouseTracking::Button } else { MouseTracking::None },
            1003 => self.mouse_tracking = if set { MouseTracking::Any } else { MouseTracking::None },
            1006 => self.sgr_mouse = set,
            1016 => self.sgr_pixels = set,
            _ => {}
        }
    }
}

/// Parse the `file://<host>/<path>` payload of OSC 7 into a path, percent-decoded.
fn parse_osc7_file_uri(value: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(value).ok()?;
    let rest = text.strip_prefix("file://")?;
    // Drop the authority (host); the path begins at the first '/'.
    let path = match rest.find('/') {
        Some(idx) => &rest[idx..],
        None => return None,
    };
    Some(percent_decode(path))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse an extended SGR color tail (after `38`/`48`): `5;<n>` (256-palette) or
/// `2;<r>;<g>;<b>` (truecolor). Returns the color and how many values it
/// consumed beyond the `38`/`48` introducer.
fn parse_extended_color(rest: &[u16]) -> Option<(Color, usize)> {
    match rest.first()? {
        5 => Some((Color::Indexed(*rest.get(1)? as u8), 2)),
        2 => {
            let r = *rest.get(1)? as u8;
            let g = *rest.get(2)? as u8;
            let b = *rest.get(3)? as u8;
            Some((Color::Rgb(r, g, b), 4))
        }
        _ => None,
    }
}

/// Headless terminal: feed it PTY output bytes, read back the grid / cursor /
/// cwd. The `Parser` and `Screen` are separate fields so `advance` can borrow
/// both mutably.
pub struct HeadlessTerminal {
    parser: Parser,
    screen: Screen,
}

impl HeadlessTerminal {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self::with_scrollback(rows, cols, DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(rows: usize, cols: usize, scrollback_limit: usize) -> Self {
        Self { parser: Parser::new(), screen: Screen::new(rows, cols, scrollback_limit) }
    }

    /// Number of lines currently held in scrollback (off-screen above the grid).
    pub fn scrollback_len(&self) -> usize {
        self.screen.scrollback.len()
    }

    /// Text of scrollback line `index` (0 = oldest), trailing blanks trimmed.
    pub fn scrollback_row_text(&self, index: usize) -> String {
        self.screen
            .scrollback
            .get(index)
            .map(|line| line.iter().map(|cell| cell.ch).collect::<String>().trim_end().to_string())
            .unwrap_or_default()
    }

    /// Feed raw output bytes through the parser into the grid.
    pub fn process(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.parser.advance(&mut self.screen, byte);
        }
    }

    pub fn process_str(&mut self, text: &str) {
        self.process(text.as_bytes());
    }

    /// A row's text with trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        self.screen
            .grid
            .get(row)
            .map(|line| line.iter().map(|cell| cell.ch).collect::<String>().trim_end().to_string())
            .unwrap_or_default()
    }

    /// The cell at `(row, col)`, including its SGR attributes.
    pub fn cell(&self, row: usize, col: usize) -> Option<Cell> {
        self.screen.grid.get(row).and_then(|line| line.get(col)).copied()
    }

    /// Current mouse-reporting mode (set via DECSET).
    pub fn mouse_tracking(&self) -> MouseTracking {
        self.screen.mouse_tracking
    }
    /// Whether SGR mouse encoding (DECSET 1006) is on.
    pub fn sgr_mouse(&self) -> bool {
        self.screen.sgr_mouse
    }
    /// Whether SGR pixel mouse encoding (DECSET 1016) is on.
    pub fn sgr_pixels(&self) -> bool {
        self.screen.sgr_pixels
    }

    /// All rows, trailing blanks trimmed (a minimal snapshot).
    pub fn snapshot(&self) -> Vec<String> {
        (0..self.screen.rows).map(|row| self.row_text(row)).collect()
    }

    /// `(row, col)` cursor position.
    pub fn cursor(&self) -> (usize, usize) {
        (self.screen.cursor_row, self.screen.cursor_col)
    }

    pub fn cwd(&self) -> Option<&str> {
        self.screen.cwd.as_deref()
    }

    pub fn size(&self) -> (usize, usize) {
        (self.screen.rows, self.screen.cols)
    }

    /// Capture a serializable snapshot for reconnect / SSH replay.
    pub fn capture(&self) -> TerminalSnapshot {
        self.screen.capture()
    }

    /// Rebuild a terminal from a snapshot (parser starts fresh).
    pub fn from_snapshot(snapshot: &TerminalSnapshot) -> Self {
        Self { parser: Parser::new(), screen: Screen::from_snapshot(snapshot) }
    }

    /// Resize the grid (client viewport change). Shrinking keeps the most
    /// recent lines; growing pads with blanks.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.screen.resize(rows, cols);
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
    fn sgr_colon_subparam_form_parses_like_semicolon() {
        let mut term = HeadlessTerminal::new(1, 5);
        term.process_str("\x1b[38:5:42mX");
        assert_eq!(term.cell(0, 0).unwrap().attrs.fg, Color::Indexed(42));
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
        term.process_str("\x1b[?1006l");
        assert!(!term.sgr_mouse());
    }

    #[test]
    fn scrolled_off_lines_go_to_scrollback() {
        let mut term = HeadlessTerminal::new(2, 5);
        term.process_str("a\r\nb\r\nc\r\nd");
        // visible = last two; scrollback = the two evicted from the top
        assert_eq!(term.snapshot(), vec!["c".to_string(), "d".to_string()]);
        assert_eq!(term.scrollback_len(), 2);
        assert_eq!(term.scrollback_row_text(0), "a");
        assert_eq!(term.scrollback_row_text(1), "b");
    }

    #[test]
    fn scrollback_is_bounded_and_drops_oldest() {
        let mut term = HeadlessTerminal::with_scrollback(1, 5, 2);
        // 1-row grid: every newline evicts the current line into scrollback.
        term.process_str("1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(term.scrollback_len(), 2); // capped at 2
        assert_eq!(term.scrollback_row_text(0), "3"); // oldest retained
        assert_eq!(term.scrollback_row_text(1), "4");
        assert_eq!(term.row_text(0), "5"); // current visible line
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
    fn resize_shrink_keeps_most_recent_lines() {
        let mut term = HeadlessTerminal::new(2, 5);
        term.process_str("top\r\nbot");
        term.resize(1, 8); // keep the most recent (bottom) line
        assert_eq!(term.size(), (1, 8));
        assert_eq!(term.row_text(0), "bot");
    }
}
