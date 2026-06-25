// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Process-global render/latency counters, surfaced over the control socket as the
//! `metrics` verb so an AI driving aterm can MEASURE the terminal's responsiveness —
//! and DETECT lag — directly, instead of scraping the `$ATERM_TRACE_LATENCY` stderr
//! log or eyeballing it.
//!
//! aterm runs exactly one [`crate::App`] / event loop per process, so plain `static`
//! atomics are sufficient and avoid threading an `Arc` through the control listener:
//! the App (writer, on the present path) and the control thread (reader, in
//! `cmd_metrics`) live in the same process. All ops are `Relaxed` — these are monotone
//! diagnostics, never used for synchronization.
//!
//! ## Detecting lag (not just measuring a moment)
//!
//! A single "last frame" number can't reveal sustained jank, so this also keeps the
//! WORST-CASE and a SLOW-FRAME COUNT since the last [`reset`]:
//! - `frames_presented` — frames actually pushed to the surface since reset (the D-1
//!   early-out returns BEFORE the present, so a steady screen does not inflate this).
//! - `last_/max_present_latency_ns` — the `output→present` delay (PTY-output leading
//!   edge → the frame that showed it; the number `$ATERM_TRACE_LATENCY` logs), most
//!   recent and worst-since-reset.
//! - `last_/max_frame_render_ns` — compose+rasterize+present wall time, most recent
//!   and worst-since-reset.
//! - `slow_frames` — frames whose render time blew the [`SLOW_FRAME_THRESHOLD_NS`]
//!   (~30 fps) budget. A rising count is the lag signature — most often the CPU
//!   rasterizer redrawing heavy colour output (the "GPU was off" trap), so
//!   `backend=cpu` + climbing `slow_frames`/`max_frame_render_ms` is what to watch.
//! - `backend_gpu` — `true` when the live renderer is the GPU (Metal) path.
//!
//! A driver detects lag without OS profilers: `metrics reset`, drive the workload,
//! then `metrics` — if `slow_frames > 0`, or `max_frame_render_ms` is large, or
//! `backend=cpu` under heavy output, the terminal is lagging.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static FRAMES_PRESENTED: AtomicU64 = AtomicU64::new(0);
static LAST_PRESENT_LATENCY_NS: AtomicU64 = AtomicU64::new(0);
static LAST_FRAME_RENDER_NS: AtomicU64 = AtomicU64::new(0);
static MAX_PRESENT_LATENCY_NS: AtomicU64 = AtomicU64::new(0);
static MAX_FRAME_RENDER_NS: AtomicU64 = AtomicU64::new(0);
static SLOW_FRAMES: AtomicU64 = AtomicU64::new(0);
static BACKEND_GPU: AtomicBool = AtomicBool::new(false);

/// A frame whose compose+present wall time exceeds this MISSED a ~30 fps (33.3 ms)
/// budget — the floor below which interaction visibly stutters. `slow_frames` counts
/// these so a driver can DETECT sustained lag rather than read a momentary value.
pub const SLOW_FRAME_THRESHOLD_NS: u64 = 33_333_333; // 1/30 s

/// Record one presented frame. `latency_ns` is the `output→present` delay for this
/// frame, or `0` when no output burst was pending (a blink/selection/resize repaint)
/// — a `0` leaves the last real measurement in place and is NOT a slow-frame input.
/// `render_ns` is this frame's compose+present wall time.
pub fn record_present(latency_ns: u64, render_ns: u64) {
    FRAMES_PRESENTED.fetch_add(1, Ordering::Relaxed);
    if latency_ns != 0 {
        LAST_PRESENT_LATENCY_NS.store(latency_ns, Ordering::Relaxed);
        MAX_PRESENT_LATENCY_NS.fetch_max(latency_ns, Ordering::Relaxed);
    }
    LAST_FRAME_RENDER_NS.store(render_ns, Ordering::Relaxed);
    MAX_FRAME_RENDER_NS.fetch_max(render_ns, Ordering::Relaxed);
    if render_ns > SLOW_FRAME_THRESHOLD_NS {
        SLOW_FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record which renderer is live (called once at startup and on any backend swap).
pub fn set_backend_gpu(on: bool) {
    BACKEND_GPU.store(on, Ordering::Relaxed);
}

/// Zero the measurement-window stats (frame count, maxima, slow count) so a driver
/// can time a SPECIFIC operation: `metrics reset`, run the workload, then `metrics`.
/// Keeps `backend` and the momentary `last_*` readings. Resetting also clears the
/// cold-start present spike from the maxima, so the worst-case reflects steady state.
pub fn reset() {
    FRAMES_PRESENTED.store(0, Ordering::Relaxed);
    MAX_PRESENT_LATENCY_NS.store(0, Ordering::Relaxed);
    MAX_FRAME_RENDER_NS.store(0, Ordering::Relaxed);
    SLOW_FRAMES.store(0, Ordering::Relaxed);
}

/// A consistent-enough read of the counters for the `metrics` control verb.
#[derive(Clone, Copy)]
pub struct Snapshot {
    pub frames_presented: u64,
    pub last_present_latency_ns: u64,
    pub last_frame_render_ns: u64,
    pub max_present_latency_ns: u64,
    pub max_frame_render_ns: u64,
    pub slow_frames: u64,
    pub backend_gpu: bool,
}

/// Read the current counters (lock-free).
#[must_use]
pub fn snapshot() -> Snapshot {
    Snapshot {
        frames_presented: FRAMES_PRESENTED.load(Ordering::Relaxed),
        last_present_latency_ns: LAST_PRESENT_LATENCY_NS.load(Ordering::Relaxed),
        last_frame_render_ns: LAST_FRAME_RENDER_NS.load(Ordering::Relaxed),
        max_present_latency_ns: MAX_PRESENT_LATENCY_NS.load(Ordering::Relaxed),
        max_frame_render_ns: MAX_FRAME_RENDER_NS.load(Ordering::Relaxed),
        slow_frames: SLOW_FRAMES.load(Ordering::Relaxed),
        backend_gpu: BACKEND_GPU.load(Ordering::Relaxed),
    }
}
