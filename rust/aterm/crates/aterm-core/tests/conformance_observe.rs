// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Tier-1 conformance for the Observation Kernel (L0) — bind the abstract
//! `watcher_latch_model` / `idle_deadline_model` (`aterm-spec`) to the REAL
//! engine: drive a genuine [`Terminal`] through [`Terminal::process_at`] with
//! armed watchers and check the real latch decisions, plus a negative control so
//! a pass is never vacuous.
//!
//! The headline property is **IdleFor-under-replay determinism**: feeding the
//! same `(bytes, ClockReading)` schedule and expiring at *different* instants (a
//! prompt live wake vs a lazy replay tick) latches the byte-identical
//! [`Satisfaction`](aterm_core::terminal::Satisfaction) — because the kernel
//! latches at the computed deadline, never the observation instant, and reads no
//! wall clock. This is what lets the kernel coexist with the hydratable temporal
//! buffer without perturbing `conformance_recording`.

use std::time::{Duration, Instant};

use aterm_core::terminal::{ClockReading, HostBindings, Terminal, WatcherSpec};

/// A fixed clock reading at `base + off_ms` — the injected-clock seam that makes
/// replay independent of real wall-clock pacing (mirrors `replay_offset_*`).
fn clock_at(base: Instant, off_ms: u64) -> ClockReading {
    ClockReading {
        monotonic: base + Duration::from_millis(off_ms),
        wall_ms: Some(off_ms),
    }
}

#[test]
fn seq_advanced_latches_on_real_engine_output() {
    let base = Instant::now();
    let mut t = Terminal::new(24, 80);
    let seq0 = t.content_seq();
    let id = t
        .watch(WatcherSpec::SeqAdvanced { after: seq0 }, base)
        .expect("arm");
    assert!(t.watch_poll(id).is_none(), "pending before any output");

    // Real program output advances content_seq through the real pipeline.
    t.process_at(b"hello", clock_at(base, 10));

    let sat = t
        .watch_poll(id)
        .expect("real output advanced content_seq -> latched at the post_process seam");
    assert!(
        sat.seq > seq0,
        "latched seq reflects the real content advance"
    );
}

#[test]
fn negative_control_non_content_batch_does_not_latch() {
    // A batch that produces NO content mutation must NOT latch a SeqAdvanced
    // watcher — proving the kernel is bound to the real `content_seq` clock, not
    // merely to "a process_at happened". (Vacuity guard.)
    let base = Instant::now();
    let mut t = Terminal::new(24, 80);
    let id = t
        .watch(
            WatcherSpec::SeqAdvanced {
                after: t.content_seq(),
            },
            base,
        )
        .expect("arm");
    // A bare cursor-position query (DSR) emits a reply but paints no cells.
    t.process_at(b"\x1b[6n", clock_at(base, 10));
    assert!(
        t.watch_poll(id).is_none(),
        "a non-content batch must not latch a content watcher"
    );
}

#[test]
fn idle_latches_identically_live_vs_replay_on_the_real_engine() {
    // THE determinism property, end-to-end through the real engine: same
    // (bytes, clock) schedule, two different expire instants -> identical latch.
    let base = Instant::now();
    let schedule: &[(&[u8], u64)] = &[(b"a", 10), (b"b", 20), (b"cc", 35)];
    let dur = Duration::from_millis(250);

    let run = |expire_off_ms: u64| {
        let mut t = Terminal::new(24, 80);
        let id = t.watch(WatcherSpec::IdleFor { dur }, base).expect("arm");
        for (bytes, off) in schedule {
            t.process_at(bytes, clock_at(base, *off));
        }
        t.watch_expire(base + Duration::from_millis(expire_off_ms));
        t.watch_poll(id)
    };

    // Live: host wakes just after the deadline (last activity 35ms + 250ms + 1).
    let live = run(35 + 250 + 1);
    // Replay: a single lazy tick far in the "future" of the recorded schedule.
    let replay = run(100_000);

    assert_eq!(
        live, replay,
        "live and replay must latch the byte-identical Satisfaction"
    );
    assert_eq!(
        live.expect("latched").at,
        base + Duration::from_millis(35) + dur,
        "latched instant is the exact deadline (last activity + dur), not the wake"
    );
}

#[test]
fn idle_does_not_fire_before_the_deadline() {
    // Negative control for IdleFor: still-streaming output keeps pushing the
    // deadline out, so an expire mid-stream must NOT latch.
    let base = Instant::now();
    let dur = Duration::from_millis(100);
    let mut t = Terminal::new(24, 80);
    let id = t.watch(WatcherSpec::IdleFor { dur }, base).expect("arm");
    t.process_at(b"streaming", clock_at(base, 50));
    // Only 70ms since the last activity at 50ms (< 100ms): not idle yet.
    assert!(!t.watch_expire(base + Duration::from_millis(120)));
    assert!(
        t.watch_poll(id).is_none(),
        "must not latch before the deadline"
    );
    // After a full quiet window it latches.
    assert!(t.watch_expire(base + Duration::from_millis(150)));
    assert!(t.watch_poll(id).is_some());
}

#[test]
fn watchers_are_excluded_from_checkpoint_hydration() {
    // The replay-safety keystone: a checkpoint carries no watcher state, so a
    // hydrated engine starts with an EMPTY kernel — armed watchers never travel
    // through a keyframe and so cannot perturb replay determinism.
    let base = Instant::now();
    let mut t = Terminal::new(6, 20);
    t.process_at(b"seed\r\n", clock_at(base, 1));
    let _id = t
        .watch(WatcherSpec::SeqAdvanced { after: 0 }, base)
        .expect("arm");
    assert!(t.watchers_armed(), "armed before checkpoint");

    // Hydrate a fresh engine from this one's checkpoint.
    let cp = t.checkpoint();
    let hydrated = Terminal::from_checkpoint(&cp, HostBindings::none());
    assert!(
        !hydrated.watchers_armed(),
        "hydrated engine has an empty kernel — watchers are not checkpointed"
    );
}
