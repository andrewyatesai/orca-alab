<!-- SPDX-License-Identifier: Apache-2.0 -->

# Cold start — measured state, closed doors, and what is left (2026-08-01)

Written to be picked up cold. Companion: `cold-start-serialization-review.md`
(external design review of the three remaining leads).

**Status (2026-08-02):** items 1 and 2 below have LANDED — see
`cold-start-roadmap.md` for the current ranking. This file is kept as the dated
measurement record and the closed-doors list; do not re-derive either.
(It was deleted in error by `a82dc9b20e`, a publish-guard fix whose stated docs
remedy was to convert links to plain text, not to remove documents — and `docs/`
is excluded from every public export anyway, so its removal bought nothing while
leaving the roadmap citing a file that was not in the tree.)

## Measured state

Committed trend (`tools/benchmarks/trends/darwin-arm64.json`), captured on a
**quiet machine** — no builds, no other sessions:

| | 2026-07-05 seed | 2026-08-01 |
|---|---|---|
| spawn → did-finish-load | 469.9ms | **377ms** |
| spawn → workspace-ready | 567.3ms | **498ms** |

Time to a *usable terminal* is ~900ms–1s. Attribution of the last stretch
(restored 8-tab session, medians):

| stage | ms |
|---|---|
| aterm warm-start → wasm-ready | 160–183 |
| pane boot-start → layout replayed | 4 |
| layout → scrollback restored | 0 |
| scrollback → boot settled | 2 |
| boot-settled → fit measured | 29 |
| PTY connect-start → PTY bound | 80 |
| PTY bound → first frame | 136–187 |

**The remaining cost is WAITING, not computation.** Three independent findings
agree: a cold compile of the 3.86MB engine blob is far cheaper than its phase
implies; the one landed fix removed a false dependency rather than making
anything faster; and no cache or size reduction touches the number.

## Closed doors — do not re-open without new evidence

Each cost real investigation. Each is refuted by measurement, not opinion.

1. **Fork Electron.** Wrong pool. We are ~190–270ms spawn→app-ready against a
   ~127ms trivial-Electron floor, so the fork's entire startup prize is ~60–90ms
   while ~600ms of our own work sits in front of it. Revisit only after the
   app's own path is exhausted.
2. **wasm threads.** `new ImageData(sharedView)` throws in Chromium *with and
   without* `crossOriginIsolated`, and both present paths depend on it. The
   workaround measures +0.13ms @1280×800 → +0.44ms @2560×1440 **per frame**,
   roughly doubling a 0.137ms typing present, paid on every platform forever, to
   accelerate one bursty stage. Plus 2.06 MiB per thread against memory that
   never shrinks. Cross-origin isolation therefore has no payer and stays OFF.
3. **IndexedDB compiled-module cache.** Chromium refuses outright:
   `DataCloneError: A WebAssembly.Module can not be serialized for storage`
   (verified in bare Electron). A full implementation was written and reverted.
4. **Build-flag wasm size reduction.** The blobs are already stripped (no name
   section; 78.8% code / 20.8% data on the 3.86MB CPU blob) and the web crates
   are already `opt-level="z"`. Post-pass knobs are exhausted; shrinking needs
   engine work.

## What is left, in order

1. **Fix the measurement first.** — **LANDED (`afc9424ad2`).**
   `markTerminalPaneBootPhase` latched to whichever pane reached `boot-start`
   first, while `first-terminal-frame` is decided by whichever pane actually
   paints. On a multi-tab restore those are different panes, so every derived
   phase between them subtracted two different objects. It read coherently only
   because the fixture makes `tabs[0]` both first-mounted and active.
   **Two prior attempts failed here.** One resolved the lane from a bare boolean
   with no pane identity (so it could claim the wrong candidate); the other could
   leave the lane permanently unclaimed while the run still reported a first
   frame. Instrumentation that silently describes the wrong object is worse than
   none.
   **The fix is NOT "latch to the visible pane"** — that was this document's own
   original guess and it is unsound. A hidden pane CAN present: the context-loss
   rebuild wires a fresh scheduler that starts unsuspended, an Activity-portal
   slot is unsuspended but invisible, and `isVisible` defaults to true. Any
   design that predicts the pane inherits both failure modes.
   What landed instead: phases are buffered per lane and flushed, with their
   original timestamps, once the first presented frame NAMES the pane that owns
   the timeline. The lane is an outcome, not a prediction, so it cannot be left
   unclaimed while a frame is reported. A second, separate leak was fixed on the
   way: the two `pty-connection.ts` sites passed no pane id at all and bypassed
   the latch entirely.
