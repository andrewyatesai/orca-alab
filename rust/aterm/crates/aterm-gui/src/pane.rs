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
    /// When `true`, the focused pane is temporarily ZOOMED to fill the whole
    /// window ([`compute_layout`](Self::compute_layout) returns just it). A purely
    /// presentational toggle over the unchanged tree — unzoom restores the layout
    /// exactly. Ignored for a single-pane tab. iTerm2-style pane zoom.
    zoomed: bool,
}

/// The first child's default fraction of the splittable extent: a 50/50 split. A
/// later divider drag adjusts a [`PaneNode::Split::ratio`]; the MVP fixes it here.
const DEFAULT_RATIO: f32 = 0.5;

/// The smallest fraction a divider drag may leave the FIRST child (and, by
/// symmetry via `1 - MIN_RATIO` = [`MAX_RATIO`], the second). Keeps both panes
/// visibly non-trivial so a drag to the very edge never collapses a pane to a
/// sliver. (`split_extent` still clamps each side to ≥ 1 cell; this is the
/// higher-level ergonomic floor a drag is held to.)
const MIN_RATIO: f32 = 0.05;
/// The largest fraction a divider drag may give the first child — the mirror of
/// [`MIN_RATIO`], so the SECOND child also keeps at least `MIN_RATIO`.
const MAX_RATIO: f32 = 0.95;

/// One pane divider's identity + geometry, produced by [`PaneTree::divider_at`] and
/// consumed by [`PaneTree::ratio_for_pointer`] / [`PaneTree::set_divider_ratio`] to
/// drive a drag-to-resize. The `path` names the exact [`PaneNode::Split`] whose
/// divider was hit (a root-to-node walk: `false` = descend into `first`, `true` =
/// into `second`), so the mutator edits THAT split even in a deep tree. `dir`,
/// `span_start`, and `span_len` carry the split's geometry along the divided axis
/// (columns for a `Vertical` split, rows for `Horizontal`) so a pointer position
/// maps back to a ratio without re-walking the tree.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DividerHit {
    /// Root-to-split branch walk (`false` = `first`, `true` = `second`).
    path: Vec<bool>,
    /// Which way the hit split divides (vertical divider = columns, horizontal =
    /// rows). Lets the GUI pick the resize cursor (E-W vs N-S).
    pub dir: SplitDir,
    /// First splittable cell along the divided axis (the split rect's `col_off` for
    /// `Vertical`, `row_off` for `Horizontal`).
    span_start: u16,
    /// The split's extent along the divided axis (its `cols` for `Vertical`, `rows`
    /// for `Horizontal`) — the same `extent` `split_extent` divides.
    span_len: u16,
}

impl PaneTree {
    /// A new single-pane tab holding `session` (the day-one one-session-per-tab
    /// layout). Focus is that one session.
    #[must_use]
    pub fn new(session: u64) -> Self {
        PaneTree {
            root: PaneNode::Leaf { session },
            focus: session,
            zoomed: false,
        }
    }

