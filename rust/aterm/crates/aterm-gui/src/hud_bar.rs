// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! The bottom PERFORMANCE HUD plus the reusable, themed building blocks that make
//! aterm's HUDs a small framework (`hud` widgets): a streaming sample ring, an
//! auto-scaling color-graded sparkline, fixed-width health-colored fields, dim
//! separators, and a top seam — all linear-blended from the active `Theme`.
//!
//! A HUD is rendered EXACTLY like the tab strip: a row of
//! `aterm_core::terminal::RenderCell`s spliced into the composed `RenderInput`, so
//! it is WYSIWYG on glass AND visible to the `image`/`snapshot` introspection, and
//! goes through the same CPU/GPU renderer (parity holds by construction). The
//! sparkline uses the procedurally-synthesized block glyphs `▁▂▃▄▅▆▇█`
//! (cell-exact, font-independent).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use aterm_core::terminal::{RenderCell, UnderlineStyle};
use aterm_render::Theme;

/// How many recent frames the sparkline / FPS window retains.
const RING_CAP: usize = 64;
/// Frame samples older than this are dropped (keeps FPS + rolling-max honest at idle).
const RING_TTL: Duration = Duration::from_secs(3);
/// Sparkline FLOOR full-scale: the bar auto-scales to `max(this, rolling-max)` so a
/// quiet workload still shows a low staircase (not a flat line) and a slow one stays
/// a *varied* staircase instead of pinning to a solid block. 16ms ≈ the 60fps budget.
const SPARK_FLOOR_MS: f32 = 16.0;

// =============================================================================
// Streaming sample ring — the per-panel state the App owns.
// =============================================================================

#[derive(Clone, Copy)]
struct Sample {
    at: Instant,
    render_ns: u64,
    present_ns: u64,
}

/// Rolling per-frame samples driving the HUD's FPS, sparkline, and WINDOWED maxima
/// (the shared `crate::metrics` module holds only scalar all-time counters — the
/// streaming series + recent maxima live here so the HUD reflects *current*
/// smoothness, not a one-time startup spike).
pub(crate) struct HudSamples {
    ring: VecDeque<Sample>,
}

impl HudSamples {
    pub(crate) fn new() -> Self {
        Self {
            ring: VecDeque::with_capacity(RING_CAP),
        }
    }

    /// Record one presented/rendered frame. `present_ns` is 0 on the headless
    /// `image` path (no on-glass present). Called wherever `metrics::record_present`
    /// is.
    pub(crate) fn record(&mut self, render_ns: u64, present_ns: u64, now: Instant) {
        self.ring.push_back(Sample {
            at: now,
            render_ns,
            present_ns,
        });
        while self.ring.len() > RING_CAP {
            self.ring.pop_front();
        }
        while self.ring.front().is_some_and(|s| {
            now.checked_duration_since(s.at)
                .is_some_and(|a| a > RING_TTL)
        }) {
            self.ring.pop_front();
        }
    }

    fn within(&self, now: Instant, win: Duration) -> impl Iterator<Item = &Sample> {
        self.ring
            .iter()
            .filter(move |s| now.checked_duration_since(s.at).is_some_and(|a| a <= win))
    }

    /// Frames presented within the last second → rolling FPS.
    fn fps(&self, now: Instant) -> u32 {
        u32::try_from(self.within(now, Duration::from_secs(1)).count()).unwrap_or(u32::MAX)
    }

    fn last(&self) -> Option<&Sample> {
        self.ring.back()
    }

    fn max_render_ns(&self) -> u64 {
        self.ring.iter().map(|s| s.render_ns).max().unwrap_or(0)
    }

    fn max_present_ns(&self) -> u64 {
        self.ring.iter().map(|s| s.present_ns).max().unwrap_or(0)
    }

    /// Any real on-glass present recorded (vs the headless image path, all-zero)?
    fn any_present(&self) -> bool {
        self.ring.iter().any(|s| s.present_ns > 0)
    }

    /// The last `width` frame-render times as sparkline levels 0..=8, AUTO-SCALED to
    /// `max(SPARK_FLOOR, rolling-max)` so the staircase stays varied on any workload,
    /// PLUS the aligned per-bar frame-ms (same indexing, left-padded with 0) so the
    /// painter can color each bar by absolute frame health rather than bar height.
    /// Oldest→newest, left-padded with empties (0).
    fn spark(&self, width: usize) -> (Vec<u8>, Vec<f32>) {
        let ms: Vec<f64> = self
            .ring
            .iter()
            .map(|s| s.render_ns as f64 / 1.0e6)
            .collect();
        let levels = levels_autoscaled(&ms, f64::from(SPARK_FLOOR_MS), width);
        let mut ms_aligned = vec![0.0f32; width];
        let n = ms.len().min(width);
        for (i, &v) in ms.iter().rev().take(n).enumerate() {
            ms_aligned[width - 1 - i] = v as f32;
        }
        (levels, ms_aligned)
    }
}

