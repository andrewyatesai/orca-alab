// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Frame composition / redraw: the per-window present path (`redraw_window`), the
//! split-pane composition (`redraw_compose`), the in-grid tab-strip splice, blink
//! sync, and the resize plumbing — plus the pure render helpers they use
//! (`should_repaint`, divider/blit/prepend, the pixel→cell geometry). A verbatim
//! inherent-impl split of `App`; no logic change to the hot present path.

use std::num::NonZeroU32;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aterm_core::terminal::{CursorStyle, RenderCell, Terminal};
use aterm_render::{Frame, RenderInput, Theme};
use winit::dpi::PhysicalSize;

use crate::{
    App, BLINK_INTERVAL, Backend, PresentTarget, RepaintKey, SelectionFingerprint, WindowId,
    hud_bar, metrics, pane, tab_bar, term_lock,
};

/// The redraw early-out decision (D-1), as a PURE function so it is unit
/// testable without a window/event loop.
///
/// Returns `true` (must repaint) iff this is the first frame (`prev` is `None`)
/// or any presented-state term changed since the last present. Returns `false`
/// (skip the extract + rasterize + present) only when the previously presented
/// key is byte-identical to the current one — i.e. a steady screen with the same
/// blink phase, no bell flash, the same selection and cursor override. This is
/// what eliminates the steady-screen and blink-only-wake full-frame redraws.
pub(crate) fn should_repaint(prev: Option<RepaintKey>, cur: RepaintKey) -> bool {
    prev != Some(cur)
}

/// I-2: invert a frame's RGB in place when a visual-bell flash is `active`,
/// matching the on-screen present's invert (CPU `src ^ 0x00ff_ffff`; the GPU
/// blit shader does the same). Packed `0x00RRGGBB`, so XOR the low 24 bits and
/// leave the unused top byte clear. A no-op when no flash is active, so the
/// steady-screen snapshot path is byte-identical to before.
pub(crate) fn apply_bell_invert(frame: &mut Frame, active: bool) {
    if !active {
        return;
    }
    for px in &mut frame.pixels {
        *px ^= 0x00ff_ffff;
    }
}

/// Device-pixel thickness of the drop-target accent border, scaled to the window
/// and clamped so it stays a thin frame on small windows and never dominates a
/// large one.
fn drop_border_px(w: usize, h: usize) -> usize {
    (w.min(h) / 200).clamp(2, 6)
}

/// Alpha (out of 255) of the faint full-grid accent wash and the inset border.
const DROP_WASH_ALPHA: u32 = 28; // ~11% — readable content underneath
const DROP_BORDER_ALPHA: u32 = 235; // ~92% — a crisp but not harsh frame

/// Blend `fg` over `bg` (both packed `0x00RRGGBB`) at alpha `a` (0..=255), per
/// channel, leaving the top byte clear. `a == 0` returns `bg`; `a == 255` returns
/// `fg`. The canonical coverage blend (mirrors the renderer's private `blend`).
fn blend_rgb(bg: u32, fg: u32, a: u32) -> u32 {
    let inv = 255 - a;
    let r = (((bg >> 16) & 0xff) * inv + ((fg >> 16) & 0xff) * a) / 255;
    let g = (((bg >> 8) & 0xff) * inv + ((fg >> 8) & 0xff) * a) / 255;
    let b = ((bg & 0xff) * inv + (fg & 0xff) * a) / 255;
    (r << 16) | (g << 8) | b
}

/// Composite the drag-and-drop drop-target highlight over a packed `0x00RRGGBB`
/// framebuffer: a faint `accent` wash across the whole grid plus a near-opaque
/// `accent` border inset at the window edge (the chosen "inset accent border +
/// faint wash" treatment). `pixels` is row-major `w * h` (any trailing pixels are
/// ignored). Pure + allocation-free, and shared by the live CPU present and the
/// headless `image`/`snapshot` so on-glass and introspection match. The GPU
/// backend reproduces the same look in its blit shader.
pub(crate) fn apply_drop_overlay(pixels: &mut [u32], w: usize, h: usize, accent: u32) {
    if w == 0 || h == 0 {
        return;
    }
    let border = drop_border_px(w, h);
    let accent = accent & 0x00ff_ffff;
    for y in 0..h {
        let edge_row = y < border || y >= h - border;
        let Some(row) = pixels.get_mut(y * w..y * w + w) else {
            break;
        };
        for (x, px) in row.iter_mut().enumerate() {
            let on_border = edge_row || x < border || x >= w - border;
            let a = if on_border { DROP_BORDER_ALPHA } else { DROP_WASH_ALPHA };
            *px = blend_rgb(*px & 0x00ff_ffff, accent, a);
        }
    }
}

#[allow(
    clippy::items_after_test_module,
    reason = "these unit tests sit next to the drop-overlay helpers they cover; the rest of the file is the App render inherent-impl, not stray items"
)]
#[cfg(test)]
mod drop_overlay_tests {
    use super::{apply_drop_overlay, blend_rgb, drop_border_px, DROP_WASH_ALPHA};

    fn channel_dist(a: u32, b: u32) -> u32 {
        let d = |s: u32| (((a >> s) & 0xff) as i32 - ((b >> s) & 0xff) as i32).unsigned_abs();
        d(16) + d(8) + d(0)
    }

    /// The border pixels land much closer to the accent than the interior wash,
    /// and the interior is exactly the faint accent blend over the background.
    #[test]
    fn border_is_accent_heavy_interior_is_faint() {
        let (w, h) = (400usize, 300usize);
        let accent = 0x0050_FA7B;
        let mut px = vec![0x0000_0000u32; w * h]; // black background
        apply_drop_overlay(&mut px, w, h, accent);

        let corner = px[0]; // on the border
        let interior = px[(h / 2) * w + w / 2]; // far from any edge
        assert_ne!(corner, 0, "border pixel must be tinted");
        assert_ne!(interior, 0, "interior pixel must be washed");
        assert!(
            channel_dist(corner, accent) < channel_dist(interior, accent),
            "border should be nearer the accent than the interior"
        );
        assert_eq!(interior, blend_rgb(0, accent, DROP_WASH_ALPHA));
        assert!(drop_border_px(w, h) >= 2);
    }

    /// Degenerate dimensions and a no-window case are no-ops, never panics.
    #[test]
    fn zero_dims_is_noop() {
        let mut px = vec![0x0011_2233u32; 4];
        apply_drop_overlay(&mut px, 0, 0, 0x00ff_ffff);
        assert_eq!(px, vec![0x0011_2233u32; 4]);
    }

    /// The packed format is preserved: the unused top byte stays clear.
    #[test]
    fn top_byte_stays_clear() {
        let mut px = vec![0x00ab_cdefu32; 10 * 10];
        apply_drop_overlay(&mut px, 10, 10, 0x00ff_ffff);
        assert!(px.iter().all(|p| p & 0xff00_0000 == 0));
    }
}