    /// Toggle pane ZOOM: when on, [`compute_layout`](Self::compute_layout) returns
    /// only the focused pane filling the window. A no-op (stays off) for a
    /// single-pane tab. Returns the new zoom state.
    pub fn toggle_zoom(&mut self) -> bool {
        self.zoomed = !self.zoomed && self.len() > 1;
        self.zoomed
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
            // A structural change exits zoom (the layout the user zoomed is gone).
            self.zoomed = false;
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
        // A structural change exits zoom (the zoomed layout no longer applies).
        self.zoomed = false;
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
        // Zoomed: the focused pane alone fills the window (other panes are hidden
        // until unzoom). Single-pane tabs ignore the flag and take the normal path.
        if self.zoomed && self.len() > 1 {
            return vec![PaneRect {
                session: self.focus,
                row_off: 0,
                col_off: 0,
                rows: rows.max(1),
                cols: cols.max(1),
            }];
        }
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

    /// The session of the pane directly adjacent to the focused one in `dir`, or
    /// `None` when there is no pane on that side. Used by keyboard pane navigation
    /// (directional focus). A candidate must lie on the `dir` side of the focused
    /// rect AND share some perpendicular extent with it (so "left" of a tall pane
    /// only considers panes that overlap its rows); ties break toward the larger
    /// overlap, then the smaller offset (top-/left-most), for a stable choice.
    #[must_use]
    pub fn focus_neighbor(&self, dir: FocusDir, rows: u16, cols: u16) -> Option<u64> {
        let layout = self.compute_layout(rows, cols);
        let focus = self.focus();
        let cur = layout.iter().find(|r| r.session == focus)?;
        // Inclusive-exclusive edges of the focused rect.
        let (cur_top, cur_bottom) = (cur.row_off, cur.row_off + cur.rows);
        let (cur_left, cur_right) = (cur.col_off, cur.col_off + cur.cols);

        // Overlap of two [a,b) intervals (0 = none).
        let overlap =
            |a0: u16, a1: u16, b0: u16, b1: u16| -> u16 { a1.min(b1).saturating_sub(a0.max(b0)) };

        let mut best: Option<(&PaneRect, u16, u16)> = None; // (rect, distance, overlap)
        for r in &layout {
            if r.session == focus {
                continue;
            }
            let (r_top, r_bottom) = (r.row_off, r.row_off + r.rows);
            let (r_left, r_right) = (r.col_off, r.col_off + r.cols);
            // `on_side` = candidate is on the `dir` side; `dist` = gap along the
            // axis of travel; `ov` = shared perpendicular extent (must be > 0).
            let (on_side, dist, ov) = match dir {
                FocusDir::Left => (
                    r_right <= cur_left,
                    cur_left.saturating_sub(r_right),
                    overlap(cur_top, cur_bottom, r_top, r_bottom),
                ),
                FocusDir::Right => (
                    r_left >= cur_right,
                    r_left.saturating_sub(cur_right),
                    overlap(cur_top, cur_bottom, r_top, r_bottom),
                ),
                FocusDir::Up => (
                    r_bottom <= cur_top,
                    cur_top.saturating_sub(r_bottom),
                    overlap(cur_left, cur_right, r_left, r_right),
                ),
                FocusDir::Down => (
                    r_top >= cur_bottom,
                    r_top.saturating_sub(cur_bottom),
                    overlap(cur_left, cur_right, r_left, r_right),
                ),
            };
            if !on_side || ov == 0 {
                continue;
            }
            // Prefer the nearest pane (smallest gap); break ties by larger overlap.
            let better = match best {
                None => true,
                Some((_, bdist, bov)) => dist < bdist || (dist == bdist && ov > bov),
            };
            if better {
                best = Some((r, dist, ov));
            }
        }
        best.map(|(r, _, _)| r.session)
    }

    /// Hit-test a DIVIDER: if cell `(row, col)` lands on a split's 1-cell divider
    /// line (the gap [`compute_layout`] reserves between a split's children, owned
    /// by no pane), return that divider's [`DividerHit`]; otherwise `None` (the cell
    /// is inside a pane or outside the grid). Used by drag-to-resize to start a
    /// divider drag. Zoomed or single-pane tabs have no draggable divider (the
    /// focused pane fills the window), so this is always `None` for them.
    #[must_use]
    pub fn divider_at(&self, row: u16, col: u16, rows: u16, cols: u16) -> Option<DividerHit> {
        if self.len() == 1 || (self.zoomed && self.len() > 1) {
            return None;
        }
        let mut path = Vec::new();
        divider_at_in(
            &self.root,
            0,
            0,
            rows.max(1),
            cols.max(1),
            row,
            col,
            &mut path,
        )
    }

    /// Map a pointer at cell `(row, col)` to the new FIRST-child fraction for the
    /// split named by `hit`, BEFORE clamping (the raw geometric ratio). The pointer
    /// is projected onto the hit split's divided axis and divided by the split's
    /// splittable extent (the same extent [`split_extent`] divides), so dropping the
    /// divider where the pointer is yields that ratio. Returns `None` only for a
    /// degenerate split too small to hold a divider. The caller passes the result to
    /// [`Self::set_divider_ratio`], which applies the `[MIN_RATIO, MAX_RATIO]` clamp.
    #[must_use]
    pub fn ratio_for_pointer(&self, hit: &DividerHit, row: u16, col: u16) -> Option<f32> {
        // The pointer position along the divided axis (col for Vertical, row for
        // Horizontal). `split_extent` reserves 1 cell for the divider, so the
        // splittable extent is `span_len - 1`.
        let pointer = match hit.dir {
            SplitDir::Vertical => col,
            SplitDir::Horizontal => row,
        };
        let splittable = hit.span_len.saturating_sub(1);
        if splittable < 2 {
            return None;
        }
        // Offset of the pointer from the split's start, clamped into the split.
        let offset = pointer.saturating_sub(hit.span_start).min(splittable);
        Some(f32::from(offset) / f32::from(splittable))
    }

    /// Set the FIRST-child fraction of the split named by `hit` to `ratio`, clamped
    /// to `[MIN_RATIO, MAX_RATIO]` so neither pane collapses to a sliver. Returns
    /// `true` once the targeted split's `ratio` was written (it always is for a
    /// `hit` produced by [`Self::divider_at`] on the same tree); `false` if the path
    /// no longer names a split (e.g. the tree changed under a stale hit), leaving the
    /// tree untouched. A pure DATA edit — no structural change, so focus/zoom are
    /// preserved; the caller relays out + repaints.
    pub fn set_divider_ratio(&mut self, hit: &DividerHit, ratio: f32) -> bool {
        let clamped = ratio.clamp(MIN_RATIO, MAX_RATIO);
        let mut node = &mut self.root;
        for &go_second in &hit.path {
            let PaneNode::Split { first, second, .. } = node else {
                return false; // path ran off a leaf: stale hit
            };
            node = if go_second { second } else { first };
        }
        if let PaneNode::Split { ratio: r, .. } = node {
            *r = clamped;
            true
        } else {
            false
        }
    }
}

/// Walk `node` placed at `(r0, c0)` of size `rows`×`cols` (mirroring [`layout_into`])
/// looking for the SPLIT whose 1-cell divider contains `(row, col)`. `path` is the
/// running root-to-`node` branch trail (`false`/`true` = `first`/`second`); on a hit
/// it names the matched split. Returns that split's [`DividerHit`], or `None` when
/// the cell is inside a pane / off the divider. Recurses into the child whose
/// sub-rect contains the cell so a nested split's divider is found too.
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors layout_into's rect walk plus the probe cell + path accumulator; bundling only relocates the list"
)]
fn divider_at_in(
    node: &PaneNode,
    r0: u16,
    c0: u16,
    rows: u16,
    cols: u16,
    row: u16,
    col: u16,
    path: &mut Vec<bool>,
) -> Option<DividerHit> {
    let PaneNode::Split {
        dir,
        ratio,
        first,
        second,
    } = node
    else {
        return None; // a leaf has no divider
    };
    match dir {
        SplitDir::Vertical => {
            // The first child's column extent + the divider column, exactly as
            // `layout_into` computes them (so the test cell matches what was drawn).
            let (a, b) = split_extent(cols, *ratio);
            let div_col = c0 + a;
            if row >= r0 && row < r0 + rows && col == div_col {
                return Some(DividerHit {
                    path: path.clone(),
                    dir: SplitDir::Vertical,
                    span_start: c0,
                    span_len: cols,
                });
            }
            // Recurse into whichever child's column band holds the cell.
            if col < div_col {
                path.push(false);
                let hit = divider_at_in(first, r0, c0, rows, a, row, col, path);
                path.pop();
                hit
            } else {
                path.push(true);
                let hit = divider_at_in(second, r0, c0 + a + 1, rows, b, row, col, path);
                path.pop();
                hit
            }
        }
        SplitDir::Horizontal => {
            let (a, b) = split_extent(rows, *ratio);
            let div_row = r0 + a;
            if col >= c0 && col < c0 + cols && row == div_row {
                return Some(DividerHit {
                    path: path.clone(),
                    dir: SplitDir::Horizontal,
                    span_start: r0,
                    span_len: rows,
                });
            }
            if row < div_row {
                path.push(false);
                let hit = divider_at_in(first, r0, c0, a, cols, row, col, path);
                path.pop();
                hit
            } else {
                path.push(true);
                let hit = divider_at_in(second, r0 + a + 1, c0, b, cols, row, col, path);
                path.pop();
                hit
            }
        }
    }
}