/// Map a value series (oldest→newest) to sparkline levels 0..=8, AUTO-SCALED to
/// `max(floor, series-max)` so the staircase stays varied on any workload (never a
/// flat solid block). The newest `width` values land at the right; `0` values and
/// left-padding are level 0 (blank). Shared by every panel's sparkline.
pub(crate) fn levels_autoscaled(values: &[f64], floor: f64, width: usize) -> Vec<u8> {
    let mut out = vec![0u8; width];
    if width == 0 {
        return out;
    }
    // Fold only finite samples into the scale (a stray NaN/±Inf must not poison the
    // whole staircase), and clamp the floor up off zero so the divide is always safe.
    let scale = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(floor, f64::max)
        .max(f64::MIN_POSITIVE);
    let n = values.len().min(width);
    for (i, &v) in values.iter().rev().take(n).enumerate() {
        out[width - 1 - i] = if !v.is_finite() || v <= 0.0 {
            0
        } else {
            ((v / scale) * 8.0).round().clamp(1.0, 8.0) as u8
        };
    }
    out
}

// =============================================================================
// HudView — a flat, render-ready snapshot handed to the painter.
// =============================================================================

/// Everything the perf HUD draws this frame, built by the `App` from
/// [`crate::metrics::snapshot`] + [`HudSamples`]. WINDOWED maxima come from the ring;
/// `has_present` is false on the headless path (no on-glass latency yet).
pub(crate) struct HudView {
    pub backend_gpu: bool,
    pub fps: u32,
    pub last_frame_ms: f32,
    pub max_frame_ms: f32,
    pub last_present_ms: f32,
    pub max_present_ms: f32,
    pub has_present: bool,
    pub slow_frames: u64,
    pub spark: Vec<u8>,
    /// Per-bar frame-ms aligned 1:1 with `spark`, for absolute-health bar coloring.
    pub spark_ms: Vec<f32>,
}

impl HudView {
    pub(crate) fn build(samples: &HudSamples, now: Instant, spark_width: usize) -> Self {
        let m = crate::metrics::snapshot();
        let ms = |ns: u64| ns as f32 / 1.0e6;
        let last = samples.last();
        let (spark, spark_ms) = samples.spark(spark_width);
        Self {
            backend_gpu: m.backend_gpu,
            fps: samples.fps(now),
            last_frame_ms: ms(last.map_or(0, |s| s.render_ns)),
            max_frame_ms: ms(samples.max_render_ns()),
            last_present_ms: ms(last.map_or(0, |s| s.present_ns)),
            max_present_ms: ms(samples.max_present_ns()),
            has_present: samples.any_present(),
            slow_frames: m.slow_frames,
            spark,
            spark_ms,
        }
    }
}

// =============================================================================
// Reusable themed render helpers (the "framework" layer future panels share).
// =============================================================================

/// On-theme tones, all linear-blended from the active `Theme` (so HUDs track any
/// scheme), mirroring `tab_bar::strip_colors`.
pub(crate) struct HudColors {
    pub bar_bg: [u8; 3],
    pub label: [u8; 3],
    pub value: [u8; 3],
    pub good: [u8; 3],
    pub warn: [u8; 3],
    pub hot: [u8; 3],
}

fn rgb(c: u32) -> [u8; 3] {
    [
        ((c >> 16) & 0xff) as u8,
        ((c >> 8) & 0xff) as u8,
        (c & 0xff) as u8,
    ]
}

fn blend(a: u32, b: u32, t: f32) -> [u8; 3] {
    mix3(rgb(a), rgb(b), t)
}

/// Linear blend of two packed-RGB tones `a` toward `b` by `t ∈ [0,1]`.
fn mix3(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let mix = |x: u8, y: u8| (f32::from(x).mul_add(1.0 - t, f32::from(y) * t)).round() as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

/// Perceptual luma (cheap, no sRGB-linear round-trip — a binary dark/light decision),
/// matching `tab_bar::bg_is_light` so HUD + tab strip classify a theme identically.
fn bg_is_light(bg: [u8; 3]) -> bool {
    let luma = 0.299 * f32::from(bg[0]) + 0.587 * f32::from(bg[1]) + 0.114 * f32::from(bg[2]);
    luma > 150.0
}

/// WCAG relative-luminance contrast ratio between two tones (delegated to the single
/// implementation in `aterm-types`, same one `tab_bar`'s contrast test uses).
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    aterm_types::Rgb::new(a[0], a[1], a[2]).contrast(aterm_types::Rgb::new(b[0], b[1], b[2]))
}

/// Darken/lighten `c` toward the higher-contrast pole (black on a light bar, white on
/// a dark one) JUST enough to clear `target` contrast against `bg`, preserving hue.
/// Falls back to the max-contrast pole if `target` is unreachable, so a health color
/// is NEVER invisible on any theme (the light-theme defect this fixes).
fn ensure_contrast(c: [u8; 3], bg: [u8; 3], target: f64) -> [u8; 3] {
    if contrast(c, bg) >= target {
        return c;
    }
    let anchor = if bg_is_light(bg) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    let mut best = c;
    let mut best_ratio = contrast(c, bg);
    let mut step = 1u8;
    while step <= 10 {
        let m = mix3(c, anchor, f32::from(step) / 10.0);
        let r = contrast(m, bg);
        if r > best_ratio {
            best = m;
            best_ratio = r;
        }
        if r >= target {
            return m;
        }
        step += 1;
    }
    best // unreachable target → strongest available contrast
}

