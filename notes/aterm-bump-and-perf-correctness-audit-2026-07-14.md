# aterm bump + performance/correctness audit — 2026-07-14

## Dependency bump

- **aterm** submodule v0.29-112 (`b0486b97`) → **v0.29-137 (`eea3ef55`)**, 25 commits.
  Low-risk for orc: binding crates (aterm-wasm/gpu-web/ffi/effects-web) unchanged,
  no new third-party deps, `render.rs` delta is GPU fire/effects only (not orc's
  headless path). Includes `fix(scrollback): saturating arithmetic in warm-tier +
  tier-ops readers`, which hardens the exact readers orc's `serialize_scrollback_ansi`
  uses (`get_history_line`/`scrollback_lines`).
- Regenerated the committed wasm glue + napi addon (stable 1.96); `check:aterm-pin` ok.
- **"Rebuild using trust":** a full trust-toolchain cross-build of the SHIPPED artifacts
  is not viable — the Trust stage2 toolchain has no `wasm32-unknown-unknown` std, and
  its deny-mode Level-0 verification hard-fails on dependency build scripts (serde/once_cell)
  plus a solver `BUG(#4666)`. So the build stays on stable and Trust runs in its real
  role: all `ay` proof gates discharge green against the bumped state (orca-git
  cap_invariant / decode_total / line_scan_bounds, orca-net oom_bound). A
  `ORCA_RUST_TOOLCHAIN` override (default stable) was added to the wasm+addon build
  scripts for future trust experiments.

## End-user performance audit — 7 verified findings

Fixed:
- **R4** renderer bundle unminified → enabled esbuild minify (entry 7.0 → 4.2 MB, every launch).
- **R1** daemon stream batcher re-flattened a held flooding session's multi-MB ConsString
  every drain cycle (O(n²)) → cap coalescing at one write-slice, head-indexed chunk list.
- **R2** `attachLineStats` numstat scanned the whole worktree per poll → pathspec-scope to
  the known changed paths (both rename sides; full-scan fallback if the old side is missing).
- **R3** untracked line-counting had no file-count cap → skip counting above
  `MAX_UNTRACKED_LINE_COUNT_FILES` (500), like the huge-repo path.
- **R7** hover drove a full engine `render()` per cell in the worker → render-free STATE
  post, gated on the hover outcome changing.

Deferred (with rationale):
- **R5** aterm wasm not prewarmed — LOW. Deliberately opposes the documented "memory over
  warmth" design (an idle app keeps no resident worker/wasm/fonts). Prewarm would shave only
  the off-main-thread compile, not GL-acquire/build/first-render, and reintroduce exactly the
  resident memory the design rejects. Skip unless first-terminal snappiness becomes a named goal.
- **R6** background browser webviews are never discarded/hibernated, no LRU/cap — MEDIUM (RAM).
  A real architectural gap: parked (display:none) guests release GPU/compositor surfaces but keep
  renderer heap, and `setBackgroundThrottling(false)` is unconditional. The proper fix (LRU-cap
  live guests + discard-with-restore-on-focus, or discard after N minutes of workspace inactivity)
  needs a **product decision on tab-discard/restore UX**, so it is flagged for the owner rather than
  shipped blind. Minimum viable step: stop disabling background throttling for non-foreground guests.

Confirmed already-handled (audit noted, no change): main-thread hover path (already optimized),
daemon flood memory (keep-tail + valve bounded), huge-repo git work (10k limit unsubscribes +
`didHitLimit` skip + 20k stat cache + 125ms debounce), wasm compile (off-main-thread streaming).

## Correctness audit — 6 verified bugs (all fixed)

- **Bug 1** (HIGH) cold restore duplicated the whole normal-screen scrollback — gate the
  `restoreFromIncrementalLog` scrollback prefix on `modes.alternateScreen`.
- **Bug 2** (HIGH) mobile/requested budgeted snapshots doubled recent scrollback —
  `scrollbackRows:0` + `boundScrollbackAnsi` (real byte-bounding restored).
- **Bug 3** (HIGH, default macOS/Linux) Rust daemon `create_or_attach` ignored `historySeed`
  → seed the engine + report `historySeeded` (parity with the Node daemon).
- **Bug 4** (MEDIUM) a throwing engine side-channel orphaned the claimed PTY ACK credit →
  guard the aterm mirror.
- **Bug 5** (MEDIUM→LOW, log-only) stale-dispatch compared space-format columns vs an ISO-`T`
  threshold by raw bytes → `datetime()`-wrap both operands.
- **Bug 6** (LOW) ref-badge tie-break used byte order → case-insensitive `to_lowercase` (still
  cross-host deterministic).

Also fixed: the F4 `user_version_migrations` integration test (a compile break + stale schema
goldens F4 left, hidden because the target no longer compiled).

## Daemon parity gap (surfaced by the audit, FIXED)

- The Rust daemon's `getSnapshot` modes omitted `kittyKeyboardFlags` (present in the Node daemon's
  modes) — the sole divergence failing `pnpm parity:daemon` Leg B (surfaced while fixing Bug 3).
  Closed: aterm already tracks kitty keyboard state, so `HeadlessTerminal::kitty_keyboard_flags()`
  now exposes it and `build_snapshot` emits it. `pnpm parity:daemon` → PASSED.

## Gate results

typecheck ✓ · lint ✓ · full vitest 29,581 passed / 0 failed ✓ · cargo tests (touched crates,
774 + doctests, clean stable pass) ✓ · ay proof gates ✓ · parity:daemon ✓. (The recurring
cargo *doctest* `E0514` is a cross-build-cache artifact — 0 actual doctests — that appears when
crates are rebuilt under mixed toolchains/profiles; the real unit+integration tests all pass.)
</content>
