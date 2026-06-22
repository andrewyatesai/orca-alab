// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! In-tab SPLIT-PANE tree (iTerm2-style panes within one tab).
//!
//! A window has TABS (the existing [`crate::TabIndex`] + `Vec<Session>` model);
//! each TAB now owns a binary [`PaneTree`] of live sessions. A fresh tab is a
//! single [`PaneNode::Leaf`] — the exact one-session-per-tab behavior, so with no
//! splits the geometry is byte-identical to before. Splitting the FOCUSED leaf
//! turns it into a [`PaneNode::Split`] of two leaves (the original session and a
//! freshly-spawned sibling), and closing the focused leaf collapses its parent.
//!
//! This module is PURE GEOMETRY + tree bookkeeping over session ids (`u64`, the
//! same stable ids [`crate::Session::id`] routes `Wake`s with). It owns no
//! `Terminal`, no PTY, and no rendering — the GUI ([`crate::App`]) maps the
//! [`PaneRect`]s this produces back onto the live `Vec<Session>` and composes
//! their per-pane snapshots into the one window frame. Keeping it headless makes
//! the layout math (and the split/close/focus state machine) unit-testable with
//! no window, PTY, or event loop, mirroring [`crate::TabIndex`].
//!
//! DIVIDERS: a split reserves ONE cell line between its children (drawn by the
//! GUI). The `ratio` is the FIRST child's fraction of the splittable extent
//! (everything but the 1-cell divider); MVP always splits 50/50, but the ratio is
//! stored per-split so a later divider drag is a pure data edit (no structural
//! change). Each child is clamped to at least 1 cell so a tiny window never yields
//! a 0-extent pane.

/// Which way a [`PaneNode::Split`] divides its rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitDir {
    /// Children sit SIDE BY SIDE, split by a vertical divider (Cmd-D). The first
    /// child is the LEFT pane, the second the RIGHT; the columns are divided.
    Vertical,
    /// Children are STACKED, split by a horizontal divider (Cmd-Shift-D). The
    /// first child is the TOP pane, the second the BOTTOM; the rows are divided.
    Horizontal,
}

/// One node of a tab's binary pane tree: either a live session (`Leaf`) or a
/// split of two sub-trees. Sessions are referenced by their stable `u64` id (NOT
/// a `Vec` index, which shifts when an earlier pane/tab closes), exactly like
/// [`crate::Session::id`].
#[derive(Clone, Debug, PartialEq)]
pub enum PaneNode {
    /// A live terminal session occupying this whole rectangle.
    Leaf { session: u64 },
    /// A divider splitting this rectangle into `first` then `second` (left/top
    /// then right/bottom), with `ratio` the first child's fraction of the
    /// splittable extent (the cells left after the 1-cell divider).
    Split {
        dir: SplitDir,
        /// First child's fraction of the splittable extent, in `(0.0, 1.0)`.
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

/// One visible pane's placement in the window grid, in CELL coordinates. The GUI
/// locks that session's `Terminal`, snapshots it at `(rows, cols)`, and blits the
/// cells into the composite window frame at `(row_off, col_off)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaneRect {
    /// The session occupying this rect (`Leaf::session`).
    pub session: u64,
    /// Top-left cell row offset of this pane within the window grid.
    pub row_off: u16,
    /// Top-left cell column offset of this pane within the window grid.
    pub col_off: u16,
    /// This pane's height in cells (`>= 1`).
    pub rows: u16,
    /// This pane's width in cells (`>= 1`).
    pub cols: u16,
}

/// A tab's pane layout: the binary tree plus the id of the FOCUSED leaf (the pane
/// that keyboard input + the control socket target, and whose cursor draws solid).
/// Every tab owns one; a fresh tab is a single leaf focused on its own session.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneTree {
    root: PaneNode,
    /// The session id of the focused leaf. Always references a leaf that exists in
    /// `root` (maintained by `split`/`close`); used to route input + draw the solid
    /// cursor in exactly one pane.
    focus: u64,
}

/// The first child's default fraction of the splittable extent: a 50/50 split. A
/// later divider drag adjusts a [`PaneNode::Split::ratio`]; the MVP fixes it here.
const DEFAULT_RATIO: f32 = 0.5;

impl PaneTree {
    /// A new single-pane tab holding `session` (the day-one one-session-per-tab
    /// layout). Focus is that one session.
    #[must_use]
    pub fn new(session: u64) -> Self {
        PaneTree {
            root: PaneNode::Leaf { session },
            focus: session,
        }
    }