/// On-theme HUD tones. The neutral band/label/value linear-blend from the active
/// theme; the health colors (good/warn/hot) are appearance-aware semantic hues —
/// bright on dark backgrounds, deep on light — each guaranteed-readable against the
/// bar via [`ensure_contrast`]. Mirrors `tab_bar::strip_colors`' light/dark branch so
/// the HUD stays legible (and WCAG-AA, see the contrast test) on every scheme.
pub(crate) fn hud_colors(theme: Theme) -> HudColors {
    let light = bg_is_light(rgb(theme.bg));
    let bar_bg = blend(theme.bg, theme.fg, if light { 0.10 } else { 0.16 });
    // Semantic health hues per appearance. Dark: bright pastels + the theme cursor for
    // "good". Light: deep GitHub-style green/amber/red that read on a pale band.
    let (good_base, warn_base, hot_base) = if light {
        (rgb(0x0019_7A33), rgb(0x009A_6700), rgb(0x00CF_222E))
    } else {
        (rgb(theme.cursor), rgb(0x00F1_FA8C), rgb(0x00FF_6E67))
    };
    // AA for body text is 4.5:1; we aim there and let ensure_contrast fall back to the
    // best available so no value is ever unreadable.
    const AA: f64 = 4.5;
    HudColors {
        bar_bg,
        label: blend(theme.fg, theme.bg, if light { 0.40 } else { 0.48 }),
        value: ensure_contrast(rgb(theme.fg), bar_bg, AA),
        good: ensure_contrast(good_base, bar_bg, AA),
        warn: ensure_contrast(warn_base, bar_bg, AA),
        hot: ensure_contrast(hot_base, bar_bg, AA),
    }
}

/// Health grade by ascending-is-worse value against two thresholds → good/warn/hot.
pub(crate) fn grade_hi(v: f32, warn_at: f32, hot_at: f32, c: &HudColors) -> [u8; 3] {
    if v >= hot_at {
        c.hot
    } else if v >= warn_at {
        c.warn
    } else {
        c.good
    }
}

/// Health grade by descending-is-worse value (e.g. fps) → good/warn/hot.
pub(crate) fn grade_lo(v: f32, warn_below: f32, hot_below: f32, c: &HudColors) -> [u8; 3] {
    if v < hot_below {
        c.hot
    } else if v < warn_below {
        c.warn
    } else {
        c.good
    }
}

/// A HUD cell builder; `seam` draws a thin overline at the cell's top edge so the
/// whole bar reads as a band separated from the terminal content above.
pub(crate) fn cell(ch: char, fg: [u8; 3], bg: [u8; 3], bold: bool, seam: bool) -> RenderCell {
    RenderCell {
        ch,
        fg,
        bg,
        wide: false,
        emoji_presentation: false,
        bold,
        italic: false,
        underline: UnderlineStyle::None,
        strikethrough: false,
        overline: seam,
        underline_color: None,
    }
}

/// A bare HUD-background cell (fills the bar before segments are painted). The seam
/// overline is drawn in the dim label tone, giving a uniform thin top border across
/// the (majority blank) bar.
#[must_use]
pub fn blank_cell(theme: Theme) -> RenderCell {
    let c = hud_colors(theme);
    cell(' ', c.label, c.bar_bg, false, true)
}

/// The sparkline glyph for a level 0..=8 (0 → space; 1..=8 → `▁`..`█`).
fn spark_glyph(level: u8) -> char {
    match level {
        0 => ' ',
        n => char::from_u32(0x2580 + u32::from(n.min(8))).unwrap_or('█'),
    }
}

fn spark_color(level: u8, c: &HudColors) -> [u8; 3] {
    match level {
        0..=3 => c.good,
        4..=6 => c.warn,
        _ => c.hot,
    }
}

/// A left-packing cursor over a HUD row that writes themed, optionally-colored
/// segments and a color-graded sparkline, never overflowing `row`. Shared by all
/// panels so they look identical.
pub(crate) struct RowWriter<'a> {
    row: &'a mut [RenderCell],
    col: usize,
    c: HudColors,
    bar_bg: [u8; 3],
    /// Reused per-field formatting buffer so the fixed-width numeric fields cost zero
    /// heap allocations per paint (the panel sits on the measured present path).
    scratch: String,
}

impl<'a> RowWriter<'a> {
    pub(crate) fn new(row: &'a mut [RenderCell], theme: Theme) -> Self {
        let c = hud_colors(theme);
        let bar_bg = c.bar_bg;
        Self {
            row,
            col: 1, // leading inset
            c,
            bar_bg,
            scratch: String::with_capacity(16),
        }
    }

    pub(crate) fn colors(&self) -> &HudColors {
        &self.c
    }

    pub(crate) fn room(&self) -> usize {
        self.row.len().saturating_sub(self.col)
    }

    /// Write `s` in `fg` (bold optional). Stops at the row edge.
    pub(crate) fn put(&mut self, s: &str, fg: [u8; 3], bold: bool) {
        for ch in s.chars() {
            if self.col >= self.row.len() {
                break;
            }
            self.row[self.col] = cell(ch, fg, self.bar_bg, bold, true);
            self.col += 1;
        }
    }

    /// Like [`put`] for a `format_args!` value, formatting into the reused `scratch`
    /// buffer (no per-field `String` allocation). The `scratch`/`row` borrows are
    /// disjoint fields, so the char loop and the cell writes don't conflict.
    pub(crate) fn put_num(&mut self, args: std::fmt::Arguments<'_>, fg: [u8; 3], bold: bool) {
        use std::fmt::Write as _;
        self.scratch.clear();
        let _ = self.scratch.write_fmt(args);
        for ch in self.scratch.chars() {
            if self.col >= self.row.len() {
                break;
            }
            self.row[self.col] = cell(ch, fg, self.bar_bg, bold, true);
            self.col += 1;
        }
    }