/// Pure pixel→TERMINAL-cell mapping (the body of [`App::pixel_to_cell`], extracted
/// so the tab-strip row offset is unit-testable without a backend/window). Two
/// insets are removed from the raw window pixel before mapping, in order:
///   * `pad` — the interior padding border around the WHOLE window (strip included),
///     subtracted from BOTH `x` and `y` (a saturating subtract maps a click in the
///     top/left border to row/col 0);
///   * `strip_rows * ch` — the tab strip occupies the top `strip_rows` pixel rows
///     of the (already pad-inset) grid, so a click in the terminal region lands on
///     the right terminal row and a click in the strip clamps to terminal row 0.
///
/// The result is clamped to the terminal grid. `pad == 0` && `strip_rows == 0` is
/// the byte-identical pre-strip, pre-pad mapping.
#[allow(
    clippy::too_many_arguments,
    reason = "pure pixel->cell geometry over independent scalar inputs (x, y, dims, pad, strip rows); a struct would not clarify the mapping"
)]
pub(crate) fn pixel_to_term_cell(
    x: f64,
    y: f64,
    cw: usize,
    ch: usize,
    rows: u16,
    cols: u16,
    strip_rows: u16,
    pad: usize,
) -> (u16, u16) {
    let gx = (x as usize).saturating_sub(pad);
    let gy = (y as usize).saturating_sub(pad);
    let strip_px = strip_rows as usize * ch.max(1);
    let term_y = gy.saturating_sub(strip_px);
    let col = (gx / cw.max(1)).min(cols.saturating_sub(1) as usize) as u16;
    let row = (term_y / ch.max(1)).min(rows.saturating_sub(1) as usize) as u16;
    (row, col)
}

/// Pure "is this pixel in the tab strip, and if so which strip column?" (the body
/// of [`App::strip_col_at`], extracted for unit tests). The interior `pad` border
/// is removed from both axes first (the strip lives inside the pad), then `None`
/// when the pad-inset `y` is at/below the strip's pixel height (`strip_rows * ch`)
/// — i.e. in the terminal region. A click in the top `pad` band over the strip
/// still maps to the strip (gy saturates to 0). `pad == 0` is byte-identical.
pub(crate) fn strip_col_for_pixel(
    x: f64,
    y: f64,
    cw: usize,
    ch: usize,
    cols: u16,
    strip_rows: u16,
    pad: usize,
) -> Option<u16> {
    let gx = (x as usize).saturating_sub(pad);
    let gy = (y as usize).saturating_sub(pad);
    let strip_px = strip_rows as usize * ch.max(1);
    if gy >= strip_px {
        return None;
    }
    Some((gx / cw.max(1)).min(cols.saturating_sub(1) as usize) as u16)
}

/// Selection-drag AUTOSCROLL trigger: given the pointer's raw window pixel `y`, the
/// interior `pad`, the tab-strip pixel height (`strip_rows * ch`), the cell height
/// `ch`, and the terminal `rows`, return the number of scrollback lines to move so a
/// drag PAST the top/bottom viewport edge extends the selection into off-screen
/// content. Positive = scroll toward OLDER history (drag above the top edge); negative
/// = scroll toward the live BOTTOM (drag below the bottom edge); `0` = the pointer is
/// inside the grid, no autoscroll.
///
/// The magnitude grows with how far past the edge the pointer is (one line per cell
/// height of overshoot, min 1), so a fast flick to the window edge scrolls briskly
/// while a hair past the edge creeps — the familiar text-editor feel. Pure (no
/// window/term), so the edge math is unit-testable.
pub(crate) fn selection_autoscroll_lines(
    y: f64,
    pad: usize,
    strip_px: usize,
    ch: usize,
    rows: u16,
) -> i32 {
    let ch = ch.max(1);
    let top = (pad + strip_px) as f64; // first device pixel of terminal row 0
    let bottom = top + (rows as usize * ch) as f64; // one past the last terminal row
    if y < top {
        // Above the top edge → scroll into history. One line per cell-height of
        // overshoot (min 1), so the further out, the faster.
        let over = (top - y) as usize;
        (over / ch + 1) as i32
    } else if y >= bottom {
        // Below the bottom edge → scroll toward the live bottom (negative offset).
        let over = (y - bottom) as usize;
        -((over / ch + 1) as i32)
    } else {
        0
    }
}

/// Shift the composed frame `dst` DOWN by `strip_rows.len()` rows and prepend those
/// painted tab-strip rows at the top, keeping every per-row vector
/// (`cells`/`clusters`/`combining`/`images`/`line_sizes`) aligned and moving the
/// cursor + row count down with the content. Pure (the body of
/// [`App::splice_tab_strip`]'s mutation), so the row-offset math is unit-testable on
/// a bare [`RenderInput`]. An empty `strip_rows` is a no-op (byte-identical).
pub(crate) fn prepend_strip_rows(dst: &mut RenderInput, strip_rows: Vec<Vec<RenderCell>>) {
    let strip = strip_rows.len();
    if strip == 0 {
        return;
    }
    for (i, srow) in strip_rows.into_iter().enumerate() {
        dst.cells.insert(i, srow);
    }
    // Per-row sparse / sized data: prepend empty/default rows so indices stay aligned
    // with `cells`. `clusters`/`combining`/`images` are sparse (empty vecs);
    // `line_sizes` defaults to single-width.
    for _ in 0..strip {
        dst.clusters.insert(0, Vec::new());
        dst.combining.insert(0, Vec::new());
        dst.images.insert(0, Vec::new());
        dst.line_sizes
            .insert(0, aterm_core::grid::LineSize::SingleWidth);
    }
    // The cursor (terminal-grid row) is now `strip` rows lower in the window.
    dst.cursor_row += strip;
    dst.rows += strip;
    // The strip changes the presented pixels; bump the snapshot seq so the renderer's
    // content cache sees the new frame.
    dst.snapshot_seq = dst.snapshot_seq.wrapping_add(1);
}

/// Append `hud_rows` painted HUD rows at the BOTTOM of the composed frame `dst`,
/// keeping every per-row vector aligned. The bottom analog of [`prepend_strip_rows`]:
/// it does NOT move the cursor (the HUD sits BELOW the terminal grid + cursor), it
/// just grows the grid downward. Because the renderer maps row `r` to pixel band
/// `pad + r*cell_h`, the appended rows land in the bottom band automatically. Pure /
/// unit-testable; an empty `hud_rows` is a no-op (byte-identical).
pub(crate) fn append_hud_rows(dst: &mut RenderInput, hud_rows: Vec<Vec<RenderCell>>) {
    let n = hud_rows.len();
    if n == 0 {
        return;
    }
    for row in hud_rows {
        dst.cells.push(row);
        dst.clusters.push(Vec::new());
        dst.combining.push(Vec::new());
        dst.images.push(Vec::new());
        dst.line_sizes.push(aterm_core::grid::LineSize::SingleWidth);
    }
    dst.rows += n;
    dst.snapshot_seq = dst.snapshot_seq.wrapping_add(1);
}