    /// The currently FOCUSED session id (the pane keyboard input + the control
    /// socket target). Always a live leaf.
    #[must_use]
    pub fn focus(&self) -> u64 {
        self.focus
    }

    /// Move focus to `session` if it is a leaf in this tab. No-op (returns `false`)
    /// for an unknown id, so a stale focus request can never desync `focus`.
    pub fn set_focus(&mut self, session: u64) -> bool {
        if self.contains(session) {
            self.focus = session;
            true
        } else {
            false
        }
    }

    /// Whether `session` is a leaf anywhere in this tab.
    #[must_use]
    pub fn contains(&self, session: u64) -> bool {
        Self::contains_in(&self.root, session)
    }

    fn contains_in(node: &PaneNode, session: u64) -> bool {
        match node {
            PaneNode::Leaf { session: s } => *s == session,
            PaneNode::Split { first, second, .. } => {
                Self::contains_in(first, session) || Self::contains_in(second, session)
            }
        }
    }

    /// Every leaf session id in this tab, in left-to-right / top-to-bottom tree
    /// order. Used to resize/tear-down a whole tab's panes and to test round-trips.
    #[must_use]
    pub fn sessions(&self) -> Vec<u64> {
        let mut out = Vec::new();
        Self::collect(&self.root, &mut out);
        out
    }

    fn collect(node: &PaneNode, out: &mut Vec<u64>) {
        match node {
            PaneNode::Leaf { session } => out.push(*session),
            PaneNode::Split { first, second, .. } => {
                Self::collect(first, out);
                Self::collect(second, out);
            }
        }
    }

    /// The number of live panes (leaves) in this tab. `1` for a fresh tab.
    #[must_use]
    pub fn len(&self) -> usize {
        let mut n = 0;
        Self::count(&self.root, &mut n);
        n
    }

    fn count(node: &PaneNode, n: &mut usize) {
        match node {
            PaneNode::Leaf { .. } => *n += 1,
            PaneNode::Split { first, second, .. } => {
                Self::count(first, n);
                Self::count(second, n);
            }
        }
    }

