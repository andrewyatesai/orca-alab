// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

//! `TerminalCheckpoint` — a scoped, round-trippable projection of a live
//! [`Terminal`] (GREEN-ORDER step 4 / design `HIERARCHICAL_SESSIONS.md` B.3.2).
//!
//! The live [`Terminal`] is **neither `Clone` nor `Serialize`**: it holds five
//! `Box<dyn FnMut>` callbacks, two `Instant`s, four host-auth state machines, an
//! `Option<PolicyEngine>`, and a live `Parser`. A byte-identical clone is
//! impossible by construction. A [`TerminalCheckpoint`] is therefore a *precise
//! projection with a documented exclusion list*, not a clone — see the EXCLUDED
//! and DEFERRED blocks below.
//!
//! This increment captures the buffer-state core: both grid bodies (full cell
//! fidelity via the `Line` codec), per-grid cursor / scroll-region / pending-wrap
//! / tab-stops, size, and a set of cheap `Copy`/snapshot leaf fields. The
//! round-trip is proven by the in-module property test (`mod tests`), which is
//! the ship gate for this step.
//
// ===========================================================================
// EXCLUDED (host bindings, re-bound here):
//   - the five callbacks (bell, cursor_style, buffer_activation, window,
//     text_sizing)
//   - policy_engine (Option<PolicyEngine>)
//   - live auth nonces / capabilities (clipboard_auth, shell_integration_auth,
//     hyperlink_auth, dcs_auth)
// These are HOST effects, not buffer state. They are re-bound by the host on
// `from_checkpoint` via `HostBindings`. For this increment `HostBindings::none()`
// installs the same defaults `Terminal::new` does; real callbacks/policy/auth
// rebinding lands in later work. Callback-driven side effects (OSC 9/99
// notifications, OSC 52 clipboard writes, window ops, bell fires) are NOT
// replayed — they are host effects, not state.
//
// DEFERRED (later stages — NOT captured in this increment, and why):
//   - grouped sub-projections: color, transient, cursor_save, shell, marks,
//     semantic, iterm2, vi, text_selection — each needs its own per-field Repr
//     (palette stacks, SGR stack, DECSC slots, OSC133 blocks, …) and is out of
//     scope for the buffer-core increment.
//   - sixel: the decoded image store needs a lossy-edge Repr (B.4).
//   - clock-domain fields: bell_ticks (last_bell_time), sync_ticks (sync_start),
//     and rate_limiter (token bucket) — these require a Clock seam mapping
//     Instant -> Ticks that does not exist yet; capturing them faithfully on a
//     forked timeline is the explicit B.4 must-fix, separate from this step.
//   - serde: the checkpoint stores leaf engine types BY VALUE and relies on
//     `PartialEq` for the round-trip gate; on-the-wire serialization (the
//     `grid: Vec<u8>` bytes are already serde-ready) is a later concern.
//   - current_style_id is intentionally NOT carried: it indexes a per-grid
//     style interner. On restore we set `style` and call `update_style_id()` to
//     re-intern against the rebuilt grid (see `from_checkpoint`).
// ===========================================================================

use aterm_types::charset::CharacterSetState;
use aterm_types::{KittyKeyboardStateSnapshot, TaskbarProgress, XtermKeyboardState};

use super::Terminal;
use super::types::{CurrentStyle, TerminalModes};
use crate::grid::{CellFlags, Grid, PackedColor};
use crate::scrollback::{Scrollback, deserialize_lines, serialize_lines};

/// Host-side bindings re-installed on `from_checkpoint`.
///
/// A checkpoint deliberately omits the live `Terminal`'s callbacks, policy
/// engine, and auth nonces (see the EXCLUDED block at the top of this module).
/// `HostBindings` is where the host re-supplies them. For this increment it is
/// intentionally empty (all `None`); real fields are added as the rebinding
/// work lands. `HostBindings::none()` is enough to hydrate a fully-living,
/// introspectable `Terminal` whose buffer state matches the source exactly.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct HostBindings {
    // Placeholder for the five callbacks / policy engine / auth state that a
    // host re-binds. All `None`/empty in this increment; documented as deferred.
    _private: (),
}

