// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! Tier-0 for DERIVED specs: the TLA+ generated from a Rust `Model` (one source)
//! is exhaustively model-checked by the real `ty` binary.
//!
//! This is the derivation half of `docs/RFC-ty-embed-derived-tla.md`: no
//! hand-written `.tla` — `Model::to_tla()` produces the module and `.to_cfg()` the
//! config, and `ty check` proves the invariants hold over the whole bounded state
//! space. Change the model, the spec changes, and this re-checks the new spec.
//! Drift is impossible by construction. Both the single-action ring AND the
//! two-action cursor (which exercises `UNCHANGED` + a disjunctive `Next`) are
//! checked, so the derivation is shown to generalize.
//!
//! VERIFICATION GATE (honesty ratchet, batteries-on, see [`aterm_spec::verify`]): the
//! `ty` binary is discovered by a fixed canonical path search. Verification is always
//! required — an absent Trust `ty` FAILS the test with a build hint; build the
//! toolchain once (`cargo build --release -p tla-cli` in ~/trust/first-party/ty).

// The 7 introspection models are iterated via `harness::instances()`, not named here.
use aterm_spec::derive::{
    Model, active_handle_model, channel_bind_model, coalesce_model, cursor_model, evict_full_model,
    idle_deadline_model, inject_floor_model, kernel_model, pane_tree_model,
    presentation_gate_model, proxy_forward_model, read_image_seq_model, recording_model,
    ring_model, self_governor_model, session_pool_model, snapshot_model, spawn_locale_model,
    subscribe_model, tab_nav_model, tab_strip_model, tier_residency_model, transact_model,
    watcher_latch_model, window_routing_model,
};
use aterm_spec::verify::ty;
use std::path::PathBuf;
use std::process::Command;

/// Generate the model's spec + cfg and assert `ty check` succeeds.
fn assert_model_checks(ty: &PathBuf, m: &Model) {
    let dir = std::env::temp_dir().join(format!("aterm-derive-{}-{}", m.name, std::process::id()));
    std::fs::create_dir_all(&dir).expect("mk tempdir");
    let spec = dir.join(format!("{}.tla", m.name));
    let cfg = dir.join(format!("{}.cfg", m.name));
    std::fs::write(&spec, m.to_tla()).expect("write derived spec");
    std::fs::write(&cfg, m.to_cfg()).expect("write derived cfg");

    let out = Command::new(ty)
        .arg("check")
        .arg(&spec)
        .arg("--config")
        .arg(&cfg)
        .output()
        .expect("run ty check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "ty check FAILED on DERIVED {} spec\n--- generated {}.tla ---\n{}\n--- ty output ---\n{combined}",
        m.name,
        m.name,
        m.to_tla()
    );
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("derived {} spec model-checked clean by ty.", m.name);
}

#[test]
fn derived_ring_spec_model_checks() {
    let ty = ty("derived ring spec");
    assert_model_checks(&ty, &ring_model());
}

#[test]
fn derived_cursor_spec_model_checks() {
    // Exercises the multi-action / UNCHANGED generation path through `ty`.
    let ty = ty("derived cursor spec");
    assert_model_checks(&ty, &cursor_model());
}

#[test]
fn derived_evict_full_spec_model_checks() {
    // The FUNCTION-VALUED faithful ring: proves EvictOldestContiguous over a
    // live: [1..MaxSeq -> BOOLEAN] set — the property the scalar ring can't express.
    let ty = ty("derived EvictFull spec");
    assert_model_checks(&ty, &evict_full_model());
}

/// A model using the `Buggy` convention: `ty` must PROVE its invariant at the
/// committed `Buggy=0`, and find a COUNTEREXAMPLE at `Buggy=1` — so the invariant
/// is non-trivial AND genuinely catches the bug. Both spec + cfg are derived.
fn assert_proves_and_catches(ty: &PathBuf, m: &Model) {
    let dir = std::env::temp_dir().join(format!("aterm-{}-{}", m.name, std::process::id()));
    std::fs::create_dir_all(&dir).expect("mk tempdir");
    let spec = dir.join(format!("{}.tla", m.name));
    std::fs::write(&spec, m.to_tla()).expect("write spec");

    let run = |cfg_name: &str, cfg: String| -> (bool, String) {
        let cfgp = dir.join(cfg_name);
        std::fs::write(&cfgp, cfg).expect("write cfg");
        let out = Command::new(ty)
            .arg("check")
            .arg(&spec)
            .arg("--config")
            .arg(&cfgp)
            .output()
            .expect("run ty check");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    };

    let (ok, out) = run("ok.cfg", m.to_cfg());
    assert!(
        ok,
        "derived {} (Buggy=0) must model-check clean\n{out}",
        m.name
    );
    let (bug_ok, bug_out) = run("bug.cfg", m.to_cfg_with(&[("Buggy", 1)]));
    assert!(
        !bug_ok,
        "{} (Buggy=1) MUST yield a counterexample\n{bug_out}",
        m.name
    );

    let _ = std::fs::remove_dir_all(&dir);
    eprintln!(
        "derived {}: invariant proven (Buggy=0) and caught (Buggy=1 -> counterexample).",
        m.name
    );
}