/// A divider cell for the gaps BETWEEN split panes: a blank glyph filled with a
/// mid-tone background so the 1-cell line reads as a visible seam regardless of
/// font glyph coverage. The colour is a 50/50 blend of the theme's foreground and
/// background, so it contrasts on both dark and light themes.
pub(crate) fn divider_cell(theme: Theme) -> RenderCell {
    let mix = |shift: u32| {
        let a = ((theme.fg >> shift) & 0xff) as u16;
        let b = ((theme.bg >> shift) & 0xff) as u16;
        ((a + b) / 2) as u8
    };
    let seam = [mix(16), mix(8), mix(0)];
    RenderCell {
        ch: ' ',
        fg: seam,
        bg: seam,
        wide: false,
        emoji_presentation: false,
        bold: false,
        italic: false,
        underline: aterm_core::terminal::UnderlineStyle::None,
        strikethrough: false,
        overline: false,
        underline_color: None,
    }
}

/// SPLIT-PANE composition: fill `dst` with a `rows`×`cols` grid of divider cells
/// (the seam colour), reset to no cursor / no clusters / single-width rows. The
/// per-pane blit then overwrites each pane's rectangle; the cells left untouched
/// are exactly the 1-cell divider gaps between panes.
pub(crate) fn fill_divider_grid(dst: &mut RenderInput, rows: usize, cols: usize, theme: Theme) {
    let seam = divider_cell(theme);
    dst.rows = rows;
    dst.cols = cols;
    dst.cells.resize_with(rows, Vec::new);
    for row in &mut dst.cells {
        row.clear();
        row.resize(cols, seam);
    }
    dst.clusters.clear();
    dst.clusters.resize_with(rows, Vec::new);
    dst.combining.clear();
    dst.combining.resize_with(rows, Vec::new);
    dst.line_sizes.clear();
    dst.line_sizes
        .resize(rows, aterm_core::grid::LineSize::SingleWidth);
    dst.cursor_visible = false;
    dst.cursor_row = 0;
    dst.cursor_col = 0;
    dst.display_offset = 0;
}

/// Blit one pane's snapshot `src` (sized to the pane's sub-rect) into the
/// composite `dst` at cell offset `(row_off, col_off)`. Copies the resolved cells,
/// the sparse emoji-cluster / combining-mark per-row data (column-shifted by
/// `col_off`), and the per-row line size. Bounds-checked so a pane that slightly
/// overflows a degenerate tiny window can never write past the composite.
pub(crate) fn blit_pane_into(
    dst: &mut RenderInput,
    src: &RenderInput,
    row_off: usize,
    col_off: usize,
) {
    for (sr, src_row) in src.cells.iter().enumerate() {
        let dr = row_off + sr;
        let Some(dst_row) = dst.cells.get_mut(dr) else {
            break;
        };
        for (sc, cell) in src_row.iter().enumerate() {
            let dc = col_off + sc;
            if let Some(slot) = dst_row.get_mut(dc) {
                *slot = *cell;
            }
        }
        if let Some(ls) = src.line_sizes.get(sr)
            && let Some(dst_ls) = dst.line_sizes.get_mut(dr)
        {
            *dst_ls = *ls;
        }
        if let Some(dst_clusters) = dst.clusters.get_mut(dr)
            && let Some(src_clusters) = src.clusters.get(sr)
        {
            for (c, s) in src_clusters {
                dst_clusters.push((col_off + c, s.clone()));
            }
        }
        if let Some(dst_comb) = dst.combining.get_mut(dr)
            && let Some(src_comb) = src.combining.get(sr)
        {
            for (c, m) in src_comb {
                dst_comb.push((col_off + c, m.clone()));
            }
        }
    }
}

impl App {
    /// Resize every pane of EVERY tab of window `wid`'s engine + PTY to its computed
    /// sub-rect (cell geometry). A pane that fills its whole tab (no split) gets the
    /// full window grid — byte-identical to the single-session resize. Records the
    /// geometry change into each session's asciicast, exactly like `apply_term_resize`.
    pub(crate) fn resize_panes(&mut self, wid: WindowId) {
        let Some(ws) = self.windows.get(&wid) else {
            return;
        };
        let (rows, cols) = (ws.rows, ws.cols);
        // Collect the (session_id, sub_rows, sub_cols) for every pane of every tab.
        let mut targets: Vec<(u64, u16, u16)> = Vec::new();
        for tree in &ws.layouts {
            for r in tree.compute_layout(rows, cols) {
                targets.push((r.session, r.rows.max(1), r.cols.max(1)));
            }
        }
        let mut shared_changed: Vec<u64> = Vec::new();
        for (id, sub_rows, sub_cols) in targets {
            // A SHARED (Cmd-Shift-O) session has ONE grid co-viewed by several
            // windows; it can't be two sizes. Drive it to the element-wise MIN across
            // all viewers so no window over-reads it (a bigger viewer letterboxes the
            // surplus; a smaller one sees the min) — instead of reflowing the shared
            // grid to whichever window happened to resize. A non-shared session keeps
            // its own computed sub-rect (byte-identical to before).
            let shared = self.pool.views(id).is_some_and(|v| v > 1);
            let (sub_rows, sub_cols) = if shared {
                self.shared_target_geometry(id)
            } else {
                (sub_rows, sub_cols)
            };
            let Some(s) = self.pool.get(id) else { continue };
            {
                let mut term = term_lock(&s.term);
                if term.rows() == sub_rows && term.cols() == sub_cols {
                    continue; // already this size: no engine/PTY churn
                }
                term.resize(sub_rows, sub_cols);
            }
            if shared {
                shared_changed.push(id);
            }
            aterm_pty::resize(s.master, sub_rows, sub_cols);
            // Record the geometry change into this pane's asciicast (A.5.1 #1):
            // `[t, "r", "<cols>x<rows>"]` on the recorder's own timeline. Off the
            // reader hot path; main thread, lock uncontended here.
            {
                let mut rec = s.ctx.cast.lock().unwrap_or_else(|p| p.into_inner());
                let t = rec.now();
                rec.record_resize(t, sub_cols, sub_rows);
            }
            // Temporal spine (B.9): resize is a first-class recorded event
            // (reflow is path-dependent, never re-ordered — B.2.3). The pane's
            // engine is sized to its sub-rect, so record the sub-rect geometry.
            {
                s.ctx
                    .temporal
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .record_resize(sub_rows, sub_cols);
            }
        }
        // A shared session's grid changed → every co-viewing window's framed view of
        // it changed (different letterbox / sub-view), so repaint them all. The
        // resizing window `wid` also repaints via its own resize path; a duplicate
        // `request_redraw` is coalesced. Empty in the common (non-shared) case.
        for id in shared_changed {
            for ws in self.windows.values() {
                if ws.layouts.iter().any(|t| t.contains(id))
                    && let Some(w) = ws.os_window.as_ref()
                {
                    w.request_redraw();
                }
            }
        }
    }