/// A direction for keyboard pane-focus navigation ([`PaneTree::focus_neighbor`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusDir {
    /// Move focus to the pane on the left.
    Left,
    /// Move focus to the pane on the right.
    Right,
    /// Move focus to the pane above.
    Up,
    /// Move focus to the pane below.
    Down,
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

    /// Directional focus over the 2x2 golden `1 | (2 / 3)`: from each pane, the
    /// neighbor in each direction is the adjacent pane (or None at an edge).
    #[test]
    fn focus_neighbor_directions() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2); // 1 | 2, focus 2
        t.split_focused(SplitDir::Horizontal, 3); // 1 | (2 / 3), focus 3 (bottom-right)
        assert_eq!(t.focus(), 3);
        // From bottom-right (3): up→2, left→1, right/down→edge.
        assert_eq!(t.focus_neighbor(FocusDir::Up, 24, 80), Some(2));
        assert_eq!(t.focus_neighbor(FocusDir::Left, 24, 80), Some(1));
        assert_eq!(t.focus_neighbor(FocusDir::Right, 24, 80), None);
        assert_eq!(t.focus_neighbor(FocusDir::Down, 24, 80), None);
        // From top-right (2): left→1, down→3, up/right→edge.
        assert!(t.set_focus(2));
        assert_eq!(t.focus_neighbor(FocusDir::Left, 24, 80), Some(1));
        assert_eq!(t.focus_neighbor(FocusDir::Down, 24, 80), Some(3));
        assert_eq!(t.focus_neighbor(FocusDir::Up, 24, 80), None);
        assert_eq!(t.focus_neighbor(FocusDir::Right, 24, 80), None);
        // From the full-height left pane (1): right→a right-band pane; left→edge.
        assert!(t.set_focus(1));
        assert!(matches!(
            t.focus_neighbor(FocusDir::Right, 24, 80),
            Some(2 | 3)
        ));
        assert_eq!(t.focus_neighbor(FocusDir::Left, 24, 80), None);
    }

    /// A single-pane tab has no neighbor in any direction.
    #[test]
    fn focus_neighbor_single_pane_none() {
        let t = PaneTree::new(1);
        for dir in [
            FocusDir::Left,
            FocusDir::Right,
            FocusDir::Up,
            FocusDir::Down,
        ] {
            assert_eq!(t.focus_neighbor(dir, 24, 80), None);
        }
    }

    /// Zoom shows only the focused pane full-window; unzoom restores; a single-pane
    /// tab can't zoom; and a structural change (split) exits zoom.
    #[test]
    fn zoom_focused_pane_fills_and_restores() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2); // 1 | 2, focus 2
        assert_eq!(t.compute_layout(24, 80).len(), 2);

        assert!(t.toggle_zoom(), "zoom on (multi-pane)");
        let z = t.compute_layout(24, 80);
        assert_eq!(z.len(), 1, "zoom shows only the focused pane");
        assert_eq!(z[0].session, 2);
        assert_eq!(
            (z[0].row_off, z[0].col_off, z[0].rows, z[0].cols),
            (0, 0, 24, 80)
        );

        assert!(!t.toggle_zoom(), "toggle off");
        assert_eq!(t.compute_layout(24, 80).len(), 2, "unzoom restores layout");

        // Single-pane tabs ignore zoom (stays off).
        let mut s = PaneTree::new(9);
        assert!(!s.toggle_zoom());
        assert_eq!(s.compute_layout(24, 80).len(), 1);

        // A split exits zoom: all panes show again.
        let mut x = PaneTree::new(1);
        x.split_focused(SplitDir::Vertical, 2);
        assert!(x.toggle_zoom());
        x.split_focused(SplitDir::Horizontal, 3);
        assert_eq!(
            x.compute_layout(24, 80).len(),
            3,
            "split exits zoom -> all panes shown"
        );
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

    /// Divider drag on a vertical split: hitting the divider column yields a
    /// `DividerHit`, and `set_divider_ratio` moves the boundary — the left pane
    /// grows/shrinks while the geometry stays a valid 2-pane split.
    #[test]
    fn vertical_divider_drag_moves_boundary() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2); // 80 cols -> 40 | divider(40) | 39
        // The divider sits at column 40, any row.
        let hit = t.divider_at(5, 40, 24, 80).expect("divider hit at col 40");
        assert_eq!(hit.dir, SplitDir::Vertical);
        // A cell inside a pane is NOT on the divider.
        assert!(t.divider_at(5, 10, 24, 80).is_none());
        assert!(t.divider_at(5, 60, 24, 80).is_none());
        // Drag the divider left to column 20: ratio ~ 20/79.
        let ratio = t.ratio_for_pointer(&hit, 5, 20).expect("ratio for pointer");
        assert!((ratio - 20.0 / 79.0).abs() < 1e-3, "ratio {ratio}");
        assert!(t.set_divider_ratio(&hit, ratio));
        let mut rects = t.compute_layout(24, 80);
        rects.sort_by_key(|r| r.col_off);
        // Left pane shrank to ~20 cols; divider is now just past it.
        assert_eq!(rects[0].session, 1);
        assert_eq!(rects[0].cols, 20);
        assert_eq!(rects[1].col_off, 21);
        assert_eq!(rects[1].cols, 59); // 79 splittable - 20 first
    }

    /// `set_divider_ratio` CLAMPS to `[MIN_RATIO, MAX_RATIO]`: dragging the divider
    /// to (or past) an edge never collapses a pane to zero.
    #[test]
    fn divider_ratio_clamps() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2);
        let hit = t.divider_at(5, 40, 24, 80).expect("divider hit");
        // Drag hard left (ratio 0.0) clamps to MIN_RATIO; both panes survive.
        assert!(t.set_divider_ratio(&hit, 0.0));
        for r in t.compute_layout(24, 80) {
            assert!(r.cols >= 1, "no zero-width pane after min clamp: {r:?}");
        }
        let left = t
            .compute_layout(24, 80)
            .into_iter()
            .min_by_key(|r| r.col_off)
            .unwrap();
        // MIN_RATIO of 79 splittable ≈ 4 cells (round(0.05*79)=4), well above 0.
        assert!(left.cols >= (MIN_RATIO * 79.0).floor() as u16);
        // Drag hard right (ratio 1.0) clamps to MAX_RATIO; right pane survives.
        assert!(t.set_divider_ratio(&hit, 1.0));
        for r in t.compute_layout(24, 80) {
            assert!(r.cols >= 1, "no zero-width pane after max clamp: {r:?}");
        }
    }

    /// Headless hit-test → ratio mapping over a horizontal split: the divider row is
    /// found and a pointer maps to the proportional ratio along the rows.
    #[test]
    fn horizontal_divider_hit_and_ratio() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Horizontal, 2); // 24 rows -> 12 | divider(12) | 11
        let hit = t.divider_at(12, 30, 24, 80).expect("divider hit at row 12");
        assert_eq!(hit.dir, SplitDir::Horizontal);
        // Off the divider row → None.
        assert!(t.divider_at(3, 30, 24, 80).is_none());
        // Drag to row 6: ratio ~ 6/23.
        let ratio = t.ratio_for_pointer(&hit, 6, 30).expect("ratio");
        assert!((ratio - 6.0 / 23.0).abs() < 1e-3, "ratio {ratio}");
        assert!(t.set_divider_ratio(&hit, ratio));
        let mut rects = t.compute_layout(24, 80);
        rects.sort_by_key(|r| r.row_off);
        assert_eq!(rects[0].rows, 6);
        assert_eq!(rects[1].row_off, 7);
    }

    /// In a NESTED tree the hit-test targets the correct (inner) split: dragging the
    /// inner divider edits only that split, leaving the outer one untouched.
    #[test]
    fn nested_divider_targets_inner_split() {
        // 1 | (2 / 3): vertical outer, horizontal inner on the right band.
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2);
        t.split_focused(SplitDir::Horizontal, 3);
        let rects = t.compute_layout(24, 80);
        let top = rects.iter().find(|r| r.session == 2).unwrap();
        let bot = rects.iter().find(|r| r.session == 3).unwrap();
        // The inner (horizontal) divider row sits between panes 2 and 3, in the
        // right column band. It is the row just past pane 2's bottom.
        let div_row = top.row_off + top.rows;
        let probe_col = top.col_off + 1;
        let hit = t
            .divider_at(div_row, probe_col, 24, 80)
            .expect("inner divider hit");
        assert_eq!(hit.dir, SplitDir::Horizontal, "inner split is horizontal");
        // Record the OUTER (left) pane width; editing the inner split must not move it.
        let left_before = rects.iter().find(|r| r.session == 1).unwrap().cols;
        // Drag the inner divider up.
        let ratio = t
            .ratio_for_pointer(&hit, top.row_off + 2, probe_col)
            .unwrap();
        assert!(t.set_divider_ratio(&hit, ratio));
        let after = t.compute_layout(24, 80);
        let left_after = after.iter().find(|r| r.session == 1).unwrap().cols;
        assert_eq!(left_before, left_after, "outer split untouched");
        // Pane 2 (the inner first child) actually moved.
        let top_after = after.iter().find(|r| r.session == 2).unwrap();
        assert_ne!(top_after.rows, top.rows, "inner first child resized");
        // Sanity: still three disjoint panes.
        assert_eq!(after.len(), 3);
        assert!(rects_disjoint(&after));
        let _ = bot;
    }

    /// A single-pane (and a zoomed) tab has NO draggable divider.
    #[test]
    fn no_divider_when_single_or_zoomed() {
        let t = PaneTree::new(1);
        assert!(
            t.divider_at(5, 5, 24, 80).is_none(),
            "single pane: no divider"
        );
        let mut z = PaneTree::new(1);
        z.split_focused(SplitDir::Vertical, 2);
        assert!(
            z.divider_at(5, 40, 24, 80).is_some(),
            "split: has a divider"
        );
        z.toggle_zoom();
        assert!(
            z.divider_at(5, 40, 24, 80).is_none(),
            "zoomed: focused pane fills window, no divider"
        );
    }

    /// A stale `DividerHit` whose path no longer names a split is a safe no-op
    /// (`set_divider_ratio` returns false, the tree is untouched).
    #[test]
    fn stale_divider_hit_is_noop() {
        let mut t = PaneTree::new(1);
        t.split_focused(SplitDir::Vertical, 2);
        let hit = t.divider_at(5, 40, 24, 80).unwrap();
        // Collapse back to one pane — the split the hit named is gone.
        t.close_pane(2);
        let before = t.compute_layout(24, 80);
        assert!(!t.set_divider_ratio(&hit, 0.3), "stale path → no write");
        assert_eq!(before, t.compute_layout(24, 80), "tree untouched");
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