    /// A dim separator between groups: ` │ `.
    pub(crate) fn sep(&mut self) {
        self.put(" \u{2502} ", self.c.label, false);
    }

    /// A sparkline of `levels` (0..=8 each), colored by HEIGHT (low→good, peak→hot).
    /// Use for usage gauges (CPU/mem/throughput) where a taller bar IS the concern.
    pub(crate) fn sparkline(&mut self, levels: &[u8]) {
        for &lvl in levels {
            if self.col >= self.row.len() {
                break;
            }
            self.row[self.col] = cell(
                spark_glyph(lvl),
                spark_color(lvl, &self.c),
                self.bar_bg,
                false,
                true,
            );
            self.col += 1;
        }
    }

    /// A sparkline whose bar HEIGHTS come from `levels` but whose COLORS are supplied
    /// per-bar (e.g. graded by absolute frame-ms health, so fast frames stay green
    /// even when the auto-scaled bar is tall). `colors` aligns 1:1 with `levels`.
    pub(crate) fn sparkline_graded(&mut self, levels: &[u8], colors: &[[u8; 3]]) {
        for (i, &lvl) in levels.iter().enumerate() {
            if self.col >= self.row.len() {
                break;
            }
            let col = colors.get(i).copied().unwrap_or(self.c.good);
            self.row[self.col] = cell(spark_glyph(lvl), col, self.bar_bg, false, true);
            self.col += 1;
        }
    }
}

// =============================================================================
// The perf panel painter.
// =============================================================================

/// Paint the performance HUD into one strip `row` (`row.len()` == cols), pre-filled
/// with [`blank_cell`]. Fixed-width fields (no jitter), health-colored values, an
/// auto-scaled sparkline, honest `—` latency until a real present, dim separators,
/// and a top seam. Degrades right-to-left on a narrow row. Pure / unit-testable.
pub fn paint_hud(row: &mut [RenderCell], v: &HudView, theme: Theme) {
    if row.is_empty() {
        return;
    }
    let mut w = RowWriter::new(row, theme);
    let label = w.colors().label;
    let value = w.colors().value;

    // backend — accent-tinted, fixed 3 chars.
    let backend_col = if v.backend_gpu {
        w.colors().good
    } else {
        w.colors().warn
    };
    w.put(if v.backend_gpu { "gpu" } else { "cpu" }, backend_col, true);
    w.sep();

    // FPS — fixed width (right-justified 3), health-colored (60→good, 30→warn).
    let fps_col = grade_lo(v.fps as f32, 50.0, 30.0, w.colors());
    w.put_num(format_args!("{:>3}", v.fps), fps_col, true);
    w.put(" fps", label, false);
    w.sep();

    // Sparkline — drawn only when the FULL trailing block (frame-ms + latency, ~38
    // cells incl. separators) AND a >=6-cell spark both fit, so the latency readout is
    // never truncated to a stub at common widths. Sized to the remaining gap. Bars are
    // colored by ABSOLUTE frame health (grade_hi on ms), so fast frames stay green even
    // when the auto-scaled bar is tall.
    const TRAILING: usize = 38;
    const MIN_SPARK: usize = 6;
    if w.room() >= TRAILING + MIN_SPARK {
        let want = (w.room() - TRAILING).min(v.spark.len());
        let start = v.spark.len().saturating_sub(want);
        let levels = &v.spark[start..];
        let colors: Vec<[u8; 3]> = v.spark_ms[start..]
            .iter()
            .map(|&ms| {
                if ms <= 0.0 {
                    label
                } else {
                    grade_hi(ms, 8.0, 16.0, w.colors())
                }
            })
            .collect();
        w.sparkline_graded(levels, &colors);
        w.sep();
    }

    // frame render ms — last (health-colored) / max (dim), fixed width.
    let fr_col = grade_hi(v.last_frame_ms, 8.0, 16.0, w.colors());
    w.put_num(format_args!("{:>5.1}", v.last_frame_ms), fr_col, true);
    w.put("/", label, false);
    w.put_num(format_args!("{:>5.1}", v.max_frame_ms), label, false);
    w.put(" ms", label, false);
    w.sep();

    // present latency — honest: '—' until a real on-glass present exists.
    w.put("lat ", label, false);
    if v.has_present {
        let lat_col = grade_hi(v.last_present_ms, 8.0, 16.0, w.colors());
        w.put_num(format_args!("{:>5.1}", v.last_present_ms), lat_col, true);
        w.put("/", label, false);
        w.put_num(format_args!("{:>5.1}", v.max_present_ms), label, false);
        w.put(" ms", label, false);
    } else {
        w.put("  —  ", label, false);
    }

    // slow frames — only when non-zero, hot.
    if v.slow_frames > 0 {
        let hot = w.colors().hot;
        w.sep();
        w.put_num(format_args!("!{} slow", v.slow_frames), hot, true);
    }
    let _ = value; // reserved for future fields
}