#[test]
fn derived_subscribe_proves_and_catches_silent_loss() {
    let ty = ty("derived subscribe spec");
    assert_proves_and_catches(&ty, &subscribe_model());
}

/// Observation Kernel (RFC "The Reactive Surface", L0): the no-silent-loss latch
/// — a transiently-true surface predicate must be caught at the `post_process`
/// seam, never lost to a coalescing consumer wake. PROVES at Buggy=0, CATCHES the
/// deferred-to-wake coalescing bug at Buggy=1. Bound to the real engine by
/// `aterm-core/tests/conformance_observe.rs`.
#[test]
fn derived_watcher_latch_proves_and_catches_silent_loss() {
    let ty = ty("derived watcher-latch spec");
    assert_proves_and_catches(&ty, &watcher_latch_model());
}

/// Observation Kernel (RFC L0): the single armed idle deadline must equal the
/// minimum of all pending `IdleFor` deadlines, so an earlier wake is never
/// missed. PROVES `armed = min` at Buggy=0, CATCHES the keep-first bug at
/// Buggy=1. Bound to the real engine by `WatcherSet::next_deadline`.
#[test]
fn derived_idle_deadline_proves_and_catches_missed_earliest() {
    let ty = ty("derived idle-deadline spec");
    assert_proves_and_catches(&ty, &idle_deadline_model());
}

/// Self-reflection feedback governor (RFC R4 / L2): once the breaker trips, no
/// self-write survives — the storm backstop. PROVES FailClosed at Buggy=0,
/// CATCHES the breaker-bypass at Buggy=1. Bound to `aterm-agent::SelfGovernor`
/// (whose `allow_self_write` returns false once `tripped`).
#[test]
fn derived_self_governor_proves_and_catches_breaker_bypass() {
    let ty = ty("derived self-governor spec");
    assert_proves_and_catches(&ty, &self_governor_model());
}

/// Self-feed floor (RFC D3): the un-bypassable control-layer backstop never
/// admits a self-injection past an empty token bucket. PROVES NoOverdraft at
/// Buggy=0, CATCHES the overdraft at Buggy=1. Bound to `aterm-gui::inject_floor`.
#[test]
fn derived_inject_floor_proves_and_catches_overdraft() {
    let ty = ty("derived inject-floor spec");
    assert_proves_and_catches(&ty, &inject_floor_model());
}

/// Network capability (RFC D4 / L3): an edge token captured on one connection must
/// not authorize on another. PROVES NoReplay at Buggy=0, CATCHES the
/// channel-unbound bug at Buggy=1. Bound to `aterm-net::channel_bind`/`verify_presented`.
#[test]
fn derived_channel_bind_proves_and_catches_replay() {
    let ty = ty("derived channel-bind spec");
    assert_proves_and_catches(&ty, &channel_bind_model());
}

#[test]
fn derived_presentation_gate_proves_and_catches_text_colored_as_emoji() {
    // The ⏺ (U+23FA) fix, model-checked by the real `ty` over the whole bounded
    // state space: a default-TEXT code point is never resolved to the colour face
    // (Buggy=0 PROVES NoColorForText), and the old coverage-only gate is genuinely
    // caught (Buggy=1 -> counterexample).
    let ty = ty("derived presentation-gate spec");
    assert_proves_and_catches(&ty, &presentation_gate_model());
}

#[test]
fn derived_transact_proves_and_catches_lost_update() {
    let ty = ty("derived transact spec");
    assert_proves_and_catches(&ty, &transact_model());
}

#[test]
fn derived_kernel_proves_and_catches_gap() {
    let ty = ty("derived kernel spec");
    assert_proves_and_catches(&ty, &kernel_model());
}

