# Perf Proof Lane

The perf-proof lane is the local gauntlet that backs every headline terminal
performance claim — keystroke-to-paint, GPU vs CPU frame cost, per-pane wasm
memory, startup — with committed trending numbers, and fails a change that
regresses a gated metric by more than **15%**. It exists so later
visual/effects work cannot silently regress smoothness: run it before landing
anything that touches the terminal render path, and gate wave-2 effect work on
a green check.

There is no hosted CI in this fork — this lane is run locally, and its trend
files are committed as the evidence.

## Quick start

```sh
pnpm bench:perf          # capture a full run (builds the app, ~10-15 min)
pnpm bench:check         # gate the run against the committed trend
```

`bench:check` exits non-zero on any >15% regression on a gated metric.
To publish a new baseline (first run on a machine, or an intentional,
explained trade-off), append it to the trend and commit the trend file:

```sh
pnpm bench:check -- --accept
git add tools/benchmarks/trends/
```

## What one run captures

`pnpm bench:perf` (`tools/benchmarks/perf-proof-run.mjs`) drives everything
headlessly and writes one structured run to
`tools/benchmarks/results/perf-proof-<label>-<stamp>.json`:

1. **In-app harnesses** — `tests/e2e/aterm-perf-proof.spec.ts` launches the
   real Electron app (Playwright `electron-headless` project, same pattern as
   the rest of `tests/e2e`) and, in one session:
   - types real keystrokes into a live pane and reads the
     `orca:terminal:keydown-to-frame-presented` User Timing measure stamped by
     the interactive fast path in
     `src/renderer/src/lib/pane-manager/aterm/aterm-pane-present.ts` (marks
     defined in `src/shared/terminal-perf-marks.ts`);
   - runs `window.__atermLatencyBench` (render-half single-cell latency, CPU +
     GPU), `window.__atermGpuCpuBench` (steady-state ms/frame at several grid
     sizes, sparse and full-grid mutation), and `window.__atermMemoryBench`
     (wasm heap per live pane).
2. **Startup** — `tools/benchmarks/startup-time-bench.mjs` (spawn →
   `did-finish-load`, median over 3 iterations against a 2,000-file synthetic
   profile by default).
3. **Engine criterion benches** (optional fold-in) — see the manual step below.

The spec is opt-in (`ORCA_PERF_PROOF=1`, set by the runner), so the default
`pnpm test:e2e` gauntlet's runtime is unchanged. It is a measurement lane with
loose sanity asserts; the regression gate lives entirely in `bench:check`.

## The metric catalog and gate policy

`tools/benchmarks/perf-proof-metrics.mjs` is the single source of truth for
which numbers are trended and gated. Policies:

| policy | meaning |
|---|---|
| `gate` | >15% regression fails; the metric going **missing** while present in the baseline also fails (a dead harness or a lost GPU path is itself a regression) |
| `gate-if-present` | compared only when both runs carry it (the manual engine step) |
| `info` | reported, never gated (known-noisy percentiles) |

Gated today: keydown→frame median, CPU/GPU render-half medians, CPU and GPU
ms/frame at 80x24 sparse, GPU ms/frame at 200x50 full-grid (scaling guard),
wasm KB per pane, and startup spawn→did-finish-load.

Comparison logic is pure and unit-tested:

```sh
node_modules/.bin/vitest run --config tools/benchmarks/vitest.config.ts
```

## Trends

`tools/benchmarks/trends/<platform>-<arch>.json` holds one committed series
per machine (e.g. `darwin-arm64.json`). `bench:check` compares the newest run
against the **last committed entry for the same machine key** — numbers are
never compared across machines. The audit-F42 proof points call for the lane
to be maintained on two named machines: the darwin-arm64 series is seeded; a
Linux x64 series should be seeded with `pnpm bench:perf && pnpm bench:check
-- --accept` on that rig (first accept creates the file).

## Named manual steps (not headless-runnable)

These cannot run inside the Electron/Playwright lane. Run them when the
engine pin changes or when a claim depending on them is being re-proven, and
fold the numbers into the same run so they trend alongside the app metrics.

### 1. aterm engine criterion benches (requires the aterm checkout + cargo)

```sh
cd <aterm repo>   # the source of the pinned rust/aterm submodule
export PATH="$HOME/.cargo/bin:$PATH"
cargo bench -p aterm-bench --bench engine_throughput 2>&1 | tee /tmp/engine.txt
cargo bench -p aterm-bench --bench comparative       2>&1 | tee /tmp/comparative.txt
```

Then fold into a run:

```sh
pnpm bench:perf -- --engine-log /tmp/engine.txt --engine-log /tmp/comparative.txt
```

The parser (`tools/benchmarks/criterion-output-parse.mjs`) reads raw criterion
console output; these metrics are `gate-if-present`, so app-only runs skip
them without failing. The darwin-arm64 seed carries the criterion numbers
captured at engine pin a01f300.

### 2. aterm on-glass bench (`tools/onglass-bench` in the aterm repo)

Native-window glass-to-glass measurement; needs a real display and is not
foldable into the trend JSON yet (no machine-readable output mode — tracked as
the optional `[ws-proof]` aterm improvement). Record results in the PR
description when claiming glass-to-glass numbers.

### 3. Packaged-build startup

The lane measures the dev-built app (`out/`). For release evidence, point the
startup bench at a packaged build:

```sh
node tools/benchmarks/startup-time-bench.mjs --label packaged --exe <path-to-Orca>
```

## Reading the keydown→frame number honestly

- The measure is stamped by the **in-process** interactive fast path
  (`aterm-pane-present.ts`), which the spec pins via
  `window.__atermWorkerRender = false`. The default production path is the
  worker; its `presentNow` (in `aterm-worker-frame-scheduler.ts`) gets the
  production keydown emit as the coordinated wave-2 one-liner. Until then this
  metric tracks the in-process path — the same render half, minus worker
  message hops.
- The marks are local User Timing instrumentation only (visible in DevTools →
  Performance). Nothing is emitted as remote telemetry, and there is no
  on-screen HUD by design.
- Headless GPU numbers reflect whatever GL the headless session negotiates
  (ANGLE Metal on macOS rigs; possibly software GL on headless Linux). The
  trend is per-machine, so like is always compared with like; check
  `latency.glRenderer` in the run file when a GPU number moves suspiciously.
