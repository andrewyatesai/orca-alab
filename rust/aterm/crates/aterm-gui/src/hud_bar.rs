// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

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

use std::collections::VecDeque;
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
    /// `max(SPARK_FLOOR, rolling-max)` so the staircase stays varied on any workload.
    /// Oldest→newest, left-padded with empties (0).
    fn spark(&self, width: usize) -> Vec<u8> {
        let ms: Vec<f64> = self
            .ring
            .iter()
            .map(|s| s.render_ns as f64 / 1.0e6)
            .collect();
        levels_autoscaled(&ms, f64::from(SPARK_FLOOR_MS), width)
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
    let scale = values
        .iter()
        .copied()
        .fold(floor, f64::max)
        .max(f64::MIN_POSITIVE);
    let n = values.len().min(width);
    for (i, &v) in values.iter().rev().take(n).enumerate() {
        out[width - 1 - i] = if v <= 0.0 {
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
}

impl HudView {
    pub(crate) fn build(samples: &HudSamples, now: Instant, spark_width: usize) -> Self {
        let m = crate::metrics::snapshot();
        let ms = |ns: u64| ns as f32 / 1.0e6;
        let last = samples.last();
        Self {
            backend_gpu: m.backend_gpu,
            fps: samples.fps(now),
            last_frame_ms: ms(last.map_or(0, |s| s.render_ns)),
            max_frame_ms: ms(samples.max_render_ns()),
            last_present_ms: ms(last.map_or(0, |s| s.present_ns)),
            max_present_ms: ms(samples.max_present_ns()),
            has_present: samples.any_present(),
            slow_frames: m.slow_frames,
            spark: samples.spark(spark_width),
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
    let (a, b) = (rgb(a), rgb(b));
    let mix = |x: u8, y: u8| (f32::from(x).mul_add(1.0 - t, f32::from(y) * t)).round() as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

pub(crate) fn hud_colors(theme: Theme) -> HudColors {
    HudColors {
        bar_bg: blend(theme.bg, theme.fg, 0.16), // subtly raised band
        label: blend(theme.fg, theme.bg, 0.48),  // dim comment-gray labels
        value: rgb(theme.fg),                    // primary readout
        good: rgb(theme.cursor),                 // cursor-green (healthy)
        warn: rgb(0x00F1_FA8C),                  // tempered bright-yellow
        hot: rgb(0x00FF_6E67),                   // tempered bright-red
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

    /// A dim separator between groups: ` │ `.
    pub(crate) fn sep(&mut self) {
        self.put(" \u{2502} ", self.c.label, false);
    }

    /// A color-graded sparkline of `levels` (0..=8 each).
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
    w.put(&format!("{:>3}", v.fps), fps_col, true);
    w.put(" fps", label, false);
    w.sep();

    // Sparkline — only if there's comfortable room for the trailing fields.
    if w.room() > 30 {
        let want = w.room().saturating_sub(28).clamp(6, v.spark.len().max(6));
        let start = v.spark.len().saturating_sub(want);
        let levels = v.spark[start..].to_vec();
        w.sparkline(&levels);
        w.sep();
    }

    // frame render ms — last (health-colored) / max (dim), fixed width.
    let fr_col = grade_hi(v.last_frame_ms, 8.0, 16.0, w.colors());
    w.put(&format!("{:>5.1}", v.last_frame_ms), fr_col, true);
    w.put("/", label, false);
    w.put(&format!("{:>5.1}", v.max_frame_ms), label, false);
    w.put(" ms", label, false);
    w.sep();

    // present latency — honest: '—' until a real on-glass present exists.
    w.put("lat ", label, false);
    if v.has_present {
        let lat_col = grade_hi(v.last_present_ms, 8.0, 16.0, w.colors());
        w.put(&format!("{:>5.1}", v.last_present_ms), lat_col, true);
        w.put("/", label, false);
        w.put(&format!("{:>5.1}", v.max_present_ms), label, false);
        w.put(" ms", label, false);
    } else {
        w.put("  —  ", label, false);
    }

    // slow frames — only when non-zero, hot.
    if v.slow_frames > 0 {
        let hot = w.colors().hot;
        w.sep();
        w.put(&format!("!{} slow", v.slow_frames), hot, true);
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

fn push_ring(ring: &mut VecDeque<f64>, v: f64) {
    ring.push_back(v);
    while ring.len() > PANEL_RING {
        ring.pop_front();
    }
}

/// Human-readable short number: `850`, `12.0k`, `3.4M`, `1.2G`.
fn fmt_short(v: f64) -> String {
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
}

impl SysLoadPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ncpu: f64::from(crate::sysmetrics::ncpu()).max(1.0),
            total_mem: crate::sysmetrics::mem_total(),
            load: VecDeque::with_capacity(PANEL_RING),
            mem: VecDeque::with_capacity(PANEL_RING),
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
    fn poll(&mut self, _now: Instant) {
        if let Some(l) = crate::sysmetrics::load_avg_1m() {
            push_ring(&mut self.load, l);
        }
        if let Some(f) = crate::sysmetrics::mem_used_frac() {
            push_ring(&mut self.mem, f);
        }
    }
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let mut w = RowWriter::new(row, theme);
        let label = w.colors().label;
        // CPU load (1-min), normalized to a per-core fraction for the health color.
        w.put("cpu ", label, false);
        if let Some(&l) = self.load.back() {
            let frac = l / self.ncpu;
            let col = grade_hi(frac as f32, 0.7, 1.0, w.colors());
            w.put(&format!("{l:>4.2}"), col, true);
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
        if let Some(&f) = self.mem.back() {
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
            w.put(&format!("{:>3.0}%", f * 100.0), col, true);
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
    prev: Option<(u64, u64, Instant)>,
    rx: VecDeque<f64>,
    tx: VecDeque<f64>,
}

impl NetworkPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            prev: None,
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
        let Some((rx, tx)) = crate::sysmetrics::net_bytes() else {
            return;
        };
        if let Some((prx, ptx, pt)) = self.prev {
            let dt = now
                .checked_duration_since(pt)
                .map_or(0.0, |d| d.as_secs_f64());
            if dt > 0.0 {
                // Clamp negative (per-interface u32 counter wrap) to 0.
                push_ring(&mut self.rx, rx.saturating_sub(prx) as f64 / dt);
                push_ring(&mut self.tx, tx.saturating_sub(ptx) as f64 / dt);
            }
        }
        self.prev = Some((rx, tx, now));
    }
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let mut w = RowWriter::new(row, theme);
        let label = w.colors().label;
        let good = w.colors().good;
        w.put("net ", label, false);
        if self.rx.is_empty() {
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
}

impl AppFedPanel {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled }
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
    fn paint(&self, row: &mut [RenderCell], theme: Theme) {
        let mut w = RowWriter::new(row, theme);
        let label = w.colors().label;
        let streams = crate::app_fed::snapshot(Instant::now(), 8);
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
        for s in &streams {
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
        let levels = s.spark(4);
        assert!(
            levels.iter().any(|&l| l < 8),
            "auto-scaled sparkline must not pin every slow frame to a full block: {levels:?}"
        );
        assert!(
            levels.contains(&8),
            "the rolling-max frame should reach the top: {levels:?}"
        );
    }

    #[test]
    fn sparkline_levels_map_to_block_glyphs() {
        assert_eq!(spark_glyph(0), ' ');
        assert_eq!(spark_glyph(1), '▁');
        assert_eq!(spark_glyph(8), '█');
        assert_eq!(spark_glyph(9), '█');
    }
}