    /// Split the FOCUSED leaf in `dir`, inserting `new_session` as the SECOND child
    /// (right/bottom) and keeping the original session as the first. Focus moves to
    /// the new pane (the standard "split and type in the new one" behavior). The
    /// split is 50/50 ([`DEFAULT_RATIO`]). No-op (returns `false`) if the focused
    /// leaf somehow isn't found (it always is), leaving the tree untouched so the
    /// caller can drop the just-spawned session.
    ///
    /// TRUST anchor: this is the `Split` action of the ty-proven `pane_tree` machine
    /// (`pane_tree_model()`); the Tier-1 binding is `pane_tree_conformance`.
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "pane_tree",
            action = "Split",
            project = "aterm_gui::pane_tree_conformance::project"
        )
    )]
    pub fn split_focused(&mut self, dir: SplitDir, new_session: u64) -> bool {
        let focus = self.focus;
        if Self::split_leaf(&mut self.root, focus, dir, new_session) {
            self.focus = new_session;
            true
        } else {
            false
        }
    }

    fn split_leaf(node: &mut PaneNode, target: u64, dir: SplitDir, new_session: u64) -> bool {
        match node {
            PaneNode::Leaf { session } if *session == target => {
                let original = *session;
                *node = PaneNode::Split {
                    dir,
                    ratio: DEFAULT_RATIO,
                    first: Box::new(PaneNode::Leaf { session: original }),
                    second: Box::new(PaneNode::Leaf {
                        session: new_session,
                    }),
                };
                true
            }
            PaneNode::Leaf { .. } => false,
            PaneNode::Split { first, second, .. } => {
                Self::split_leaf(first, target, dir, new_session)
                    || Self::split_leaf(second, target, dir, new_session)
            }
        }
    }

    /// Close the FOCUSED pane (Cmd-W). See [`Self::close_pane`].
    pub fn close_focused(&mut self) -> CloseOutcome {
        self.close_pane(self.focus)
    }

    /// Close the pane holding `session` (the FOCUSED pane via [`Self::close_focused`],
    /// or any pane whose reader hit EOF). Returns the outcome:
    /// * [`CloseOutcome::Collapsed`] — the leaf was removed and its parent replaced
    ///   by the SIBLING sub-tree; focus re-seats on the nearest surviving leaf. The
    ///   tab (and window) keeps living.
    /// * [`CloseOutcome::LastPane`] — that leaf was the tab's ONLY pane, so the whole
    ///   tab should close (the caller removes the tab; the engine's last tab closing
    ///   exits the app, unchanged).
    ///
    /// Either way the returned `closed` is the session id that was removed, so the
    /// caller tears down exactly that session (closes its PTY master → its reader
    /// thread ends) and deregisters it. Every OTHER pane's session — and its reader
    /// thread — is untouched. An unknown id is treated as the focused pane (the
    /// caller only calls this for live panes; `close_session` filters unknown ids).
    ///
    /// TRUST anchor: the `CloseOutcome::Collapsed` arm is the `Close` action of the
    /// ty-proven `pane_tree` machine (`pane_tree_model()`) — the tree shrinks by one
    /// leaf and focus re-seats on a survivor IN RANGE. (`LastPane` is a tab-machine
    /// transition, out of this model's scope.) Tier-1 binding: `pane_tree_conformance`.
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "pane_tree",
            action = "Close",
            project = "aterm_gui::pane_tree_conformance::project"
        )
    )]
    pub fn close_pane(&mut self, session: u64) -> CloseOutcome {
        let closed = if self.contains(session) {
            session
        } else {
            self.focus
        };
        // The only pane in the tab: nothing to collapse into; the tab closes.
        if matches!(self.root, PaneNode::Leaf { .. }) {
            return CloseOutcome::LastPane { closed };
        }
        Self::remove_leaf(&mut self.root, closed);
        // Re-seat focus on the nearest surviving leaf if the focused pane was the
        // one removed; otherwise focus stays where it was (a background pane's EOF
        // must not steal focus from the pane the user is typing in).
        if self.focus == closed || !self.contains(self.focus) {
            self.focus = Self::first_leaf(&self.root);
        }
        CloseOutcome::Collapsed { closed }
    }

    /// Replace the `Split` parent of `target`'s leaf with `target`'s SIBLING (the
    /// other child), removing the leaf and the now-childless split node. Assumes
    /// `target` is a leaf somewhere under a `Split` (guaranteed by `close_focused`,
    /// which handles the lone-root leaf separately). Returns `true` once spliced.
    fn remove_leaf(node: &mut PaneNode, target: u64) -> bool {
        // Is one of THIS split's direct children the target leaf? If so, replace
        // this whole split node with the OTHER child (collapse the parent).
        if let PaneNode::Split { first, second, .. } = node {
            let first_is_target =
                matches!(&**first, PaneNode::Leaf { session } if *session == target);
            let second_is_target =
                matches!(&**second, PaneNode::Leaf { session } if *session == target);
            if first_is_target {
                let survivor = std::mem::replace(second.as_mut(), PaneNode::Leaf { session: 0 });
                *node = survivor;
                return true;
            }
            if second_is_target {
                let survivor = std::mem::replace(first.as_mut(), PaneNode::Leaf { session: 0 });
                *node = survivor;
                return true;
            }
            // Otherwise recurse into whichever subtree holds the target.
            if let PaneNode::Split { first, second, .. } = node {
                return Self::remove_leaf(first, target) || Self::remove_leaf(second, target);
            }
        }
        false
    }

    /// The left/top-most leaf session of a subtree (the deterministic focus seat
    /// after a collapse).
    fn first_leaf(node: &PaneNode) -> u64 {
        match node {
            PaneNode::Leaf { session } => *session,
            PaneNode::Split { first, .. } => Self::first_leaf(first),
        }
    }

    /// Compute every visible pane's placement for a window of `rows`×`cols` cells.
    /// Returns one [`PaneRect`] per leaf, with 1-cell dividers reserved between
    /// split children (the gaps are NOT covered by any rect; the GUI paints them).
    /// A single-leaf tab yields exactly one rect covering the whole window — the
    /// non-split geometry, byte-identical to today.
    #[must_use]
    pub fn compute_layout(&self, rows: u16, cols: u16) -> Vec<PaneRect> {
        let mut out = Vec::with_capacity(self.len());
        layout_into(&self.root, 0, 0, rows.max(1), cols.max(1), &mut out);
        out
    }

    /// Hit-test: the session id of the pane whose rect contains cell `(row, col)`,
    /// or `None` when the point lands on a divider / outside the grid. Used by
    /// click-to-focus.
    #[must_use]
    pub fn pane_at(&self, row: u16, col: u16, rows: u16, cols: u16) -> Option<u64> {
        self.compute_layout(rows, cols).into_iter().find_map(|r| {
            let in_rows = row >= r.row_off && row < r.row_off + r.rows;
            let in_cols = col >= r.col_off && col < r.col_off + r.cols;
            (in_rows && in_cols).then_some(r.session)
        })
    }
}