2. **Then trace the queue.** — **LANDED (`0560e12f20`).** For the pane that
   presented: how many builds were admitted before it, and how long it waited.
   `MAX_CONCURRENT_PANE_BUILDS = 2`, FIFO, and the first two slots are granted
   **synchronously**. `pnpm bench:first-terminal` now prints a `[bench] queue`
   table; `--active-tab-index` moves the active tab off `tabs[0]`, which is both
   the empirical proof of item 1 and the only way to see a non-trivial queue
   position. **Run on 2026-08-02 — see item 3.**
3. **Then decide** whether visible-first admission is worth doing. — **DECIDED
   2026-08-02: IMPLEMENT.** Measured with 8 restored tabs: with the active tab
   LAST, the pane that presented was admitted last (admitIndex 7) after waiting
   541ms for a slot, and time-to-first-terminal-frame was 1259ms vs 1019ms with
   the active tab first. ~240ms is the honest end-to-end figure (541ms is the
   ceiling — the winner's own build is cheaper once the engine is warm). All 8
   panes enqueue in ONE React commit, so the *initial* grant is what must be
   reordered, not just releases. Measure total background-completion alongside,
   so a foreground win cannot hide a restore regression. See
   `cold-start-roadmap.md` for the table.
4. **Remaining lead:** start the worker's own module compile at worker boot
   instead of at first pane init. Must not read settings before they hydrate and
   must not add work to terminal-less launches.

## Method — earned the hard way

- **Startup numbers swing ~30% with machine load.** The same commit read 499ms
  idle and 657–693ms while builds ran. This produced a false "upstream v1.4.161
  cost 250ms" claim that vanished under controlled comparison. Only interleaved
  A/B (A,B,A,B) in one quiet window is valid; treat <5% as noise.
- **Code reading found every real bug; performance prediction failed every
  time.** Reading located the `__dirname` outage, the font/compile coupling, the
  CPU-glue regression and the stale Rust flag. Every unmeasured speed claim made
  this session was wrong.
- **Removing an `await` can drop a side effect.** Dropping `await loadAterm()`
  from the font path also dropped the only call REGISTERING the main-thread CPU
  glue, so every keystroke threw. Nothing caught it: the suites mock the
  registry and no test typed. `tests/e2e/aterm-worker-path-typing.spec.ts` now
  gates it, and it is proven non-vacuous.
- **A gate that refuses you is working.** `bench:check` rejected a partial
  perf-proof run that would have silently dropped seven latency/GPU/memory
  metrics.

## Known-good invocations

```sh
# Engine criterion benches. Needs an IDLE machine and the atpkg Trust toolchain.
# --manifest-path from the repo ROOT dodges rust/.cargo's offline vendor set,
# which lacks proptest. (Updated 2026-09-28: the original recipe forced
# RUSTFLAGS="-Ztrust-verify=off" through rustup's `trust` cargo; the unverified
# lane is now named on the command line instead.)
CARGO_NET_OFFLINE=false \
  targo --unverified bench --manifest-path rust/aterm/Cargo.toml -p aterm-bench --bench engine_throughput

pnpm bench:perf -- --engine-log /tmp/engine.txt --engine-log /tmp/comparative.txt
pnpm bench:check            # gate; -- --accept appends to the trend
pnpm bench:first-terminal   # the restored-session time-to-terminal lane
```

Worktrees need three gitignored artifacts symlinked from the main checkout or
suites fail with misleading symptoms: `node_modules`,
`native/orca-node/orca_node.node`, and `rust/target/release/orca-daemon` (a
missing daemon shows as an error toast that intercepts every e2e click). The
packaged build additionally needs `mobile/node_modules` and one run of
`mobile/scripts/build-terminal-webview-engine.mjs`.