// =============================================================================
// The Panel framework — a stack of themed, streaming HUD rows. Adding a new HUD
// is: define a struct, impl `Panel` (paint + optional on_present/poll), register
// it in `App::panels`. All panels share the chrome above (RowWriter / colors /
// grade / sparkline / seam), so they look identical and track the theme.
// =============================================================================

/// Stable identity for a panel (config keys, menu toggles, registry lookup).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PanelId {
    Perf,
    SysLoad,
    Network,
    AppFed,
}

impl PanelId {
    /// Every panel id, in registry/stack order (top → bottom). The single source for a
    /// generic surface (the Performance control panel, config-reload sync, introspection)
    /// to iterate panels without hardcoding the set.
    pub(crate) const ALL: [PanelId; 4] = [
        PanelId::Perf,
        PanelId::SysLoad,
        PanelId::Network,
        PanelId::AppFed,
    ];

    /// The `aterm.toml` / `Config` key that enables this panel — the single source shared
    /// by config load/reload, the Performance control panel's persist, and introspection.
    pub(crate) fn config_key(self) -> &'static str {
        match self {
            PanelId::Perf => "show_perf_hud",
            PanelId::SysLoad => "show_sysload_hud",
            PanelId::Network => "show_network_hud",
            PanelId::AppFed => "show_appfed_hud",
        }
    }

    /// A human label for this panel — the single source shared by the Performance control
    /// panel checkboxes, the View-menu items, and the `controls perf` introspection dump.
    pub(crate) fn label(self) -> &'static str {
        match self {
            PanelId::Perf => "Performance (fps / frame time)",
            PanelId::SysLoad => "System load (CPU / memory)",
            PanelId::Network => "Network (rx / tx)",
            PanelId::AppFed => "App-fed activity (metric streams)",
        }
    }
}

/// One stackable HUD row. `paint` is pure (no I/O); state is fed either at PRESENT
/// time (`on_present`, for frame-coupled panels like Perf) or on the HUD refresh
/// tick (`poll`, for OS/store-sampled panels). Both default to no-op.
pub(crate) trait Panel {
    fn id(&self) -> PanelId;
    fn enabled(&self) -> bool;
    fn set_enabled(&mut self, on: bool);
    /// Paint into a pre-blanked row (`row.len() == cols`); never resize it.
    fn paint(&self, row: &mut [RenderCell], theme: Theme);
    /// Frame-coupled feed (render/present timing). Default: ignored.
    fn on_present(&mut self, _render_ns: u64, _present_ns: u64, _now: Instant) {}
    /// Interval-driven sampling on the HUD tick. Default: ignored.
    fn poll(&mut self, _now: Instant) {}
}

/// Cap for a per-panel sample ring (CPU/net history).
const PANEL_RING: usize = 48;

/// A poll-driven panel's last sample is rendered as `n/a` once it is older than this
/// (the OS probe failed/stopped). Keeps the readout HONEST rather than freezing the
/// last value forever as if it were current. ~16 missed 300ms HUD ticks.
const PANEL_STALE: Duration = Duration::from_secs(5);

/// Is a poll-driven sample stamped at `at` still fresh as of `now`?
fn fresh(at: Option<Instant>, now: Instant) -> bool {
    at.is_some_and(|t| now.saturating_duration_since(t) <= PANEL_STALE)
}

/// Per-interface byte-counter delta, robust to the raw 32-bit `if_data` counters: a
/// normal increase (or a single 32-bit wrap) yields the true delta; an implausibly
/// large backwards jump (a counter RESET, e.g. interface re-init) is treated as 0 so
/// it never shows as a multi-gigabyte one-tick spike.
fn iface_delta(new: u32, prev: u32) -> u64 {
    let d = new.wrapping_sub(prev);
    if d > u32::MAX / 2 { 0 } else { u64::from(d) }
}

fn push_ring(ring: &mut VecDeque<f64>, v: f64) {
    ring.push_back(v);
    while ring.len() > PANEL_RING {
        ring.pop_front();
    }
}

/// Human-readable short number: `850`, `12.0k`, `3.4M`, `1.2G`.
fn fmt_short(v: f64) -> String {
    if !v.is_finite() {
        return "--".to_string();
    }
    if v >= 1.0e9 {
        format!("{:.1}G", v / 1.0e9)
    } else if v >= 1.0e6 {
        format!("{:.1}M", v / 1.0e6)
    } else if v >= 1.0e3 {
        format!("{:.1}k", v / 1.0e3)
    } else {
        format!("{v:.0}")
    }
}

/// Human-readable byte rate: `0 B/s`, `340K/s`, `1.2M/s`.
fn fmt_rate(bps: f64) -> String {
    if !bps.is_finite() {
        return "-- B/s".to_string();
    }
    if bps >= 1.0e9 {
        format!("{:.1}G/s", bps / 1.0e9)
    } else if bps >= 1.0e6 {
        format!("{:.1}M/s", bps / 1.0e6)
    } else if bps >= 1.0e3 {
        format!("{:.0}K/s", bps / 1.0e3)
    } else {
        format!("{bps:.0} B/s")
    }
}

// --- Perf panel (frame render/present timing) -------------------------------

/// The render performance panel: backend, fps, frame-time sparkline, frame-ms,
/// present latency, slow-frame count. Fed at present time.
pub(crate) struct PerfPanel {
    enabled: bool,
    samples: HudSamples,
}