/// The result of [`PaneTree::close_focused`]: which session was removed and
/// whether the tab survives (a sibling remained) or must close (it was the last
/// pane).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseOutcome {
    /// A sibling remained: the parent split collapsed into it. `closed` is the
    /// removed session.
    Collapsed { closed: u64 },
    /// The focused pane was the tab's only one; the whole tab should close.
    /// `closed` is that session.
    LastPane { closed: u64 },
}

impl CloseOutcome {
    /// The session id that was removed (to tear down + deregister), in both cases.
    #[must_use]
    pub fn closed(self) -> u64 {
        match self {
            CloseOutcome::Collapsed { closed } | CloseOutcome::LastPane { closed } => closed,
        }
    }
}

/// Recursively place `node` into the rectangle at `(r0, c0)` of size `rows`×`cols`,
/// pushing a [`PaneRect`] for each leaf. A split reserves one cell line for its
/// divider and clamps each child to at least 1 cell so a tiny window never yields
/// a 0-extent pane (it just stops being splittable visually).
fn layout_into(node: &PaneNode, r0: u16, c0: u16, rows: u16, cols: u16, out: &mut Vec<PaneRect>) {
    match node {
        PaneNode::Leaf { session } => {
            out.push(PaneRect {
                session: *session,
                row_off: r0,
                col_off: c0,
                rows,
                cols,
            });
        }
        PaneNode::Split {
            dir,
            ratio,
            first,
            second,
        } => match dir {
            SplitDir::Vertical => {
                // Split the COLUMNS: [first | divider(1) | second].
                let (a, b) = split_extent(cols, *ratio);
                layout_into(first, r0, c0, rows, a, out);
                // second starts after the first pane + the 1-cell divider.
                layout_into(second, r0, c0 + a + 1, rows, b, out);
            }
            SplitDir::Horizontal => {
                // Split the ROWS: [first / divider(1) / second].
                let (a, b) = split_extent(rows, *ratio);
                layout_into(first, r0, c0, a, cols, out);
                layout_into(second, r0 + a + 1, c0, b, cols, out);
            }
        },
    }
}