impl HostBindings {
    /// A null/empty set of host bindings.
    ///
    /// Installs the same host-effect defaults as `Terminal::new` (no callbacks,
    /// no policy engine, default auth posture).
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// Per-grid cursor + region + wrap + tab-stop projection.
///
/// Captured independently for the main grid and (when present) the alt grid,
/// because each carries its own cursor and scroll region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCursorRepr {
    /// Cursor row (0-based, grid-relative).
    pub cursor_row: u16,
    /// Cursor column (0-based, grid-relative).
    pub cursor_col: u16,
    /// Deferred-wrap (pending-wrap / wrap-next) flag.
    pub pending_wrap: bool,
    /// DECSTBM scroll-region top (inclusive).
    pub scroll_top: u16,
    /// DECSTBM scroll-region bottom (inclusive).
    pub scroll_bottom: u16,
    /// Per-column tab stops (`true` = stop set at that column).
    pub tab_stops: Vec<bool>,
}

impl GridCursorRepr {
    fn capture(grid: &Grid) -> Self {
        let region = grid.scroll_region();
        Self {
            cursor_row: grid.cursor_row(),
            cursor_col: grid.cursor_col(),
            pending_wrap: grid.pending_wrap(),
            scroll_top: region.top,
            scroll_bottom: region.bottom,
            tab_stops: grid.tab_stops().to_vec(),
        }
    }

    fn apply(&self, grid: &mut Grid) {
        grid.set_scroll_region(self.scroll_top, self.scroll_bottom);
        grid.set_cursor(self.cursor_row, self.cursor_col);
        grid.set_pending_wrap(self.pending_wrap);
        grid.restore_tab_stops(&self.tab_stops);
    }
}

/// Minimal, by-value style projection.
///
/// `CurrentStyle` carries cached fields that are pure functions of the four
/// semantically-meaningful inputs `(fg, bg, flags, protected)`, so we capture
/// only those and rebuild via `CurrentStyle::new(...)` on restore. This both
/// avoids depending on `PartialEq` for `CurrentStyle`'s private cache and keeps
/// the round-trip honest (the rebuilt cache is recomputed, then `style` is
/// re-interned by `update_style_id()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleRepr {
    /// Foreground color.
    pub fg: PackedColor,
    /// Background color.
    pub bg: PackedColor,
    /// SGR cell flags (bold, italic, underline, …).
    pub flags: CellFlags,
    /// DECSCA selective-erase protection.
    pub protected: bool,
}

impl StyleRepr {
    fn capture(style: &CurrentStyle) -> Self {
        Self {
            fg: style.fg,
            bg: style.bg,
            flags: style.flags,
            protected: style.protected,
        }
    }

    fn into_style(self) -> CurrentStyle {
        CurrentStyle::new(self.fg, self.bg, self.flags, self.protected)
    }
}

/// A scoped, round-trippable projection of a live [`Terminal`] (B.3.2).
///
/// Equality is *structural*: `checkpoint() == from_checkpoint(&c).checkpoint()`
/// is the re-checkpoint identity proven by the round-trip test. The grid bodies
/// are stored as `serialize_lines`-encoded bytes (scrollback-then-visible); all
/// other captured fields are stored by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCheckpoint {
    /// Grid rows (visible).
    pub rows: u16,
    /// Grid cols.
    pub cols: u16,
    /// Main-grid body: `serialize_lines(grid.checkpoint_lines())`
    /// (scrollback-then-visible).
    pub grid: Vec<u8>,
    /// Main-grid cursor/region/wrap/tab projection.
    pub cursor: GridCursorRepr,
    /// Alt-grid body, if an alt grid exists.
    pub alt_grid: Option<Vec<u8>>,
    /// Alt-grid cursor/region/wrap/tab projection, if an alt grid exists.
    pub alt_cursor: Option<GridCursorRepr>,
    /// Terminal modes (Copy; ~40 boolean/enum fields).
    pub modes: TerminalModes,
    /// Current SGR style (minimal by-value projection).
    pub style: StyleRepr,
    /// Character set state (G0-G3, GL, GR, single-shift).
    pub charset: CharacterSetState,
    /// Kitty keyboard protocol state (via snapshot/restore).
    pub kitty_keyboard: KittyKeyboardStateSnapshot,
    /// xterm keyboard modifier/format options (XTMODKEYS/XTFMTKEYS).
    pub xterm_keyboard: XtermKeyboardState,
    /// Taskbar progress (ConEmu OSC 9;4).
    pub taskbar_progress: Option<TaskbarProgress>,
    /// Secure keyboard entry mode.
    pub secure_keyboard_entry: bool,
    /// Current working directory (OSC 7).
    pub current_working_directory: Option<String>,
    /// Parser was in Ground state at capture time (B.3.3 invariant).
    pub parser_ground: bool,
}