impl PerfPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            samples: HudSamples::new(),
        }
    }
}

impl Panel for PerfPanel {
    fn id(&self) -> PanelId {
        PanelId::Perf
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }
    fn on_present(&mut self, render_ns: u64, present_ns: u64, now: Instant) {
        self.samples.record(render_ns, present_ns, now);
    }
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let view = HudView::build(&self.samples, Instant::now(), 24);
        paint_hud(row, &view, theme);
    }
}

// --- System-load panel (OS scope: CPU load + memory pressure) ---------------

/// Whole-machine CPU load (1-min, normalized by core count) + memory-in-use
/// fraction, sampled on the HUD tick via [`crate::sysmetrics`].
pub(crate) struct SysLoadPanel {
    enabled: bool,
    ncpu: f64,
    total_mem: Option<u64>,
    load: VecDeque<f64>,
    mem: VecDeque<f64>,
    /// When each series was last successfully sampled, for stale → `n/a` decay.
    load_at: Option<Instant>,
    mem_at: Option<Instant>,
}

impl SysLoadPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ncpu: f64::from(crate::sysmetrics::ncpu()).max(1.0),
            total_mem: crate::sysmetrics::mem_total(),
            load: VecDeque::with_capacity(PANEL_RING),
            mem: VecDeque::with_capacity(PANEL_RING),
            load_at: None,
            mem_at: None,
        }
    }
}

impl Panel for SysLoadPanel {
    fn id(&self) -> PanelId {
        PanelId::SysLoad
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }
    fn poll(&mut self, now: Instant) {
        if let Some(l) = crate::sysmetrics::load_avg_1m() {
            push_ring(&mut self.load, l);
            self.load_at = Some(now);
        }
        if let Some(f) = crate::sysmetrics::mem_used_frac() {
            push_ring(&mut self.mem, f);
            self.mem_at = Some(now);
        }
    }
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let now = Instant::now();
        let mut w = RowWriter::new(row, theme);
        let label = w.colors().label;
        // CPU load (1-min), normalized to a per-core fraction for the health color.
        w.put("cpu ", label, false);
        if let (Some(&l), true) = (self.load.back(), fresh(self.load_at, now)) {
            let frac = l / self.ncpu;
            let col = grade_hi(frac as f32, 0.7, 1.0, w.colors());
            w.put_num(format_args!("{l:>4.2}"), col, true);
            let levels = levels_autoscaled(
                &self.load.iter().copied().collect::<Vec<_>>(),
                self.ncpu,
                12,
            );
            w.put(" ", label, false);
            w.sparkline(&levels);
        } else {
            w.put(" n/a", label, false);
        }
        w.sep();
        // Memory in use.
        w.put("mem ", label, false);
        if let (Some(&f), true) = (self.mem.back(), fresh(self.mem_at, now)) {
            let col = grade_hi(f as f32, 0.75, 0.90, w.colors());
            if let Some(total) = self.total_mem {
                let used = f * total as f64;
                w.put(
                    &format!("{}/{}", fmt_short(used), fmt_short(total as f64)),
                    w.colors().value,
                    false,
                );
                w.put(" ", label, false);
            }
            w.put_num(format_args!("{:>3.0}%", f * 100.0), col, true);
            let levels = levels_autoscaled(&self.mem.iter().copied().collect::<Vec<_>>(), 1.0, 12);
            w.put(" ", label, false);
            w.sparkline(&levels);
        } else {
            w.put(" n/a", label, false);
        }
    }
}

// --- Network panel (OS scope: whole-machine rx/tx rate) ---------------------

/// Whole-machine network throughput (rx/tx bytes/sec), from `getifaddrs` byte
/// counters diffed over the HUD tick. Per-process net is not OS-attributable; an
/// app reports its own traffic via the app-fed channel instead.
pub(crate) struct NetworkPanel {
    enabled: bool,
    /// Per-interface raw counters from the previous poll, keyed by interface name, so
    /// each interface is diffed independently (no spike on interface add/remove/wrap).
    prev: HashMap<String, (u32, u32)>,
    prev_at: Option<Instant>,
    /// When a rate was last successfully derived, for stale → `n/a` decay.
    at: Option<Instant>,
    rx: VecDeque<f64>,
    tx: VecDeque<f64>,
}

impl NetworkPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            prev: HashMap::new(),
            prev_at: None,
            at: None,
            rx: VecDeque::with_capacity(PANEL_RING),
            tx: VecDeque::with_capacity(PANEL_RING),
        }
    }
}