#[test]
fn derived_snapshot_proves_and_catches_leak() {
    let ty = ty("derived snapshot spec");
    assert_proves_and_catches(&ty, &snapshot_model());
}

/// SPAWN LOCALE: `ty` proves the child always ends up with a UTF-8 `LC_CTYPE`
/// (`ChildHasUtf8Ctype`, Buggy=0) and catches the shipped all-unset guard that left a
/// present-but-non-UTF-8 inherited locale unfixed (Buggy=1 → counterexample) — the
/// formal twin of the emacs box-drawing-`?` fix in `aterm_pty::resolve_spawn_locale`.
#[test]
fn derived_spawn_locale_proves_and_catches_non_utf8_child() {
    let ty = ty("derived spawn-locale spec");
    assert_proves_and_catches(&ty, &spawn_locale_model());
}

/// COALESCE: `ty` proves the bulk and single-char write lanes never diverge over
/// the same event stream (the screen is a pure function of the byte log), and
/// catches the bulk-lane skipped-fixup regression (the wide-char-wrap-tail and
/// ZWJ-join class fixed in aterm-grid/aterm-core). This is the model the engine
/// lacked when those two bugs shipped.
#[test]
fn derived_coalesce_proves_and_catches_lane_divergence() {
    let ty = ty("derived coalesce spec");
    assert_proves_and_catches(&ty, &coalesce_model());
}

// --- Property-combinator suite (the introspection control-plane models) ---
//
// The introspection models (M1 dispatch, M2 relay, S1 registry, the forward-handshake
// liveness twin, and the F1 info-flow / ordering / reply-fidelity class models) are
// now `derive::props` combinator INSTANCES, driven by ONE umbrella test over the
// shared instance table. Adding a verified property is a generator instance (~3
// lines) + one row in `harness::instances()` — no new test fn.
#[path = "common/harness.rs"]
mod harness;

/// LIVENESS / deadlock-freedom: `ty` must model-check clean at `Buggy = 0` (the
/// served terminal stutters via the `Done` self-loop) and report a DEADLOCK — not
/// an invariant violation — at `Buggy = 1` (the all-parties-parked wedge). The
/// liveness twin of [`assert_proves_and_catches`], using `CHECK_DEADLOCK TRUE` via
/// `to_cfg_deadlock_with`. This is the mechanism that closes the documented gap:
/// it catches the blocking-call class (the `drain_buffered` `fill_buf` hang) that
/// no reachable-bad-STATE safety invariant can see.
fn assert_deadlock_free_and_catches_wedge(ty: &PathBuf, m: &Model) {
    let dir = std::env::temp_dir().join(format!("aterm-dl-{}-{}", m.name, std::process::id()));
    std::fs::create_dir_all(&dir).expect("mk tempdir");
    let spec = dir.join(format!("{}.tla", m.name));
    std::fs::write(&spec, m.to_tla()).expect("write spec");

    let run = |cfg_name: &str, cfg: String| -> (bool, String) {
        let cfgp = dir.join(cfg_name);
        std::fs::write(&cfgp, cfg).expect("write cfg");
        let out = Command::new(ty)
            .arg("check")
            .arg(&spec)
            .arg("--config")
            .arg(&cfgp)
            .output()
            .expect("run ty check");
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };

    let (ok, out) = run("ok.cfg", m.to_cfg_deadlock_with(&[]));
    assert!(
        ok,
        "derived {} (Buggy=0) must be DEADLOCK-FREE\n{out}",
        m.name
    );
    let (bug_ok, bug_out) = run("bug.cfg", m.to_cfg_deadlock_with(&[("Buggy", 1)]));
    assert!(
        !bug_ok,
        "{} (Buggy=1) MUST deadlock (the wedge)\n{bug_out}",
        m.name
    );
    assert!(
        bug_out.contains("Deadlock"),
        "{} (Buggy=1) failure must be a DEADLOCK, not an invariant violation\n{bug_out}",
        m.name
    );
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!(
        "derived {}: deadlock-free (Buggy=0) and wedge caught (Buggy=1 -> Deadlock).",
        m.name
    );
}

/// THE UMBRELLA: every property-combinator instance PROVES (Buggy=0) + CATCHES
/// (Buggy=1) under the real `ty` checker — a `Safety` invariant via
/// `assert_proves_and_catches`, a `Liveness` instance via
/// `assert_deadlock_free_and_catches_wedge`. The 7 introspection models
/// (dispatch/relay/registry/secrecy/ordering/reply-fidelity + forward-handshake)
/// are iterated from the ONE shared table; a new property adds a row there, not a
/// test fn here.
#[test]
fn property_classes_prove_and_catch_under_ty() {
    let ty = ty("property-combinator suite");
    for inst in harness::instances() {
        match inst.class {
            harness::Class::Safety => assert_proves_and_catches(&ty, &inst.model),
            harness::Class::Liveness { .. } => {
                assert_deadlock_free_and_catches_wedge(&ty, &inst.model);
            }
        }
    }
}