    /// The glyph cell size in pixels, from the live rasterizer (GPU's internal
    /// CPU face, or the standalone CPU renderer).
    pub(crate) fn cell_size(&self) -> (usize, usize) {
        self.backend.cell_size()
    }

    /// The window/swapchain pixel size for a `total_rows`×`cols` grid, INCLUDING
    /// the renderer's interior padding border (`2·pad` per axis). `total_rows` is
    /// the WHOLE composed grid the renderer presents — the terminal rows PLUS the
    /// tab-strip rows above them (the strip is spliced in as real grid rows). This
    /// is the single place window geometry is derived, so the on-screen surface,
    /// the GPU swapchain, and the offscreen framebuffer the `image` verb reads all
    /// agree. With `pad == 0` and `tab_strip_rows == 0` this is the historical
    /// `cols·cell_w × rows·cell_h`.
    pub(crate) fn frame_px(&self, total_rows: u16, cols: u16) -> PhysicalSize<u32> {
        let (w, h) = self.backend.frame_size(total_rows as usize, cols as usize);
        PhysicalSize::new(w as u32, h as u32)
    }

    /// The window pixel size for the CURRENT terminal grid: the terminal rows plus
    /// the tab strip above, padded. The canonical window/swapchain size — every
    /// window-create / resize / grid-resize path routes through this so the strip
    /// AND the interior padding are always accounted for in lockstep.
    pub(crate) fn window_frame_px(&self, rows: u16, cols: u16) -> PhysicalSize<u32> {
        self.frame_px(
            rows.saturating_add(self.tab_strip_rows)
                .saturating_add(self.hud_rows),
            cols,
        )
    }

    /// Push the current blink phase into the rasterizer.
    pub(crate) fn sync_blink_phase(&mut self) {
        let phase = self.front().is_none_or(|ws| ws.blink_phase);
        self.backend.set_cursor_blink_phase(phase);
    }