impl Panel for NetworkPanel {
    fn id(&self) -> PanelId {
        PanelId::Network
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }
    fn poll(&mut self, now: Instant) {
        let Some(ifaces) = crate::sysmetrics::net_ifaces() else {
            return;
        };
        let cur: HashMap<String, (u32, u32)> =
            ifaces.into_iter().map(|(n, i, o)| (n, (i, o))).collect();
        if let Some(pt) = self.prev_at {
            let dt = now
                .checked_duration_since(pt)
                .map_or(0.0, |d| d.as_secs_f64());
            if dt > 0.0 {
                // Diff EACH interface independently; an interface with no prior baseline
                // (just appeared) contributes 0 this tick rather than its whole counter.
                let (mut drx, mut dtx) = (0u64, 0u64);
                for (name, &(ci, co)) in &cur {
                    if let Some(&(pi, po)) = self.prev.get(name) {
                        drx += iface_delta(ci, pi);
                        dtx += iface_delta(co, po);
                    }
                }
                push_ring(&mut self.rx, drx as f64 / dt);
                push_ring(&mut self.tx, dtx as f64 / dt);
                self.at = Some(now);
            }
        }
        self.prev = cur;
        self.prev_at = Some(now);
    }
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let now = Instant::now();
        let mut w = RowWriter::new(row, theme);
        let label = w.colors().label;
        let good = w.colors().good;
        w.put("net ", label, false);
        if self.rx.is_empty() || !fresh(self.at, now) {
            w.put("n/a", label, false);
            return;
        }
        let rx_bps = self.rx.back().copied().unwrap_or(0.0);
        let tx_bps = self.tx.back().copied().unwrap_or(0.0);
        w.put("\u{2193}", w.colors().value, false); // ↓
        w.put(&format!(" {} ", fmt_rate(rx_bps)), good, false);
        w.sparkline(&levels_autoscaled(
            &self.rx.iter().copied().collect::<Vec<_>>(),
            1024.0,
            10,
        ));
        w.sep();
        w.put("\u{2191}", w.colors().value, false); // ↑
        w.put(&format!(" {} ", fmt_rate(tx_bps)), good, false);
        w.sparkline(&levels_autoscaled(
            &self.tx.iter().copied().collect::<Vec<_>>(),
            1024.0,
            10,
        ));
    }
}

// --- App-fed panel (AI token spend / any process-reported metric) -----------

/// Renders the app-fed metric streams (`crate::app_fed`) — e.g. an AI tool's
/// `tokens.in`/`tokens.out` counters pushed via `aterm-ctl metric`. Each stream
/// shows its latest value, derived per-second rate, and a throughput sparkline.
pub(crate) struct AppFedPanel {
    enabled: bool,
    /// Snapshot refreshed on the HUD `poll` tick (~3 fps), so `paint` (which runs on
    /// the measured present path, potentially every frame) never takes the global
    /// `app_fed` store lock nor rebuilds the per-stream views under contention with
    /// the control-thread writer. Mirrors how SysLoad/Network sample on `poll`.
    cache: Vec<crate::app_fed::StreamView>,
}

impl AppFedPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            cache: Vec::new(),
        }
    }
}