impl Terminal {
    /// Capture a [`TerminalCheckpoint`] — a pure read, no host effects, no fs.
    ///
    /// The parser MUST be in Ground state (B.3.3): a checkpoint taken
    /// mid-sequence would silently lose the parser's partial state (which is not
    /// in the projection). This is `debug_assert`ed.
    #[must_use]
    pub fn checkpoint(&self) -> TerminalCheckpoint {
        debug_assert!(
            self.parser_is_ground(),
            "checkpoint() requires parser_is_ground() (B.3.3)"
        );

        let grid_bytes = serialize_lines(&self.grid.checkpoint_lines());
        let cursor = GridCursorRepr::capture(&self.grid);

        let (alt_grid, alt_cursor) = match &self.alt_grid {
            Some(alt) => (
                Some(serialize_lines(&alt.checkpoint_lines())),
                Some(GridCursorRepr::capture(alt)),
            ),
            None => (None, None),
        };

        TerminalCheckpoint {
            rows: self.grid.rows(),
            cols: self.grid.cols(),
            grid: grid_bytes,
            cursor,
            alt_grid,
            alt_cursor,
            modes: self.modes,
            style: StyleRepr::capture(&self.style),
            charset: self.charset,
            kitty_keyboard: self.kitty_keyboard.snapshot(),
            xterm_keyboard: self.xterm_keyboard,
            taskbar_progress: self.taskbar_progress,
            secure_keyboard_entry: self.secure_keyboard_entry,
            current_working_directory: self.current_working_directory.clone(),
            parser_ground: self.parser_is_ground(),
        }
    }

    /// Rebuild a fully-living [`Terminal`] from a checkpoint, re-binding host
    /// effects via `host` (B.3.2).
    ///
    /// The rebuilt terminal's *buffer state* matches the source exactly (proven
    /// by the round-trip test); host bindings (callbacks/policy/auth) are NOT
    /// from the checkpoint — they come from `host` (see the EXCLUDED block).
    #[must_use]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "from_checkpoint takes ownership of host bindings (callbacks/policy/auth) \
                  and installs them; this increment's HostBindings is empty, but the \
                  by-value signature is the stable rebinding seam (B.3.2) and must not \
                  churn when real Box<dyn FnMut> fields land"
    )]
    pub fn from_checkpoint(c: &TerminalCheckpoint, host: HostBindings) -> Terminal {
        // `host` is intentionally consumed even though this increment's
        // HostBindings is empty: it pins the API so callers wire rebinding here
        // and the signature does not churn when real fields land.
        let _ = host;

        let main_grid = restore_grid(c.rows, c.cols, &c.grid, &c.cursor);
        let mut terminal = Terminal::with_grid(main_grid);

        // Alt grid (same restore path), if present.
        if let (Some(alt_bytes), Some(alt_cursor)) = (&c.alt_grid, &c.alt_cursor) {
            terminal.alt_grid = Some(restore_grid(c.rows, c.cols, alt_bytes, alt_cursor));
        }

        // Leaf fields by value.
        terminal.modes = c.modes;
        terminal.charset = c.charset;
        terminal.kitty_keyboard.restore_snapshot(c.kitty_keyboard);
        terminal.xterm_keyboard = c.xterm_keyboard;
        terminal.taskbar_progress = c.taskbar_progress;
        terminal.secure_keyboard_entry = c.secure_keyboard_entry;
        terminal
            .current_working_directory
            .clone_from(&c.current_working_directory);

        // Style: set the semantic style, then RE-INTERN against the rebuilt
        // grid's interner (do NOT carry current_style_id — it is a per-grid
        // index). update_style_id() refreshes the cached colors + StyleId.
        terminal.style = c.style.into_style();
        {
            // `sgr_style().update_style_id()` re-interns `style` against the
            // grid's StyleTable. We reach it via the generated handler split.
            let (_parser, mut handler) = terminal.split_for_process();
            handler.sgr_style().update_style_id();
        }

        terminal
    }
}