#[test]
fn derived_tier_residency_proves_and_catches_silent_loss() {
    // HIERARCHICAL_SESSIONS.md Addendum B, B.8.2 (GREEN-ORDER step 3): the
    // spill-not-forget property of the hydratable temporal buffer. `ty` PROVES
    // NoSilentLoss at Buggy=0 (every evicted seq stays resident in warm/cold over
    // the whole bounded state space) and CATCHES the silent loss at Buggy=1 (Push
    // drops on evict without spilling) -> counterexample. The proof must hold
    // BEFORE the spill hook ships.
    let ty = ty("derived TierResidency spec");
    assert_proves_and_catches(&ty, &tier_residency_model());
}

#[test]
fn derived_recording_proves_and_catches_dropped_event() {
    // HIERARCHICAL_SESSIONS.md Addendum B, B.8.3 (GREEN-ORDER step 5): the
    // hydration-faithfulness centerpiece — replaying from a keyframe reproduces
    // the live engine state, P(replay@t) = P(live@t), as a parallel-fold
    // refinement (NOT a counter tautology). `ty` PROVES ReplayFaithful at Buggy=0
    // (keyframe-seed + forward replay = the live parity fold over the whole
    // bounded space) and CATCHES the silent drop at Buggy=1 (a ReplayStep skips a
    // payload, so the replay parity diverges from live) -> counterexample. Only
    // authorable after the B.4.2 Clock seam made time an explicit recorded input.
    let ty = ty("derived Recording spec");
    assert_proves_and_catches(&ty, &recording_model());
}

#[test]
fn derived_read_image_seq_proves_and_catches_torn_read() {
    // REARCH A-3: the read_image snapshot-seq protocol — monotone seq,
    // snapshot internal-consistency (no torn read), staleness-detectable.
    // `ty` PROVES NoTornRead + SeqIsStaleOrCurrent at Buggy=0, and CATCHES the
    // torn read at Buggy=1 (a later Write leaks into the active snapshot).
    let ty = ty("derived read_image seq spec");
    assert_proves_and_catches(&ty, &read_image_seq_model());
}

#[test]
fn derived_window_routing_proves_and_catches_missed_exit() {
    // In-process multi-window routing (GUI multi-window work): `ty` PROVES
    // ExitIffEmpty + FrontmostLive + FrontmostAllocated at Buggy=0 (closing the
    // last window exits the app; the frontmost is null iff there are no windows
    // and is never a future/reused id), and CATCHES the missed exit at Buggy=1
    // (the last close fails to exit, leaving win_count=0 with exited=0) ->
    // counterexample on ExitIffEmpty.
    let ty = ty("derived window routing spec");
    assert_proves_and_catches(&ty, &window_routing_model());
}

#[test]
fn derived_tab_nav_proves_and_catches_out_of_range_active() {
    // The GUI per-window tab-strip index machine (`TabIndex` in aterm-gui): `ty`
    // PROVES CountPositive + ActiveInRange at Buggy=0 — a window always keeps >= 1
    // tab and the active index never leaves the renderer's range under ANY
    // interleaving of NewTab / SelectTab / Cycle / Close over the whole bounded
    // (Cap=4) space — and CATCHES the out-of-range active at Buggy=1 (a Close that
    // forgets to re-clamp `active` after the count shrinks, so closing the last
    // active tab leaves `active = count` past the new end) -> counterexample on
    // ActiveInRange. This holds the new tab feature to the same Trust bar as the
    // engine: the renderer never indexes a tab that no longer exists.
    let ty = ty("derived tab navigation spec");
    assert_proves_and_catches(&ty, &tab_nav_model());
}

