# Federation Closure — Program Ledger (COMPLETE)

Status: **COMPLETE — all residuals (1–5) closed.**
Landed to `main` via `--no-ff` merge of `w6b-federation` (tip `3799a5c883`).
Engine: aterm submodule pinned `944c2608dc12` (+ `config/patches/aterm-wasm-source-fixes.patch`, cosmetic web_time/version only).
Final gate: **GATE: PASS** (Codex adversarial FINAL gate, read-only, per-residual PASS with file:line evidence + direct real-WASM probes for residuals 2/4/5).

The federated-terminal-search program is closed. The aterm engine exposes the real
fed E-1 exports (`search_summary` / `row_range_json` / `search_index_release`,
memmem E9b), the remote federated source enumerates real connected panes, and the
client-side replay geometry is frozen against the post-drain engine buffer.

## Verification (all green, at land)

| Check | Result |
|-------|--------|
| `pnpm typecheck` (node / cli / web) | PASS (mobile skipped — `mobile/node_modules` absent in worktree; renderer-only change) |
| `pnpm lint` (oxlint + all gate scripts) | PASS (exit 0) |
| `check:aterm-pin` | PASS — 8 committed wasm artifacts match aterm `944c2608` + patch |
| `check:wasm-pins` | PASS — crypto + git wasm match pins |
| vitest federated-search + pane-manager + remote-runtime-pty-transport | PASS — 1236 passed / 1 skipped (131 files) |
| vitest components/terminal-pane + main terminal-search | PASS — 2517 passed (196 files) |
| Ordering-test mutation proof | Reverting the geometry seam to a synchronous capture turns both scenarios red (independently confirmed) |

## Residual status — all CLOSED (PASS)

### Residual 1 — real-engine-tested export consumers — **CLOSED (PASS)**
- The fed E-1 exports (`search_summary` / `row_range_json` / `search_index_release`)
  are consumed against a live `AtermTerminal`, not a mock
  (`src/renderer/src/lib/pane-manager/aterm/federated-fed-exports-real-engine.test.ts`).

### Residual 2 — search_summary reads the RETAINED just-built index — **CLOSED (PASS)**
- `rust/aterm/crates/aterm-core/src/terminal/search_budgeted.rs:365` — the completed
  budgeted state is RETAINED (`ScanPhase::Complete`), no longer `.take()`-discarded;
  `search_summary_results` (`:434`) reads it directly.
- `search_summary_reads_completed_index_with_zero_rebuild` (`:836`) asserts the rebuild
  counter does NOT advance on the retained read, with a content-change negative control
  that DOES rebuild (`:868`). Carried by aterm pin `944c2608`, baked into the committed wasm.

### Residual 3 — discoverRemoteFederatedPanes returns real panes + post-drain replay geometry — **CLOSED (PASS)**
- **Real enumeration.** `src/renderer/src/lib/federated-search/remote-pane-discovery.ts:42`
  reads `listRemoteFederatedPaneBindings()` from the transport-populated registry
  (`remote-federated-pane-registry.ts`), joins live store tab identity, and resolves each
  binding's env / handle / session / replay geometry via LIVE getters. The remote-runtime
  PTY transport self-registers a binding at construction and unregisters on destroy
  (`remote-runtime-pty-transport.ts`). Not `[]`, not a self-asserting mock.
- **Ordering bug fixed (fed §2.4).** `onSnapshot` previously froze the client replay
  geometry SYNCHRONOUSLY right after `processData`, but production applies the snapshot
  replay ASYNCHRONOUSLY off the pane's replay-write queue
  (`replayDataCallback → scheduleReplayDataDrain → writeReplayDataAsync`). The synchronous
  read captured the PRE-replay buffer — under-counting on fresh attach (an in-window
  replayed-history match wrongly demoted to inline-only) and over-counting on reconnect
  against a deep prior buffer (a post-window host row accepted as in-window → wrong-row jump).
  Fix (`3799a5c883`): a new `awaitReplayApplied` transport option, supplied by pty-connection
  as `replayWriteQueue.then(() => waitForTerminalOutputParsed(...))`, defers the geometry
  freeze until the real drain settles; a monotonic freeze-seq keeps only the latest snapshot's
  post-drain freeze. The synchronous capture remains only as a no-seam fallback.
- **Mutation-proof tests.** `remote-runtime-pty-transport.test.ts` "federated replay geometry
  (fed §2.4)" drives the REAL async path (async buffer mutation via `onReplayData`;
  `awaitReplayApplied` resolves after; `readClientReplayGeometry` reads the live buffer),
  covering both modes: (a) fresh attach — in-window history match not demoted (`:3047`);
  (b) reconnect vs a deep prior buffer — post-window row stays out-of-window while an
  in-window row lands on the correct client row (`:3083`). Reverting the seam to the
  synchronous capture turns both red (24 / 1024 rows vs the required 64) — independently confirmed.

### Residual 4 — (d3) remote-row remap test is real + mutation-proof — **CLOSED (PASS)**
- `src/renderer/src/lib/pane-manager/aterm/remote-remap-real-reflow.test.ts` — client rows come
  from a live `AtermTerminal` read back via `search_summary` (never hand-computed); a non-zero
  replay origin makes the offset load-bearing; a real width-change reflow (`resize` + reflow)
  moves physical rows and the remap must flag `approximate`. A ±1 slip fails.

### Residual 5 — 5E federated benchmark with a real reuse floor — **CLOSED (PASS)**
- `src/renderer/src/lib/federated-search/federated-search-fanout-latency.bench.test.ts:171` —
  floor `warm ≤ cold × 0.7` PROVES reuse (reuse structurally deletes the from-scratch index
  build over deep scrollback; a rebuild-every-time regression collapses to ratio ~1.0 and trips
  the bound), plus a 2 s catastrophic ceiling. Direct execution: cold median ~27 ms, warm
  median ~4.7 ms, ratio ~0.17 — comfortable margin. Runs unconditionally in CI.

## Gate history
- Regate #0 (pre-fix): `GATE: FAIL — 3,5`
- Regate #1 (post residual-5 fix `6d036909ea`): `GATE: FAIL — 3`
- Regate #2 (post residual-3 enumeration `4c150967f8`): `GATE: FAIL — 3` (ordering bug found)
- FINAL (post ordering fix `3799a5c883`): **`GATE: PASS`**

## Disposition
- `landed = true`. `w6b-federation` merged to `main` via `--no-ff`; aterm pinned `944c2608`
  (residual 2), `check:aterm-pin` green. All residuals 1–5 are genuinely closed. Program COMPLETE.