/// Rebuild a single grid from `serialize_lines` bytes + a cursor projection.
///
/// The byte stream is `scrollback-then-visible` (the `checkpoint_lines` layout):
/// the last `rows` lines are the visible rows; everything before is scrollback,
/// oldest first. We attach the scrollback to a tiered store (preserving order),
/// then restore the visible rows via the shared `fill_row_from_line` path, then
/// apply the cursor/region/wrap/tabs.
fn restore_grid(rows: u16, cols: u16, bytes: &[u8], cursor: &GridCursorRepr) -> Grid {
    let lines = deserialize_lines(bytes);
    let visible_start = lines.len().saturating_sub(rows as usize);
    let (scrollback_lines, visible_lines) = lines.split_at(visible_start);

    let mut scrollback = Scrollback::with_defaults();
    // Don't let the default line cap silently drop restored history.
    scrollback.set_line_limit(None);
    for line in scrollback_lines {
        // push_line is infallible for the in-memory tier.
        scrollback.push_line(line.clone());
    }

    let mut grid = Grid::with_tiered_scrollback(rows, cols, 1000, scrollback);
    grid.restore_visible_from_lines(visible_lines);
    cursor.apply(&mut grid);
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive a fresh terminal with a byte stream that exercises EVERY captured
    /// field, leaving the parser in Ground state.
    fn build_rich_terminal(rows: u16, cols: u16) -> Terminal {
        let mut t = Terminal::new(rows, cols);

        // --- text + SGR: bold + underline + 256-color fg + truecolor bg ---
        t.process(b"\x1b[1;4;38;5;202;48;2;10;20;30mstyled\x1b[0m\r\n");

        // --- plain lines, more than `rows` so scrollback fills ---
        for i in 0..(rows as usize + 6) {
            t.process(format!("line{i}\r\n").as_bytes());
        }

        // --- DECSTBM scroll region (rows 2..rows-1, 1-based) ---
        t.process(format!("\x1b[2;{}r", rows - 1).as_bytes());

        // --- cursor move + pending-wrap: write the last column on a row ---
        t.process(b"\x1b[3;1H"); // row 3
        let last_col_fill: String = std::iter::repeat('Z').take(cols as usize).collect();
        t.process(last_col_fill.as_bytes()); // fills to last col -> pending_wrap set

        // --- tab stops: clear all then set one via HTS ---
        t.process(b"\x1b[4;1H"); // move somewhere safe
        t.process(b"\x1b[3g"); // TBC 3: clear all tab stops
        t.process(b"\x1b[1;5H\x1bH"); // move to col 5, HTS sets a tab stop here

        // --- charset designation: G0 = DEC special graphics ---
        t.process(b"\x1b(0");

        // --- kitty keyboard: push flags ---
        t.process(b"\x1b[>5u");

        // --- XTMODKEYS (xterm keyboard) ---
        t.process(b"\x1b[>4;2m");

        // --- taskbar progress (ConEmu OSC 9;4): this engine has NO byte-stream
        //     handler for OSC 9;4 (handle_osc_9 explicitly ignores `9;4;...`;
        //     the field is otherwise only cleared by reset). It is a host-set
        //     leaf, so we set it directly to guarantee a non-default captured
        //     value and exercise its capture/restore path. Honest note: this
        //     field round-trips by value but is not driver-reachable today. ---
        t.taskbar_progress = Some(TaskbarProgress::Normal(42));

        // --- secure keyboard entry: also host-set (no OSC); use the setter ---
        t.set_secure_keyboard_entry(true);

        // --- OSC 7 cwd ---
        t.process(b"\x1b]7;file://host/tmp/work\x07");

        // --- alt screen: enter 1049, write into alt, then we keep alt active ---
        t.process(b"\x1b[?1049h");
        t.process(b"ALT-SCREEN-CONTENT\r\n");

        assert!(
            t.parser_is_ground(),
            "test stream must leave parser in Ground state"
        );
        t
    }

    /// Read a cell's (char, flags, fg, bg) at (row, col) on the ACTIVE grid.
    fn cell_signature(
        t: &Terminal,
        row: u16,
        col: u16,
    ) -> (char, CellFlags, PackedColor, PackedColor) {
        let cell = *t
            .grid()
            .row(row)
            .and_then(|r| r.get(col))
            .expect("cell in range");
        (
            cell.char(),
            cell.flags(),
            cell.fg_color().unwrap_or(PackedColor::DEFAULT_FG),
            cell.bg_color().unwrap_or(PackedColor::DEFAULT_BG),
        )
    }

    #[test]
    fn checkpoint_roundtrip_full_projection() {
        let (rows, cols) = (12u16, 40u16);
        let t = build_rich_terminal(rows, cols);

        // Sanity: we actually captured non-default state.
        assert!(t.modes.alternate_screen, "alt screen active at capture");
        assert!(t.secure_keyboard_entry, "secure input captured");
        assert!(t.taskbar_progress.is_some(), "taskbar captured");
        assert!(t.current_working_directory.is_some(), "cwd captured");
        assert!(
            t.alt_grid.is_some(),
            "alt grid present (main saved under alt)"
        );

        let c0 = t.checkpoint();
        let h = Terminal::from_checkpoint(&c0, HostBindings::none());
        let c1 = h.checkpoint();

        // (A) re-checkpoint equality — the ship gate.
        assert_eq!(c0, c1, "re-checkpoint equality (c0 == c1)");

        // (B) rendered content equality.
        assert_eq!(
            t.visible_content(),
            h.visible_content(),
            "visible_content equal"
        );
        for r in 0..rows as usize {
            assert_eq!(t.row_text(r), h.row_text(r), "row_text equal for row {r}");
        }

        // cursor equality.
        assert_eq!(t.cursor(), h.cursor(), "cursor equal");

        // scroll region + modes + charset equality.
        assert_eq!(
            t.grid().scroll_region(),
            h.grid().scroll_region(),
            "scroll region equal"
        );
        assert_eq!(t.modes, h.modes, "modes equal");
        assert_eq!(t.charset, h.charset, "charset equal");
        assert_eq!(t.taskbar_progress, h.taskbar_progress, "taskbar equal");
        assert_eq!(
            t.secure_keyboard_entry, h.secure_keyboard_entry,
            "secure input equal"
        );
        assert_eq!(
            t.current_working_directory, h.current_working_directory,
            "cwd equal"
        );
        assert_eq!(t.xterm_keyboard, h.xterm_keyboard, "xterm keyboard equal");
        assert_eq!(
            t.kitty_keyboard.snapshot(),
            h.kitty_keyboard.snapshot(),
            "kitty keyboard equal"
        );
        assert_eq!(
            t.grid().tab_stops(),
            h.grid().tab_stops(),
            "tab stops equal"
        );
        assert_eq!(
            t.grid().pending_wrap(),
            h.grid().pending_wrap(),
            "pending wrap equal"
        );
    }

    #[test]
    fn checkpoint_post_hydration_styled_write_matches() {
        // Proves current_style_id was correctly recomputed: a styled write on
        // both the source and the hydrated terminal must produce identical
        // cells. (If from_checkpoint had carried a stale StyleId, the hydrated
        // write would intern against the wrong table and diverge.)
        let (rows, cols) = (10u16, 30u16);
        let mut t = build_rich_terminal(rows, cols);
        let c0 = t.checkpoint();
        let mut h = Terminal::from_checkpoint(&c0, HostBindings::none());

        // Same styled write to both (move home, set a fresh distinctive style).
        let seq = b"\x1b[H\x1b[1;3;38;2;1;2;3;48;5;9mQ\x1b[0m";
        t.process(seq);
        h.process(seq);

        assert!(t.parser_is_ground() && h.parser_is_ground());

        let ts = cell_signature(&t, 0, 0);
        let hs = cell_signature(&h, 0, 0);
        assert_eq!(
            ts, hs,
            "post-hydration styled cell identical (char, flags, fg, bg)"
        );

        // And re-checkpoints still agree after the identical follow-on writes.
        assert_eq!(
            t.checkpoint(),
            h.checkpoint(),
            "post-write re-checkpoint equality"
        );
    }

    #[test]
    fn checkpoint_alt_screen_toggle_on_hydrated() {
        // Toggle alt-screen OFF on the hydrated terminal and confirm it tracks
        // the source doing the same — the main grid (saved under alt at capture)
        // must come back identically.
        let (rows, cols) = (10u16, 30u16);
        let mut t = build_rich_terminal(rows, cols);
        let c0 = t.checkpoint();
        let mut h = Terminal::from_checkpoint(&c0, HostBindings::none());

        // Exit alt screen on both.
        t.process(b"\x1b[?1049l");
        h.process(b"\x1b[?1049l");

        assert!(!t.modes.alternate_screen && !h.modes.alternate_screen);
        assert_eq!(
            t.visible_content(),
            h.visible_content(),
            "main screen restored identically after alt toggle"
        );
        for r in 0..rows as usize {
            assert_eq!(
                t.row_text(r),
                h.row_text(r),
                "row {r} equal after alt toggle"
            );
        }
        assert_eq!(
            t.checkpoint(),
            h.checkpoint(),
            "re-checkpoint equal after alt toggle"
        );
    }

    #[test]
    fn checkpoint_no_alt_grid_when_absent() {
        // A plain terminal that never entered alt screen has no alt grid in the
        // checkpoint, and still round-trips.
        let mut t = Terminal::new(6, 20);
        t.process(b"hello\r\nworld\r\n");
        assert!(t.parser_is_ground());
        let c0 = t.checkpoint();
        assert!(c0.alt_grid.is_none(), "no alt grid captured");
        assert!(c0.alt_cursor.is_none());

        let h = Terminal::from_checkpoint(&c0, HostBindings::none());
        assert_eq!(c0, h.checkpoint(), "re-checkpoint equal (no alt)");
        assert_eq!(t.visible_content(), h.visible_content());
        assert!(h.alt_grid.is_none(), "hydrated has no alt grid");
    }
}