impl Panel for AppFedPanel {
    fn id(&self) -> PanelId {
        PanelId::AppFed
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }
    fn poll(&mut self, now: Instant) {
        self.cache = crate::app_fed::snapshot(now, 8);
    }
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let mut w = RowWriter::new(row, theme);
        let label = w.colors().label;
        let streams = &self.cache;
        if streams.is_empty() {
            w.put("feed ", w.colors().good, true);
            w.put(
                "(no streams — pipe with: aterm-ctl metric <name> <value>)",
                label,
                false,
            );
            return;
        }
        let mut first = true;
        for s in streams {
            if !first {
                w.sep();
            }
            first = false;
            if w.room() < 12 {
                break;
            }
            w.put(&s.name, w.colors().good, true);
            w.put(&format!(" {}", fmt_short(s.last)), w.colors().value, false);
            if s.rate > 0.0 {
                w.put(&format!(" {}/s", fmt_short(s.rate)), label, false);
            }
            w.put(" ", label, false);
            w.sparkline(&s.spark);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> HudView {
        HudView {
            backend_gpu: true,
            fps: 60,
            last_frame_ms: 4.2,
            max_frame_ms: 9.1,
            last_present_ms: 0.8,
            max_present_ms: 3.0,
            has_present: true,
            slow_frames: 0,
            spark: vec![1, 2, 3, 4, 5, 6, 7, 8],
            spark_ms: vec![2.0, 3.0, 4.0, 6.0, 8.0, 10.0, 14.0, 20.0],
        }
    }

    #[test]
    fn paints_within_bounds_and_keeps_the_bar_band() {
        let theme = Theme::default();
        let bar_bg = hud_colors(theme).bar_bg;
        for cols in [1usize, 10, 40, 80, 200] {
            let mut row = vec![blank_cell(theme); cols];
            paint_hud(&mut row, &view(), theme);
            assert_eq!(row.len(), cols, "paint must not resize the row");
            for cellv in &row {
                assert_eq!(cellv.bg, bar_bg, "every HUD cell keeps the themed bar bg");
                assert!(cellv.overline, "every HUD cell carries the top seam");
            }
        }
    }

    #[test]
    fn latency_is_honest_until_a_real_present() {
        let theme = Theme::default();
        let mut v = view();
        v.has_present = false;
        let mut row = vec![blank_cell(theme); 120];
        paint_hud(&mut row, &v, theme);
        let text: String = row.iter().map(|c| c.ch).collect();
        assert!(
            text.contains("lat   —"),
            "headless latency shows an em-dash, got {text:?}"
        );
    }

    /// Regression for the mis-sized sparkline budget that truncated the latency block
    /// to a `lat   0.` stub across the most common terminal widths. The sparkline must
    /// only draw when the FULL trailing block (frame-ms + present-latency, both `ms`
    /// fields) still fits, so the headline latency is never clipped.
    #[test]
    fn latency_readout_is_not_truncated_at_common_widths() {
        let theme = Theme::default();
        for cols in [56usize, 64, 70, 80, 100, 120] {
            let mut row = vec![blank_cell(theme); cols];
            paint_hud(&mut row, &view(), theme);
            let text: String = row.iter().map(|c| c.ch).collect();
            assert!(
                text.matches("ms").count() >= 2,
                "cols={cols}: both frame and present 'ms' fields must render, got {text:?}"
            );
            assert!(
                text.contains("0.8"),
                "cols={cols}: the present-latency value must not be truncated, got {text:?}"
            );
        }
    }

    #[test]
    fn sparkline_auto_scales_so_uniform_slow_frames_are_not_all_full_block() {
        // All frames ~40ms: with a FIXED 16ms scale every bar would be █ (level 8);
        // auto-scaling to the rolling max spreads them so the staircase stays varied.
        let mut s = HudSamples::new();
        let now = Instant::now();
        for (i, &ns) in [40_000_000u64, 20_000_000, 40_000_000, 10_000_000]
            .iter()
            .enumerate()
        {
            s.record(ns, 0, now + Duration::from_millis(i as u64 * 10));
        }
        let (levels, _ms) = s.spark(4);
        assert!(
            levels.iter().any(|&l| l < 8),
            "auto-scaled sparkline must not pin every slow frame to a full block: {levels:?}"
        );
        assert!(
            levels.contains(&8),
            "the rolling-max frame should reach the top: {levels:?}"
        );
    }

    /// Regression for the perf sparkline being colored by autoscaled bar HEIGHT (so a
    /// uniformly-healthy workload painted an alarming all-yellow/red staircase). Bars
    /// must be colored by ABSOLUTE frame health: fast frames stay `good` even when the
    /// auto-scaled bar is at full height.
    #[test]
    fn perf_sparkline_colors_track_frame_health_not_bar_height() {
        let theme = Theme::default();
        let good = hud_colors(theme).good;
        let mut v = view();
        v.spark = vec![8; 8]; // all bars at full height (autoscaled to the window)
        v.spark_ms = vec![2.0; 8]; // ...but every frame is a fast 2ms (healthy)
        let mut row = vec![blank_cell(theme); 140];
        paint_hud(&mut row, &v, theme);
        let spark_cells: Vec<&RenderCell> = row
            .iter()
            .filter(|c| ('\u{2581}'..='\u{2588}').contains(&c.ch))
            .collect();
        assert!(
            !spark_cells.is_empty(),
            "sparkline should render at 140 cols"
        );
        assert!(
            spark_cells.iter().all(|c| c.fg == good),
            "healthy fast frames must color the sparkline 'good', not by bar height"
        );
    }

    #[test]
    fn iface_delta_handles_increase_wrap_and_reset() {
        // Normal increase.
        assert_eq!(iface_delta(1000, 400), 600);
        assert_eq!(iface_delta(400, 400), 0);
        // A genuine 32-bit counter wrap across the boundary → the true small delta.
        assert_eq!(iface_delta(100, u32::MAX - 99), 200);
        // A counter RESET (large backwards jump) → 0, not a multi-GB phantom spike.
        assert_eq!(iface_delta(0, 1000), 0);
        assert_eq!(iface_delta(5, 3_000_000), 0);
    }

    #[test]
    fn poll_samples_decay_to_stale_after_the_ttl() {
        let now = Instant::now();
        assert!(!fresh(None, now), "never-sampled is not fresh");
        assert!(fresh(Some(now), now), "just-sampled is fresh");
        assert!(
            fresh(Some(now), now + PANEL_STALE - Duration::from_millis(1)),
            "within the TTL is fresh"
        );
        assert!(
            !fresh(Some(now), now + PANEL_STALE + Duration::from_secs(1)),
            "past the TTL is stale → n/a"
        );
    }

    #[test]
    fn sparkline_levels_map_to_block_glyphs() {
        assert_eq!(spark_glyph(0), ' ');
        assert_eq!(spark_glyph(1), '▁');
        assert_eq!(spark_glyph(8), '█');
        assert_eq!(spark_glyph(9), '█');
    }

    /// Every HUD health color (good/warn/hot) and the value/label tones must stay
    /// READABLE against the bar background on EVERY built-in scheme — dark AND light.
    /// This is the regression guard for the light-theme defect where the old fixed-hex
    /// warn/hot collapsed to ~1.2:1 (invisible). Mirrors `tab_bar`'s contrast test; the
    /// 3.0:1 floor is the WCAG-AA large/bold-text + non-text-contrast threshold (the
    /// values render bold), while `hud_colors` itself aims for 4.5:1.
    #[test]
    fn hud_colors_meet_wcag_aa_on_every_builtin_scheme() {
        for name in aterm_types::scheme::builtin_names() {
            let s = aterm_types::scheme::builtin(name).expect("builtin exists");
            let tp = s.to_theme_parts();
            let theme = Theme {
                fg: tp.fg,
                bg: tp.bg,
                cursor: tp.cursor,
                selection: tp.selection,
            };
            let c = hud_colors(theme);
            for (role, fg) in [
                ("good", c.good),
                ("warn", c.warn),
                ("hot", c.hot),
                ("value", c.value),
            ] {
                let ratio = contrast(fg, c.bar_bg);
                assert!(
                    ratio >= 3.0,
                    "{name}: HUD {role} contrast {ratio:.2} < 3.0:1 against the bar"
                );
            }
        }
    }
}