/// Divide `extent` cells into `(first, second)` around a 1-cell divider, with
/// `first` taking `ratio` of the splittable extent (everything but the divider).
/// Both sides are clamped to `>= 1` so neither pane ever vanishes; when `extent`
/// is too small to hold two panes + a divider (`< 3`), the divider is dropped and
/// each side gets at least 1 cell (overflowing the rect rather than producing a
/// 0-extent pane — a degenerate tiny-window case the GUI clamps on present).
fn split_extent(extent: u16, ratio: f32) -> (u16, u16) {
    // Reserve one cell for the divider; the rest is splittable.
    let splittable = extent.saturating_sub(1);
    if splittable < 2 {
        // Too small for two panes + a divider: give each side 1 cell.
        return (1, 1);
    }
    let ratio = ratio.clamp(0.0, 1.0);
    // First child's share, clamped so BOTH sides keep at least 1 cell.
    let first = ((f32::from(splittable) * ratio).round() as u16).clamp(1, splittable - 1);
    let second = splittable - first;
    (first, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh single-pane tab lays out as ONE rect covering the whole window —
    /// the non-split geometry must be byte-identical to "no panes" (the whole
    /// no-regression contract rests on this).
    #[test]
    fn single_pane_fills_window() {
        let t = PaneTree::new(7);
        assert_eq!(t.len(), 1);
        assert_eq!(t.focus(), 7);
        let rects = t.compute_layout(24, 80);
        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0],
            PaneRect {
                session: 7,
                row_off: 0,
                col_off: 0,
                rows: 24,
                cols: 80
            }
        );
    }

    /// Cmd-D vertical split: two panes side by side, a 1-cell divider column
    /// between them, both full height. 80 cols → 79 splittable → 40 | divider | 39.
    #[test]
    fn vertical_split_geometry() {
        let mut t = PaneTree::new(1);
        assert!(t.split_focused(SplitDir::Vertical, 2));
        assert_eq!(t.focus(), 2, "focus follows the new pane");
        let mut rects = t.compute_layout(24, 80);
        rects.sort_by_key(|r| r.col_off);
        assert_eq!(rects.len(), 2);
        // Left pane: session 1, cols 0..40.
        assert_eq!(
            rects[0],
            PaneRect {
                session: 1,
                row_off: 0,
                col_off: 0,
                rows: 24,
                cols: 40
            }
        );
        // Right pane: session 2, starts after 40 + 1-cell divider = col 41, 39 wide.
        assert_eq!(
            rects[1],
            PaneRect {
                session: 2,
                row_off: 0,
                col_off: 41,
                rows: 24,
                cols: 39
            }
        );
        // The divider column (40) is covered by NO rect.
        assert!(
            rects
                .iter()
                .all(|r| !(r.col_off..r.col_off + r.cols).contains(&40))
        );
    }

    /// Cmd-Shift-D horizontal split: two panes stacked, a 1-cell divider row
    /// between them, both full width. 24 rows → 23 splittable → 12 | divider | 11.
    #[test]
    fn horizontal_split_geometry() {
        let mut t = PaneTree::new(1);
        assert!(t.split_focused(SplitDir::Horizontal, 2));
        let mut rects = t.compute_layout(24, 80);
        rects.sort_by_key(|r| r.row_off);
        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            PaneRect {
                session: 1,
                row_off: 0,
                col_off: 0,
                rows: 12,
                cols: 80
            }
        );
        assert_eq!(
            rects[1],
            PaneRect {
                session: 2,
                row_off: 13,
                col_off: 0,
                rows: 11,
                cols: 80
            }
        );
        assert!(
            rects
                .iter()
                .all(|r| !(r.row_off..r.row_off + r.rows).contains(&12))
        );
    }

    /// A 2x2 golden: vertical split, then horizontally split the (focused) right
    /// pane. Three leaves (1 | (2 / 3)), every rect disjoint, no divider overlap.
    #[test]
    fn nested_2x2_layout() {
        let mut t = PaneTree::new(1);
        assert!(t.split_focused(SplitDir::Vertical, 2)); // focus → 2 (right)
        assert!(t.split_focused(SplitDir::Horizontal, 3)); // split right → top 2 / bottom 3
        assert_eq!(t.len(), 3, "1 | (2 / 3) — three panes");
        assert_eq!(t.sessions(), vec![1, 2, 3]);
        let rects = t.compute_layout(24, 80);
        assert_eq!(rects.len(), 3);
        // Left pane spans full height.
        let left = rects.iter().find(|r| r.session == 1).unwrap();
        assert_eq!(left.rows, 24);
        // The two right panes share the right column band and stack.
        let top = rects.iter().find(|r| r.session == 2).unwrap();
        let bot = rects.iter().find(|r| r.session == 3).unwrap();
        assert_eq!(top.col_off, bot.col_off);
        assert_eq!(top.cols, bot.cols);
        assert!(top.row_off < bot.row_off);
        // No two rects overlap (cell-by-cell disjointness over the window).
        assert!(rects_disjoint(&rects));
    }

    /// Split → close round-trip: closing the focused (new) pane collapses back to
    /// the original single pane, with focus re-seated on the survivor. The
    /// surviving session is untouched (its reader thread stays alive — the caller
    /// only tears down `closed`).
    #[test]
    fn split_then_close_round_trips() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2);
        assert_eq!(t.focus(), 2);
        let outcome = t.close_focused();
        assert_eq!(outcome, CloseOutcome::Collapsed { closed: 2 });
        assert_eq!(t.len(), 1, "collapsed back to one pane");
        assert_eq!(t.sessions(), vec![1], "the sibling survives");
        assert_eq!(t.focus(), 1, "focus re-seats on the survivor");
        // And the survivor lays out full-window again — byte-identical to fresh.
        assert_eq!(
            t.compute_layout(24, 80),
            PaneTree::new(1).compute_layout(24, 80)
        );
    }

    /// Closing the focused pane in a deeper tree collapses only its parent; the
    /// other branch is structurally untouched.
    #[test]
    fn close_collapses_only_parent() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2); // 1 | 2, focus 2
        t.split_focused(SplitDir::Horizontal, 3); // 1 | (2 / 3), focus 3
        // Close 3: its parent (the 2/3 split) collapses into 2; left branch (1) stays.
        let outcome = t.close_focused();
        assert_eq!(outcome, CloseOutcome::Collapsed { closed: 3 });
        assert_eq!(t.sessions(), vec![1, 2]);
        assert_eq!(t.focus(), 1, "focus re-seats on the left/top-most survivor");
        // The geometry is now exactly a 2-pane vertical split of 1 | 2.
        let mut expected = PaneTree::new(1);
        expected.split_focused(SplitDir::Vertical, 2);
        assert_eq!(t.compute_layout(24, 80), expected.compute_layout(24, 80));
    }

    /// Closing a BACKGROUND pane (reader EOF on a non-focused pane) collapses it
    /// but does NOT steal focus from the pane the user is typing in.
    #[test]
    fn close_background_pane_keeps_focus() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2); // 1 | 2, focus 2
        // Re-focus the left pane (session 1), then session 2's reader hits EOF.
        assert!(t.set_focus(1));
        let outcome = t.close_pane(2);
        assert_eq!(outcome, CloseOutcome::Collapsed { closed: 2 });
        assert_eq!(t.sessions(), vec![1]);
        assert_eq!(t.focus(), 1, "focus stays on the pane the user is using");
    }

    /// Closing the LAST pane signals the tab should close (LastPane), not a
    /// collapse — the caller removes the tab (and the last tab closing exits).
    #[test]
    fn close_last_pane_signals_tab_close() {
        let mut t = PaneTree::new(9);
        let outcome = t.close_focused();
        assert_eq!(outcome, CloseOutcome::LastPane { closed: 9 });
        assert_eq!(outcome.closed(), 9);
    }

    /// Focus → session mapping: click-to-focus picks the pane under the cell, and
    /// a divider cell maps to no pane (focus unchanged).
    #[test]
    fn pane_at_hit_test() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2); // 40 | divider(40) | 39
        // A cell in the left band → session 1.
        assert_eq!(t.pane_at(5, 10, 24, 80), Some(1));
        // A cell in the right band → session 2.
        assert_eq!(t.pane_at(5, 60, 24, 80), Some(2));
        // The divider column → no pane.
        assert_eq!(t.pane_at(5, 40, 24, 80), None);
        // Out of grid → no pane.
        assert_eq!(t.pane_at(99, 99, 24, 80), None);
        // set_focus follows the hit-test result.
        assert!(t.set_focus(1));
        assert_eq!(t.focus(), 1);
        assert!(!t.set_focus(999), "unknown id is rejected, focus unchanged");
        assert_eq!(t.focus(), 1);
    }

    /// `set_focus`/`contains` reject ids that aren't leaves in this tab.
    #[test]
    fn focus_only_live_leaves() {
        let t = PaneTree::new(3);
        assert!(t.contains(3));
        assert!(!t.contains(4));
    }

    /// A 2x1 golden across an odd width: 81 cols → 80 splittable → 40 | 40, divider
    /// at col 40. (Round-trips the even/odd split math.)
    #[test]
    fn vertical_split_odd_width() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2);
        let mut rects = t.compute_layout(24, 81);
        rects.sort_by_key(|r| r.col_off);
        assert_eq!(rects[0].cols, 40);
        assert_eq!(rects[1].col_off, 41);
        assert_eq!(rects[1].cols, 40);
    }

    /// A degenerate tiny window still yields one non-zero rect per pane (never a
    /// 0-extent pane the renderer would choke on).
    #[test]
    fn tiny_window_no_zero_panes() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2);
        for rect in t.compute_layout(1, 2) {
            assert!(
                rect.rows >= 1 && rect.cols >= 1,
                "no 0-extent pane: {rect:?}"
            );
        }
    }

    /// Helper: are all rects pairwise cell-disjoint? (No two panes claim the same
    /// cell — dividers are gaps owned by neither.)
    fn rects_disjoint(rects: &[PaneRect]) -> bool {
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let rows_overlap = a.row_off < b.row_off + b.rows && b.row_off < a.row_off + a.rows;
                let cols_overlap = a.col_off < b.col_off + b.cols && b.col_off < a.col_off + a.cols;
                if rows_overlap && cols_overlap {
                    return false;
                }
            }
        }
        true
    }
}
