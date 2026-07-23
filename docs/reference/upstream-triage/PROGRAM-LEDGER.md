# Federation Closure — Program Ledger (TRUE final state)

Branch: `w6b-federation` @ `6d036909ea`
Engine: aterm submodule pinned `944c2608dc12` (+ `config/patches/aterm-wasm-source-fixes.patch`, cosmetic web_time/version only)
Final gate: **GATE: FAIL — 3** (Codex `gpt-5.6-sol`, read-only, on the branch diff)

The aterm engine exposes real fed E-1 exports (`search_summary` / `row_range_json`
/ `search_index_release`, memmem E9b) on aterm `origin/main`, verified present in
`src/renderer/src/lib/pane-manager/aterm/aterm_wasm.d.ts`. Residual 1 (real-engine-tested
export consumers) is closed and already on `main`. The FINAL gate was re-run after a
residual-5 fix; the honest outcome is below.

## Verification (all green, in the w6b-federation worktree)

| Check | Result |
|-------|--------|
| `pnpm typecheck` | PASS (exit 0) |
| `pnpm lint` (oxlint + 12 gate scripts) | PASS (exit 0) |
| `check:aterm-pin` | PASS — 8 committed wasm artifacts match aterm `944c2608` + patch |
| `check:wasm-pins` | PASS — crypto + git wasm match pins |
| vitest federated-search + pane-manager/aterm | PASS — 672/672 (82 files) |
| vitest pane-manager + components/terminal-pane | PASS — 3592 passed / 1 skipped (312 files) |

## Residual status (Codex per-residual verdict, verbatim deciding lines)

### Residual 2 — search_summary reads the RETAINED just-built index — **CLOSED (PASS)**
- `rust/aterm/crates/aterm-core/src/terminal/search_budgeted.rs:375` — the completed
  budgeted state is RETAINED (`self.budgeted_search = Some(state)` with `ScanPhase::Complete`),
  no longer `.take()`-discarded; `search_summary_results` (line 443) reads it directly.
- Instrumented test `search_summary_reads_completed_index_with_zero_rebuild` (line 836):
  asserts the rebuild counter does NOT advance on the retained read (line 853) and a
  negative control — content change → fallback DOES rebuild (line 877). Not a dead counter.
- Carried by the aterm pin `944c2608`, hence baked into the committed wasm the TS tests load.

### Residual 3 — discoverRemoteFederatedPanes returns real panes — **OPEN (FAIL)**
- `src/renderer/src/lib/federated-search/remote-pane-discovery.ts:25` — the function body
  returns exactly `[]`. This is a deliberate, documented deferral: remote-pane enumeration
  (resolving each pane's replayed-anchor geometry via `multiplexer.getReplayedHostAnchor()`
  + replay origin/row-count/cols) is wired where the remote pty transport is owned, at pin
  time. The production `terminal.search` CALL side (5B wire, runtime route + SSH mux) IS
  complete and cancellation-aware; only ENUMERATION is inert. Returning `[]` keeps the remote
  adapter registered and never yields a wrong result — but it is not a genuine close.
- **Cannot be closed in this worktree without the remote-transport owner.** Fabricating panes
  would be forcing green; not done.

### Residual 4 — (d3) remote-row remap test is real + mutation-proof — **CLOSED (PASS)**
- `src/renderer/src/lib/pane-manager/aterm/remote-remap-real-reflow.test.ts:126` — replaces the
  plain-offset tautology (which fed the remap the numbers it recomputes). Client rows come from
  a live `AtermTerminal` read back via `search_summary` (never hand-computed); a non-zero replay
  origin makes the offset load-bearing; a real width-change reflow (`resize` + `pump_reflow`)
  moves physical rows and the remap must flag `approximate` (line 178). A ±1 slip fails.

### Residual 5 — 5E federated benchmark with a real floor — **CLOSED (PASS)** (fixed this pass)
- `src/renderer/src/lib/federated-search/federated-search-fanout-latency.bench.test.ts:171`.
- Codex regate #1 first FAILED this: the original floor `warmMs ≤ coldMs × 1.5` permits warm to
  EQUAL cold, so a rebuild-every-time regression (residual 2 undone) passes — it proved nothing.
- Fix (commit `6d036909ea`): floor tightened to `warmMs ≤ coldMs × 0.7`, which PROVES reuse —
  reuse structurally deletes the dominant cost (from-scratch index build over deep scrollback),
  collapsing a regression onto ratio ~1.0 and tripping the bound. Measured ratio ~0.18
  (cold ~29.4 ms, warm ~5.3 ms, ~5.6× speedup; reps clustered ±3% across runs) → ~3.9× margin.
  Correctness-per-pane floor and 2 s catastrophic ceiling unchanged; runs unconditionally in CI.

## Gate history
- Regate #0 (pre-fix): `GATE: FAIL — 3,5`
- Regate #1 (post residual-5 fix `6d036909ea`): `GATE: FAIL — 3`

## Disposition
- `held = false` — a genuine PASS was NOT reached; the branch is blocked by the honest
  residual-3 failure, not by concurrency. Nothing to land.
- `w6b-federation` is verified (typecheck/lint/tests/pins all green) and carries genuine
  closures of residuals 2, 4, 5. It must NOT be merged to `main` until residual 3 is genuinely
  closed by the remote-pty transport owner (real enumeration, not `[]`).