#[test]
fn derived_pane_tree_proves_and_catches_dangling_focus() {
    // The GUI in-tab split-pane tree (`PaneTree` in aterm-gui): `ty` PROVES
    // TreeNonEmpty + FocusInRange at Buggy=0 — a tab's pane tree always keeps >= 1
    // leaf and the focused leaf index never leaves the renderer's `0..leaf_count-1`
    // range under ANY interleaving of Split (Cmd-D/Cmd-Shift-D) / Close (Cmd-W) over
    // the whole bounded (Cap=4) space — and CATCHES the dangling focus at Buggy=1 (a
    // Close that forgets to re-point `focused` to a surviving sibling after the leaf
    // count shrinks, so closing the focused last leaf leaves `focused = leaf_count`
    // past the new end) -> counterexample on FocusInRange. This holds the split-pane
    // feature to the same Trust bar as tabs: input + the solid cursor never route to
    // a pane that no longer exists, and the tree is never empty while the tab is open.
    let ty = ty("derived pane tree spec");
    assert_proves_and_catches(&ty, &pane_tree_model());
}

#[test]
fn derived_session_pool_proves_and_catches_premature_close() {
    // The GUI session pool refcount accounting (`SessionPool` in aterm-gui): `ty`
    // PROVES ClosedIffEmpty at Buggy=0 — a pooled session's entry is retired exactly
    // when (and only when) its last window viewer detaches, so the Cmd-Shift-O
    // two-windows-one-session path (refcount 2) never retires early and a fully
    // detached session never leaks an entry — and CATCHES the premature retire at
    // Buggy=1 (a Release that retires on EVERY detach, closing while a co-viewer
    // remains) -> counterexample on ClosedIffEmpty.
    let ty = ty("derived session pool spec");
    assert_proves_and_catches(&ty, &session_pool_model());
}

#[test]
fn derived_tab_strip_proves_and_catches_strip_desync() {
    // The native macOS titlebar tab strip (the NSSegmentedControl in aterm-gui's
    // toolbar.rs): `ty` PROVES StripMirrorsTruth at Buggy=0 — the strip's segment
    // count always equals the tab count, its selection always equals the active tab,
    // and the selection stays a valid (in-range) segment index, under ANY interleaving
    // of NewTab / SelectTab / Close over the whole bounded (Cap=4) space — and CATCHES
    // the desync at Buggy=1 (a Close that forgets to re-sync the strip — a missed
    // refresh_window_tabs on a non-front-window close — leaving BOTH seg_count and
    // selected stale, so the strip shows an extra segment with an out-of-range
    // selection) -> counterexample on StripMirrorsTruth. This is the two-lane parity
    // discipline the GUI tab-strip sync must preserve so the native chrome never shows
    // a phantom tab or highlights a segment past the end.
    let ty = ty("derived tab strip spec");
    assert_proves_and_catches(&ty, &tab_strip_model());
}

#[test]
fn derived_active_handle_proves_and_catches_stale_handle() {
    // The GLOBAL control-socket ActiveHandle mirror (`active_handle` in aterm-gui's
    // App): `ty` PROVES HandleMirrorsFront at Buggy=0 — every path that moves the
    // frontmost window's active session ALSO re-points the global control handle (the
    // resync_active_or_window -> sync_active_session discipline), so introspection /
    // drive verbs (text/feed/signal) always target the session the user is looking at,
    // under ANY interleaving of front-active changes over the whole bounded space — and
    // CATCHES the "swallow class" at Buggy=1 (a close-collapse / new-window path that
    // re-mirrors only the per-window state via sync_window and forgets the global
    // re-point) -> counterexample on HandleMirrorsFront. This holds the multi-window
    // control target to the same Trust bar as the per-window tab strip: the one global
    // handle never drives a stale or just-closed session (the bug fixed by routing
    // apply_close_outcome / create_window_internal / push_stub_tab through
    // resync_active_or_window).
    let ty = ty("derived active handle spec");
    assert_proves_and_catches(&ty, &active_handle_model());
}

#[test]
fn derived_proxy_forward_proves_and_catches_forward_cycle() {
    // The cross-process @child proxy forward (control.rs proxy_forward_plan): `ty`
    // PROVES OneHopNoCycle at Buggy=0 — rewriting the child's selector to `@.` caps the
    // forward chain at one cross-process hop, so no A->B->A ping-pong or unbounded
    // relay-thread/fd growth can form (the structural invariant that REPLACED the
    // removed explicit hop-cap) — and CATCHES the loop class at Buggy=1 (a forward that
    // relays the original cross-selector instead of `@.`, so the child re-forwards and
    // the chain grows past one hop) -> counterexample on OneHopNoCycle. If the `@.`
    // rewrite ever regresses, this exhaustive check fails.
    let ty = ty("derived proxy forward spec");
    assert_proves_and_catches(&ty, &proxy_forward_model());
}