    /// Force the blink phase ON (cursor solid) and restart the blink period —
    /// the standard "cursor is solid while you type" behavior. Repaints only
    /// if the phase actually changed.
    pub(crate) fn reset_blink(&mut self, wid: WindowId) {
        let mut flipped = false;
        if let Some(ws) = self.windows.get_mut(&wid) {
            if ws.next_blink.is_some() {
                ws.next_blink = Some(Instant::now() + BLINK_INTERVAL);
            }
            if !ws.blink_phase {
                ws.blink_phase = true;
                flipped = true;
            }
        }
        if flipped {
            self.sync_blink_phase();
            if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
                w.request_redraw();
            }
        }
    }

    pub(crate) fn redraw_window(&mut self, id: WindowId) {
        // Frame wall-clock start, read back into the `metrics` verb's
        // `last_frame_render_ms` on an actual present (early-out frames return before
        // `record_present`, so they never count). One `Instant::now()` per redraw.
        let frame_started = Instant::now();
        let Some(ws0) = self.windows.get(&id) else {
            return;
        };
        let Some(window) = ws0.os_window.clone() else {
            return;
        };
        // No present target yet (surface not created): nothing to draw into, and
        // we must NOT consume damage, so bail before touching the lock.
        match ws0.present.as_ref() {
            Some(PresentTarget::Gpu { .. }) if self.backend.is_gpu() => {}
            Some(PresentTarget::Cpu { .. }) if !self.backend.is_gpu() => {}
            // Present target absent, or backend/target kind mismatch (transient
            // during a backend rebuild): nothing valid to draw into.
            _ => return,
        }
        let (rows, cols) = (ws0.rows as usize, ws0.cols as usize);
        // Visual bell: the presented frame has its RGB inverted while a flash is
        // active. The flash state machine decides "active"; `about_to_wait` wakes
        // the loop at its deadline so the normal frame returns.
        let invert = ws0.bell_flash.is_active(Instant::now());
        // Unfocused windows force a hollow cursor (mirrors `on_focus`); part of
        // the visual state the grid damage tracker doesn't see.
        let cursor_override = (!ws0.focused).then_some(CursorStyle::HollowBlock);
        let blink_phase = ws0.blink_phase;
        // Drag-and-drop hover: while a file is dragged over the window we paint a
        // drop-target highlight at present time (like the bell invert above).
        let drag_hover = ws0.drag_hover;
        let last_present = ws0.last_present;

        // Renderer-global cursor state belongs to whichever window we are about to
        // encode: the shared backend's blink phase + focus-driven hollow override are
        // not per-window, so re-apply THIS window's values right before the encode
        // (last-writer-wins once more than one window exists). Redundant but harmless
        // at n==1 (sync_blink_phase/on_focus already set the same values).
        self.backend.set_cursor_blink_phase(blink_phase);
        self.backend.set_cursor_style_override(cursor_override);

        // D-1 early-out. Hold the Terminal mutex only long enough to read the
        // damage epoch + selection + title and, IF we decide to repaint, refill
        // the persistent RenderInput in place and consume the damage — all
        // atomically so no PTY damage is dropped. The early-out compares this
        // frame's RepaintKey to the last presented one: a steady screen with the
        // same blink phase / bell-flash / selection / focus skips the entire
        // extract + rasterize + present (the coarse screen-level skip, on top of
        // the renderer's own row-level damage cache in `render_input_cached`).
        //
        // SPLIT PANES: a multi-pane tab composes the frame from EVERY visible pane
        // (see `redraw_compose`), so its early-out folds all visible panes' damage.
        // The single-pane path below is the EXACT original, byte-identical.
        let multi_pane = self.active_tree(id).is_some_and(|t| t.len() > 1);
        // The tab-strip titles must be read OUTSIDE the term lock (reading each tab's
        // title locks its term); read them ONCE here and reuse for BOTH the RepaintKey
        // fingerprint and the strip splice below (instead of locking every tab twice).
        // The fingerprint is part of the RepaintKey, so it MUST be computed before the
        // early-out — a title change has to invalidate it. BUT a single-tab window
        // draws a blank seam (no titles — see `splice_tab_strip_with`), so its strip
        // content is invariant: skip the per-tab title read + lock entirely and use a
        // constant fingerprint. The lock + title read run only when the strip is
        // enabled AND there are 2+ tabs (the only case a title actually shows). Opening
        // a 2nd tab flips this branch, changing `tab_strip` and forcing the repaint.
        // Strip disabled: byte-identical to the pre-strip path (empty, fp 0, no-op).
        let (strip_titles, tab_strip) = self.redraw_tab_strip_state(id);
        let title = if multi_pane {
            match self.redraw_compose(id, rows, cols, invert, drag_hover, cursor_override, tab_strip)
            {
                Some(title) => title,
                None => {
                    // Nothing visible changed across any pane: refresh chrome, skip.
                    let title = self
                        .windows
                        .get(&id)
                        .map_or_else(String::new, |ws| term_lock(&ws.term).title().to_string());
                    self.apply_title(id, &window, &title);
                    return;
                }
            }
        } else {
            let Some(ws) = self.windows.get_mut(&id) else {
                return;
            };
            let mut term = term_lock(&ws.term);
            let key = RepaintKey {
                damage_epoch: term.damage_epoch(),
                blink_phase,
                invert,
                drag_hover,
                cursor_override,
                selection: SelectionFingerprint::of(term.text_selection()),
                tab_strip,
            };
            let title = term.title().to_string();
            // The HUD streams its own values (fps/latency/sparkline) independent of
            // terminal damage, so when it is on we never take the content early-out —
            // the HUD_INTERVAL timer drives a bounded ~3fps refresh.
            if self.hud_rows == 0 && !should_repaint(last_present, key) {
                // Nothing visible changed since the last present. Drop the lock,
                // refresh only the window chrome (a title-only change needs no
                // pixel repaint), and skip the frame entirely.
                drop(term);
                self.apply_title(id, &window, &title);
                return;
            }
            // We are committing to present this frame: REFILL the reused snapshot
            // in place (no per-frame container-Vec alloc when dims are stable) and
            // consume the damage under the SAME lock; render after the guard drops.
            // A-3: the ENGINE builds the snapshot (`Terminal::cell_frame_into`); the
            // renderer is a pure consumer of `RenderInput`.
            term.cell_frame_into(&mut ws.input_scratch, rows, cols);
            term.take_damage();
            ws.last_present = Some(key);
            title
        };
        // HONEST render cost (fixes the HUD observer effect): capture the TERMINAL
        // compose time HERE — after the grid snapshot is built but BEFORE the chrome
        // (tab strip + HUD) is spliced and before the present — so turning the HUD on
        // does not inflate the very `frame_ms` it reports. The HUD's own paint cost
        // (row alloc + numeric formatting + sparkline) and the present both fall
        // outside this window; present latency is reported separately below.
        let render_ns = frame_started.elapsed().as_nanos() as u64;
        // SPLICE the visible tab strip ABOVE the just-filled terminal grid (shifting
        // the content + cursor down by `tab_strip_rows`). A no-op when the strip is
        // disabled, so `input_scratch` is then the terminal grid exactly as before
        // (byte-identical). Both the single-pane and composed paths funnel here.
        self.splice_tab_strip_with(id, tab_strip, strip_titles);
        // SPLICE the performance HUD into the bottom row (a no-op when off).
        self.splice_hud_bar(id);
        // Reflect the program-set title (OSC 0/2) in the window chrome, falling
        // back to "aterm" when nothing has set one. Only calls set_title on an
        // actual change (a cheap String compare on the already-unlocked path).
        self.apply_title(id, &window, &title);

        // Present the just-filled `input_scratch` into this window's surface. A
        // borrow/target mismatch (transient during a backend rebuild) aborts the
        // frame WITHOUT recording metrics — the same as the inline early returns.
        // While a file is dragged over the window, paint the drop-target highlight
        // in the theme accent (`theme.cursor`); `None` keeps the present path
        // byte-identical to before this feature.
        let overlay = drag_hover.then_some(self.theme.cursor);
        if !self.present_input_scratch(id, invert, overlay) {
            return;
        }
        let present_latency_ns = self.present_latency_ns();
        // Publish this frame's timing to the process-global metrics counters, read
        // back over the control socket's `metrics` verb so a driving AI can measure
        // responsiveness directly. Off the correctness path; only on a real present.
        // `render_ns` was captured above (terminal compose only, HUD-exclusive).
        metrics::record_present(present_latency_ns, render_ns);
        // Feed the HUD's rolling sample ring (fps window + sparkline) from the SAME
        // real presents the metrics counters see.
        let hud_now = std::time::Instant::now();
        for p in &mut self.panels {
            p.on_present(render_ns, present_latency_ns, hud_now);
        }
        // Frame-pacing: stamp this present so the soft cap in the `Wake::Output`
        // handler coalesces sub-`MIN_FRAME_INTERVAL` bursts against it. Reached only
        // on a REAL present (the D-1 early-out returns before this when the screen is
        // unchanged), so the cap measures from genuine frames, not skipped ones.
        if let Some(ws) = self.windows.get_mut(&id) {
            ws.last_present_at = Some(std::time::Instant::now());
        }
        // Publish the freshly-presented screen to assistive tech (macOS VoiceOver)
        // when the `a11y-appkit` feature is on. Reaches here only on an ACTUAL
        // present (the D-1 early-out returns before this), so a steady screen costs
        // nothing; a no-op on the default build and off-macOS.
        self.update_accessibility(id, &window);
        let _ = window;
    }

    /// Compute this window's tab-strip titles + fingerprint for the current frame.
    ///
    /// The fingerprint feeds the RepaintKey, so it MUST be computed before the
    /// early-out — a title change has to invalidate it. A single-tab window draws a
    /// blank seam (no titles — see `splice_tab_strip_with`), so its strip content is
    /// invariant: skip the per-tab title read + lock entirely and use a constant
    /// fingerprint. The lock + title read run only when the strip is enabled AND
    /// there are 2+ tabs (the only case a title actually shows). Opening a 2nd tab
    /// flips this branch, changing the fingerprint and forcing the repaint. Strip
    /// disabled: byte-identical to the pre-strip path (empty, fp 0, no-op).
    fn redraw_tab_strip_state(&self, id: WindowId) -> (Vec<String>, u64) {
        let tab_count = self.windows.get(&id).map_or(0, |ws| ws.layouts.len());
        if self.tab_strip_enabled() && tab_count >= 2 {
            let titles = self.tab_titles(id);
            let active = self.windows.get(&id).map_or(0, |ws| ws.tabs.active);
            let fp = self.tab_strip_fingerprint_from(&titles, active);
            (titles, fp)
        } else {
            (Vec::new(), 0)
        }
    }

    /// Present the window's filled `input_scratch` into its surface via the active
    /// backend. Returns `false` (frame aborted, NO metrics recorded) on a present
    /// target / backend-kind mismatch — transient during a backend rebuild — exactly
    /// as the inline early returns did; `true` once the present has been issued.
    fn present_input_scratch(&mut self, id: WindowId, invert: bool, overlay: Option<u32>) -> bool {
        // Disjoint borrows: the renderer (`self.backend`) and the target window's
        // present target + input snapshot are SEPARATE fields of `self`, so
        // destructuring lets both be borrowed mutably at once with no aliasing.
        let App {
            backend, windows, ..
        } = self;
        let Some(ws) = windows.get_mut(&id) else {
            return false;
        };
        if backend.is_gpu() {
            // GPU on-glass present: render the offscreen frame (the single source
            // of truth) and BLIT it straight into the swapchain — no Frame, no
            // softbuffer copy, no GPU->CPU readback. The blit shader applies the
            // visual-bell invert. The same offscreen texture is what the
            // snapshot/`image` introspection reads back, so screen == introspection.
            let input = &ws.input_scratch;
            if let (
                Some(gpu),
                Some(PresentTarget::Gpu {
                    gpu_surface,
                    window_gpu,
                }),
            ) = (backend.gpu_mut(), ws.present.as_mut())
            {
                // Map the accent → the GPU overlay params (the alphas are this
                // module's single source of truth; the GPU derives the border
                // thickness from the framebuffer size to match the CPU path).
                let gpu_overlay = overlay.map(|accent| aterm_gpu::DropOverlay {
                    accent,
                    wash_a: DROP_WASH_ALPHA as u8,
                    border_a: DROP_BORDER_ALPHA as u8,
                });
                gpu.present_input(window_gpu, gpu_surface, input, invert, gpu_overlay);
            } else {
                return false;
            }
        } else {
            // CPU present: rasterize via the renderer's damage-tracked cache and
            // take a BORROW of the framebuffer (`render_input_cached`) rather than
            // an owned `Frame` — eliding the per-frame cache→Frame clone — then
            // copy it into the softbuffer surface, applying the visual-bell invert
            // per pixel. The only full-framebuffer copy left is cache→surface.
            let Some(PresentTarget::Cpu { surface, .. }) = ws.present.as_mut() else {
                return false;
            };
            let view = match backend {
                // `&mut ws.cpu_cache` (this window's damage cache) and
                // `&ws.input_scratch` are disjoint sub-borrows of `ws`; `r` borrows
                // `backend`, which is a sibling field of `windows`, so all three are
                // non-aliasing. The cache is per-window (S5c), so two windows on one
                // CPU `Renderer` keep their damage tracking isolated.
                Backend::Cpu(r) => r.render_input_cached(&mut ws.cpu_cache, &ws.input_scratch),
                Backend::Gpu(_) => return false,
            };
            let pixels = view.pixels();
            let (w, h) = (view.width().max(1) as u32, view.height().max(1) as u32);
            surface
                .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                .ok();
            if let Ok(mut buf) = surface.buffer_mut() {
                let n = buf.len().min(pixels.len());
                if invert {
                    for (dst, &src) in buf[..n].iter_mut().zip(&pixels[..n]) {
                        *dst = src ^ 0x00ff_ffff;
                    }
                } else {
                    buf[..n].copy_from_slice(&pixels[..n]);
                }
                for px in buf.iter_mut().skip(n) {
                    *px = 0;
                }
                // Drag-and-drop highlight: composite the inset accent border + faint
                // wash over the just-filled framebuffer (after the bell invert, so
                // it reads as chrome on top). A no-op allocation-free pass; skipped
                // entirely when nothing is hovered.
                if let Some(accent) = overlay {
                    apply_drop_overlay(&mut buf[..n], w as usize, h as usize, accent);
                }
                let _ = buf.present();
            }
        }
        true
    }

    /// Latency self-introspection: the frame is now presented. If an output burst is
    /// pending, return how long it waited from "content ready" to "presented"
    /// (output->present) — aterm's render-pipeline latency, the slice of
    /// input-to-photon software controls. swap(0) so the next burst's leading edge
    /// restarts the clock; `$ATERM_TRACE_LATENCY` keeps the stderr log, but the
    /// number is always returned for the `metrics` verb regardless.
    fn present_latency_ns(&self) -> u64 {
        let stamp = self.last_output_ns.swap(0, Ordering::Relaxed);
        if stamp != 0 {
            let now = self.lat_epoch.elapsed().as_nanos() as u64;
            let dt = now.saturating_sub(stamp);
            if self.trace_latency {
                eprintln!("aterm-latency output->present: {:.2} ms", dt as f64 / 1e6);
            }
            dt
        } else {
            0
        }
    }

    /// SPLIT PANES: compose the active tab's frame from EVERY visible pane and fill
    /// `input_scratch` at window size, ready for the SAME present path the
    /// single-pane redraw uses (CPU/GPU consume `input_scratch` unchanged — no
    /// renderer change). Returns `Some(focused_title)` when a present is needed, or
    /// `None` on the D-1 early-out (nothing visible changed across any pane).
    ///
    /// The combined early-out folds every visible pane's `damage_epoch` (so a
    /// background-pane write in this tab still repaints) plus the focused pane's
    /// blink/invert/cursor-override/selection state. On a repaint it lays out the
    /// panes, locks each in turn, refills `pane_scratch`, and blits its cells into
    /// `input_scratch` at the pane's offset; the FOCUSED pane's cursor is the only
    /// solid cursor (others draw none), and 1-cell dividers fill the gaps.
    #[allow(
        clippy::too_many_arguments,
        reason = "a window's full compose inputs (id/dims/invert/drag-hover/cursor-override/tab-strip); bundling them into a struct only relocates the argument list"
    )]
    pub(crate) fn redraw_compose(
        &mut self,
        wid: WindowId,
        rows: usize,
        cols: usize,
        invert: bool,
        drag_hover: bool,
        cursor_override: Option<CursorStyle>,
        tab_strip: u64,
    ) -> Option<String> {
        // Read theme BEFORE borrowing `ws` (fill_divider_grid needs it after the
        // ws borrow is live). Layout + per-pane state come from window `wid`.
        let theme = self.theme;
        let ws = self.windows.get(&wid)?;
        let tree = &ws.layouts[ws.tabs.active];
        let focus = tree.focus();
        let blink_phase = ws.blink_phase;
        let last_present = ws.last_present;
        let rects = tree.compute_layout(ws.rows, ws.cols);
        // Fold every visible pane's damage epoch into one key term (wrapping add is
        // fine — the early-out only needs the combination to CHANGE on any change).
        let mut damage_epoch: u64 = 0;
        let mut focus_selection =
            SelectionFingerprint::of(&aterm_core::selection::TextSelection::new());
        // Clone each pane's `term` handle OUT of the `&self`/`ws` borrow so the
        // mutating composition loop below can write this window's `input_scratch`/
        // `pane_scratch` freely. Cheap: an `Arc` clone per visible pane. Panes whose
        // session was just torn down (impossible mid-redraw) are skipped.
        let panes: Vec<(pane::PaneRect, Arc<Mutex<Terminal>>)> = rects
            .iter()
            .filter_map(|r| self.pool.get(r.session).map(|s| (*r, s.term.clone())))
            .collect();
        for (r, term) in &panes {
            let mut term = term_lock(term);
            // Per-pane damage is window-scoped via the per-window `last_present`
            // (read above); the take_damage below is per-session, but the early-out
            // compares against THIS window's key, so a co-viewer window is not
            // starved (it keeps its own last_present and re-folds the same epochs).
            damage_epoch = damage_epoch.wrapping_add(term.damage_epoch());
            if r.session == focus {
                focus_selection = SelectionFingerprint::of(term.text_selection());
            }
        }
        let key = RepaintKey {
            damage_epoch,
            blink_phase,
            invert,
            drag_hover,
            cursor_override,
            selection: focus_selection,
            tab_strip,
        };
        // HUD on → never early-out (it streams independently; see redraw_window).
        if self.hud_rows == 0 && !should_repaint(last_present, key) {
            return None;
        }
        // Commit to presenting. Re-borrow `ws` mutably now (the immutable borrow
        // above is dropped). Fill the composite: window-size grid of divider cells
        // first, then overlay each pane.
        let ws = self.windows.get_mut(&wid)?;
        fill_divider_grid(&mut ws.input_scratch, rows, cols, theme);
        let mut focus_title = String::new();
        for (r, term) in &panes {
            let (sub_rows, sub_cols) = (r.rows as usize, r.cols as usize);
            let (cursor, title) = {
                let mut term = term_lock(term);
                term.cell_frame_into(&mut ws.pane_scratch, sub_rows, sub_cols);
                term.take_damage();
                // The cursor (window coords) is drawn SOLID only in the focused
                // pane; other panes contribute none.
                let cursor = (r.session == focus && ws.pane_scratch.cursor_visible).then_some((
                    ws.pane_scratch.cursor_row,
                    ws.pane_scratch.cursor_col,
                    ws.pane_scratch.cursor_style,
                ));
                (cursor, term.title().to_string())
            };
            // `pane_scratch` and `input_scratch` are disjoint fields of `ws`.
            blit_pane_into(
                &mut ws.input_scratch,
                &ws.pane_scratch,
                r.row_off as usize,
                r.col_off as usize,
            );
            if r.session == focus {
                focus_title = title;
                match cursor {
                    Some((cr, cc, style)) => {
                        ws.input_scratch.cursor_row = r.row_off as usize + cr;
                        ws.input_scratch.cursor_col = r.col_off as usize + cc;
                        ws.input_scratch.cursor_visible = true;
                        ws.input_scratch.cursor_style = style;
                    }
                    None => ws.input_scratch.cursor_visible = false,
                }
            }
        }
        // A composed frame has no single selection (cross-pane selection is
        // deferred); the focused pane's text is selectable only when it fills the
        // window (the single-pane path). Stamp a fresh seq so the cache sees change.
        ws.input_scratch.selection = aterm_core::selection::TextSelection::new();
        ws.input_scratch.snapshot_seq = ws.input_scratch.snapshot_seq.wrapping_add(1);
        ws.last_present = Some(key);
        Some(focus_title)
    }

    /// Splice the VISIBLE tab strip into the top `tab_strip_rows` rows of the
    /// just-composed `input_scratch` frame, shifting the terminal content (and the
    /// cursor) DOWN by `tab_strip_rows`. Called from `redraw` after either the
    /// single-pane or composed path filled `input_scratch` at TERMINAL size
    /// (`self.rows × self.cols`); the result is the FULL-window frame
    /// (`(self.rows + tab_strip_rows) × self.cols`) the renderer presents.
    ///
    /// A no-op when the strip is disabled (`tab_strip_rows == 0`) — `input_scratch`
    /// is then the terminal grid exactly as before, so the present + oracle paths are
    /// byte-identical. The strip's laid-out segments are cached in `self.tab_segments`
    /// for click hit-testing. The session grids are NEVER touched — only the composed
    /// `RenderInput` is shifted (so a program's cursor row, reflow, and SIGWINCH
    /// geometry are unchanged).
    pub(crate) fn splice_tab_strip(&mut self, wid: WindowId) {
        if self.tab_strip_rows == 0 {
            return;
        }
        // Cold callers (snapshot / oracle paths) read the titles + fingerprint here;
        // the redraw hot path computes them ONCE and calls `splice_tab_strip_with`
        // directly, sharing the work with the RepaintKey fingerprint.
        let titles = self.tab_titles(wid);
        let active = self.windows.get(&wid).map_or(0, |ws| ws.tabs.active);
        let tab_strip = self.tab_strip_fingerprint_from(&titles, active);
        self.splice_tab_strip_with(wid, tab_strip, titles);
    }

    /// Splice with the strip `tab_strip` fingerprint + `titles` already computed by
    /// the caller (the redraw path reuses the ones it built for the RepaintKey, so
    /// each tab's terminal is locked ONCE per present, not twice). E3: when the
    /// fingerprint AND column width match the last build, the painted strip rows are
    /// REUSED from `cached_strip_rows` — the common present (terminal content moved,
    /// the strip did not) skips the `layout_segments` + `paint_strip` rebuild. The
    /// output is byte-identical either way (the cache is keyed on exactly what the
    /// rows are painted from: fingerprint = count+active+titles, plus `cols`).
    pub(crate) fn splice_tab_strip_with(
        &mut self,
        wid: WindowId,
        tab_strip: u64,
        titles: Vec<String>,
    ) {
        let strip = self.tab_strip_rows as usize;
        if strip == 0 {
            return;
        }
        let (cols, tab_count, active) = match self.windows.get(&wid) {
            Some(ws) => (ws.cols as usize, ws.layouts.len(), ws.tabs.active),
            None => return,
        };
        let cache_key = (tab_strip, cols);
        let hit = self.windows.get(&wid).is_some_and(|ws| {
            ws.last_strip_fp == Some(cache_key) && ws.cached_strip_rows.len() == strip
        });
        if !hit {
            // Rebuild: lay out the segments + paint the labels onto the LAST strip row
            // (upper rows stay bare chrome). Cache the rows + segments for reuse.
            //
            // SINGLE-TAB: a window with one tab shows a CLEAN body-coloured seam — no
            // tab button, no ✕, no `+` — so a lone session reads like a plain terminal
            // (and the OS title bar already shows its title), not a TUI tab widget. The
            // chrome appears only once a 2nd tab exists. `tab_strip_fingerprint_from`
            // folds the tab COUNT, so opening/closing the 2nd tab invalidates this
            // cache and repaints. `tab_segments` is cleared too, so a click in the
            // blank seam does nothing (no invisible `+` to hit). (The row is still
            // RESERVED — fully reclaiming it needs per-window window-resize on the
            // 1↔2 transition; tracked as a follow-up.)
            let theme = self.theme;
            let segments = if tab_count >= 2 {
                tab_bar::layout_segments(cols as u16, tab_count, active)
            } else {
                Vec::new()
            };
            let rows: Vec<Vec<RenderCell>> = (0..strip)
                .map(|r| {
                    let mut row = vec![tab_bar::blank_cell(theme); cols];
                    if r + 1 == strip && !segments.is_empty() {
                        tab_bar::paint_strip(&mut row, &segments, &titles, active, theme);
                    }
                    row
                })
                .collect();
            if let Some(ws) = self.windows.get_mut(&wid) {
                ws.tab_segments = segments;
                ws.cached_strip_rows = rows;
                ws.last_strip_fp = Some(cache_key);
            }
        }
        // Shift the composed frame DOWN by `strip` rows, prepending the (cached)
        // strip rows. Clone the cache so it stays intact for the next present.
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        let strip_rows = ws.cached_strip_rows.clone();
        prepend_strip_rows(&mut ws.input_scratch, strip_rows);
    }

    /// Splice the HUD panel STACK into the bottom rows of the composed frame (the
    /// bottom analog of [`Self::splice_tab_strip_with`]): one themed `RenderCell` row
    /// per ENABLED panel, top→bottom in registry order. A no-op when no panel is on
    /// (`hud_rows == 0`). Called from both the on-screen present and the headless
    /// `image`/`snapshot` paths, so the HUD is WYSIWYG and introspectable. Rows are
    /// rebuilt each present (they stream), but they are a handful of rows — trivial.
    pub(crate) fn splice_hud_bar(&mut self, wid: WindowId) {
        if self.hud_rows == 0 {
            return;
        }
        // `cap` = HUD rows this window can show (`min(hud_rows, hud_cap)`); a window too
        // short for the full stack drops the bottom panels rather than producing a frame
        // taller than the window. `u16::MAX` cap (no resize yet / headless) ⇒ all shown.
        let (cols, cap) = match self.windows.get(&wid) {
            Some(ws) => (ws.cols as usize, self.hud_rows.min(ws.hud_cap) as usize),
            None => return,
        };
        if cap == 0 {
            return;
        }
        let theme = self.theme;
        // Build one row per enabled panel (top of the stack first), capped to `cap`.
        // `self.panels` and `self.windows` are disjoint fields, so the read-then-get_mut
        // below is borrow-clean.
        let mut rows: Vec<Vec<RenderCell>> = Vec::with_capacity(cap);
        for p in self.panels.iter().filter(|p| p.enabled()).take(cap) {
            let mut row = vec![hud_bar::blank_cell(theme); cols];
            p.paint(&mut row, theme);
            rows.push(row);
        }
        if let Some(ws) = self.windows.get_mut(&wid) {
            append_hud_rows(&mut ws.input_scratch, rows);
        }
    }

    /// Apply a `(rows, cols)` grid resize to the engine + PTY + GPU swapchain
    /// (the geometry the main thread owns). The CPU softbuffer resizes itself in
    /// `redraw` from the Frame dims. No-op when the geometry is unchanged. Shared
    /// by the window `Resized` path and the control-socket resize (RES-1).
    ///
    /// TABS + PANES: rows/cols are WINDOW-level, so a resize re-lays EVERY tab's
    /// panes of window `wid` and resizes each pane's engine + PTY to ITS sub-rect
    /// (not just the active one) — a background tab/pane kept at the old size would
    /// reflow wrongly the moment it became visible, and its app (vim/htop) would see
    /// a stale `SIGWINCH` geometry. With one pane per tab this is the same single
    /// resize as before (the pane fills the whole window).
    pub(crate) fn apply_term_resize(&mut self, wid: WindowId, rows: u16, cols: u16) -> bool {
        let (cw, ch) = self.cell_size();
        // Report the real cell pixel metric to THIS window's panes' engines so
        // inline images (iTerm2 OSC 1337 `File=`) sized in pixels/percent land on
        // the right cell footprint. Pushed before the no-op early-return so every
        // session stays in sync with the font in use.
        if let Some(ids) = self.windows.get(&wid).map(|ws| {
            ws.layouts
                .iter()
                .flat_map(|t| t.sessions())
                .collect::<Vec<_>>()
        }) {
            for id in ids {
                if let Some(s) = self.pool.get(id) {
                    term_lock(&s.term).set_cell_pixel_size(cw as u16, ch as u16);
                }
            }
        }
        if Some((rows, cols)) == self.windows.get(&wid).map(|ws| (ws.rows, ws.cols)) {
            return false;
        }
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.rows = rows;
            ws.cols = cols;
        }
        // Resize every pane (of every tab of THIS window) to its computed sub-rect;
        // with no splits each pane fills its whole tab = the full window grid.
        // `resize_panes` records each pane's asciicast + temporal-spine resize event.
        self.resize_panes(wid);
        // GPU mode: reconfigure THIS window's swapchain to the new framebuffer pixel
        // size (the PADDED full-window grid: terminal rows + the tab strip above,
        // plus the `2·pad` interior border) so the blit target matches the frame the
        // renderer encodes. `frame_size` reads the renderer's live `pad`; with
        // `pad == 0` && `tab_strip_rows == 0` this is the original `rows * ch`.
        // Include the bottom HUD band too so the swapchain matches the spliced frame —
        // using the window's EFFECTIVE HUD rows (`min(hud_rows, hud_cap)`) so a window
        // too short for the full stack sizes the swapchain to what actually renders.
        let eff_hud = self
            .hud_rows
            .min(self.windows.get(&wid).map_or(u16::MAX, |ws| ws.hud_cap));
        let strip = (self.tab_strip_rows + eff_hud) as usize;
        let App {
            backend, windows, ..
        } = self;
        if let Some(ws) = windows.get_mut(&wid)
            && let (Some(gpu), Some(PresentTarget::Gpu { gpu_surface, .. })) =
                (backend.gpu_mut(), ws.present.as_mut())
        {
            let win_rows = rows as usize + strip;
            let (w_px, h_px) = gpu.frame_size(win_rows, cols as usize);
            gpu.resize_surface(gpu_surface, w_px as u32, h_px as u32);
        }
        true
    }

    /// RES-1: a control-socket `resize` verb landed on the main thread (via
    /// `Wake::Input` carrying an `InputEvent::Resize { echo_to_window: true }`).
    /// Apply the term/PTY/framebuffer resize, then ask the window to match the new
    /// grid pixel size so the on-screen geometry tracks the engine (the window
    /// `Resized` event that follows is a no-op — the grid already matches). Finally
    /// request a redraw so the resized screen is presented. Without this the verb
    /// left `App.rows/cols` + framebuffer stale and sent no Wake, so a follow-up
    /// `image`/`dims` disagreed. The interactive window-resize path uses
    /// [`Self::apply_term_resize`] directly (no `request_inner_size`) so it never
    /// fights an edge-drag.
    pub(crate) fn apply_grid_resize(&mut self, rows: u16, cols: u16) {
        // The control `resize` verb follows the active/front window.
        let Some(wid) = self.frontmost_window else {
            return;
        };
        let changed = self.apply_term_resize(wid, rows, cols);
        if !changed {
            return;
        }
        // Request the FULL window size (terminal rows + the tab strip above, plus
        // the `2·pad` interior border) so the on-screen geometry tracks the engine.
        // `window_frame_px` folds in the strip AND the pad; with both zero this keeps
        // the original request (byte-identical).
        let size = self.window_frame_px(rows, cols);
        if let Some(w) = self.front().and_then(|ws| ws.os_window.as_ref()) {
            // A best-effort request; the WM may clamp. The engine/PTY geometry is
            // already authoritative regardless of what the window settles on.
            let _ = w.request_inner_size(size);
            w.request_redraw();
        }
    }
}
