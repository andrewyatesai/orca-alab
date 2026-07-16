import { mkdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { test, expect } from './helpers/orca-app'
import { focusActiveTerminalInput, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { TERMINAL_PERF_MARKS, TERMINAL_PERF_MEASURES } from '../../src/shared/terminal-perf-marks'

// Perf-proof lane driver (ws-proof). Runs the in-renderer aterm harnesses —
// __atermLatencyBench, __atermGpuCpuBench, __atermMemoryBench — plus a REAL
// typed keydown→frame-presented measurement over the User Timing marks stamped
// by aterm-pane-present.ts, in ONE app session, and writes the raw structured
// numbers to ORCA_PERF_PROOF_OUT for tools/benchmarks/perf-proof-run.mjs.
//
// This is a MEASUREMENT LANE, not a perf gate: asserts here are loose sanity
// checks; the >15% regression gate lives in tools/benchmarks/perf-proof-check.mjs
// against the committed trend JSONs. Opt-in only (`pnpm bench:perf`) so the
// default e2e gauntlet's runtime is unchanged.

type LatencyStats = {
  samples: number
  medianMs: number
  p95Ms: number
  minMs: number
  maxMs: number
  meanMs: number
}
type LatencyBenchResult = {
  glRenderer: string | null
  glVendor: string | null
  gpuAdapterInfo: string | null
  renderHalf: { cpu: LatencyStats; gpu: LatencyStats | null; gpuReason?: string }
  frameTable: {
    cols: number
    rows: number
    atermCpuMsPerFrame: number
    atermGpuMsPerFrame: number | null
  }[]
}
type GpuCpuPathBench = {
  path: 'cpu' | 'gpu'
  mutation: 'sparse' | 'full'
  cols: number
  rows: number
  msPerFrame: number
  fps: number
  firstFrameMs: number
  initMs: number
  submitMsPerFrame: number
}
type GpuCpuModeRow = {
  cols: number
  rows: number
  mutation: 'sparse' | 'full'
  cpu: GpuCpuPathBench
  gpu: GpuCpuPathBench | null
}
type GpuCpuBenchResult = {
  available: boolean
  reason?: string
  rows: { cols: number; rows: number; sparse: GpuCpuModeRow; full: GpuCpuModeRow }[]
  adapterInfo: string | null
  glRenderer: string | null
  glVendor: string | null
  cpuWasmLoadMs: number
  gpuWasmLoadMs: number
}
type MemoryBenchResult = {
  panes: number
  scrollbackLines: number
  cols: number
  rows: number
  bytesPerPane: number
  kbPerPane: number
  totalHeapBytes: number
}
type BenchProbe = {
  __atermLatencyBench?: (
    sizes: [number, number][],
    iterations: number,
    warmup: number,
    frames: number
  ) => Promise<LatencyBenchResult>
  __atermGpuCpuBench?: (sizes: [number, number][], frames: number) => Promise<GpuCpuBenchResult>
  __atermMemoryBench?: (
    cols: number,
    rows: number,
    scrollbackLines: number,
    panes: number
  ) => Promise<MemoryBenchResult>
}

type KeydownSection = {
  attempted: number
  /** Which render path stamped the frame-presented mark (see the pin above). */
  renderPath: 'in-process'
  stats: LatencyStats | null
  reason?: string
}

function computeStats(samples: number[]): LatencyStats {
  const sorted = [...samples].sort((a, b) => a - b)
  const at = (q: number): number =>
    sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]
  return {
    samples: sorted.length,
    medianMs: at(0.5),
    p95Ms: at(0.95),
    minMs: sorted[0],
    maxMs: sorted.at(-1) ?? sorted[0],
    meanMs: sorted.reduce((a, b) => a + b, 0) / sorted.length
  }
}

test.describe('aterm perf-proof lane @perf-proof', () => {
  test.skip(
    process.env.ORCA_PERF_PROOF !== '1',
    'perf-proof lane runs via `pnpm bench:perf` (ORCA_PERF_PROOF=1), not in the default suite'
  )

  test('captures keydown→frame plus latency/GPU/memory harness numbers', async ({
    orcaPage
  }, testInfo) => {
    // Several throwaway engine builds (CPU + GPU at multiple grids) — well past
    // the default per-test budget.
    test.setTimeout(420_000)
    orcaPage.on('pageerror', (err) => {
      // eslint-disable-next-line no-console
      console.log(`[renderer:pageerror] ${err.message}`)
    })

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the GPU opt-in BEFORE the pane so the GPU paths are loadable for the
    // self-contained benches (they build their own engines either way), and pin
    // the live pane to the in-process render path: the ws-proof mark pair lives
    // in aterm-pane-present.ts (the in-process interactive fast path); the worker
    // path's presentNow gets its production emit in wave 2 (coordinated file).
    await orcaPage.evaluate(() => {
      const w = window as unknown as {
        __atermGpuEnabled?: boolean
        __atermWorkerRender?: boolean
      }
      w.__atermGpuEnabled = true
      w.__atermWorkerRender = false
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)

    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            () => typeof (window as unknown as BenchProbe).__atermLatencyBench === 'function'
          ),
        { timeout: 30_000, message: 'aterm bench hooks should be ready' }
      )
      .toBe(true)

    // ---- 1. keydown→frame-presented over the REAL pane (typed echo) ----------
    // The lane stamps the keydown mark from a capture-phase listener — the
    // harness-side stand-in for the production keydown emit site — and reads the
    // measure that aterm-pane-present.ts emits on its interactive fast path.
    const keydownMark = TERMINAL_PERF_MARKS.keydown
    const keydownMeasure = TERMINAL_PERF_MEASURES.keydownToFramePresented
    await focusActiveTerminalInput(orcaPage)
    await orcaPage.evaluate((mark) => {
      window.addEventListener('keydown', () => performance.mark(mark), { capture: true })
    }, keydownMark)
    // Let the prompt settle so early shell output doesn't blur the first samples.
    await orcaPage.waitForTimeout(1_500)

    const KEYSTROKES = 24
    const samples: number[] = []
    for (let i = 0; i < KEYSTROKES; i++) {
      const before = await orcaPage.evaluate(
        (name) => performance.getEntriesByName(name, 'measure').at(-1)?.startTime ?? -1,
        keydownMeasure
      )
      await orcaPage.keyboard.type('abcdefghijklmnop'[i % 16])
      const duration = await orcaPage
        .evaluate(
          async ({ name, before }) => {
            const deadline = Date.now() + 2_000
            while (Date.now() < deadline) {
              const entry = performance.getEntriesByName(name, 'measure').at(-1)
              if (entry && entry.startTime > before) {
                return entry.duration
              }
              await new Promise((r) => setTimeout(r, 5))
            }
            return null
          },
          { name: keydownMeasure, before }
        )
        .catch(() => null)
      if (typeof duration === 'number' && Number.isFinite(duration) && duration >= 0) {
        samples.push(duration)
      }
      await orcaPage.waitForTimeout(50)
    }
    const keydown: KeydownSection =
      samples.length >= 8
        ? { attempted: KEYSTROKES, renderPath: 'in-process', stats: computeStats(samples) }
        : {
            attempted: KEYSTROKES,
            renderPath: 'in-process',
            stats: null,
            reason: `only ${samples.length}/${KEYSTROKES} keystrokes produced a keydown→frame measure (eager presents may have coalesced onto rAF)`
          }

    // ---- 2. render-half latency + per-frame table (throwaway engines) --------
    const latency = (await orcaPage.evaluate(
      ({ sizes, iterations, warmup, frames }) => {
        const fn = (window as unknown as BenchProbe).__atermLatencyBench
        return fn ? fn(sizes as [number, number][], iterations, warmup, frames) : null
      },
      {
        sizes: [
          [80, 24],
          [120, 40]
        ],
        iterations: 40,
        warmup: 10,
        frames: 40
      }
    )) as LatencyBenchResult | null

    // ---- 3. GPU-vs-CPU steady-state frame cost --------------------------------
    const gpuCpu = (await orcaPage.evaluate(
      ({ sizes, frames }) => {
        const fn = (window as unknown as BenchProbe).__atermGpuCpuBench
        return fn ? fn(sizes as [number, number][], frames) : null
      },
      {
        sizes: [
          [80, 24],
          [120, 40],
          [200, 50]
        ],
        frames: 120
      }
    )) as GpuCpuBenchResult | null

    // ---- 4. per-pane wasm memory footprint ------------------------------------
    const memory = (await orcaPage.evaluate(
      ({ cols, rows, scrollback, panes }) => {
        const fn = (window as unknown as BenchProbe).__atermMemoryBench
        return fn ? fn(cols, rows, scrollback, panes) : null
      },
      { cols: 120, rows: 40, scrollback: 1000, panes: 4 }
    )) as MemoryBenchResult | null

    // ---- write the raw run for the lane runner --------------------------------
    const outPath =
      process.env.ORCA_PERF_PROOF_OUT ?? testInfo.outputPath(`perf-proof-raw-${Date.now()}.json`)
    mkdirSync(path.dirname(outPath), { recursive: true })
    const raw = {
      schema: 1,
      capturedAt: new Date().toISOString(),
      keydown,
      latency,
      gpuCpu,
      memory
    }
    writeFileSync(outPath, `${JSON.stringify(raw, null, 2)}\n`)
    // eslint-disable-next-line no-console
    console.log(`[perf-proof] raw run written to ${outPath}`)
    if (keydown.stats) {
      // eslint-disable-next-line no-console
      console.log(
        `[perf-proof] keydown→frame-presented: median ${keydown.stats.medianMs.toFixed(2)}ms ` +
          `p95 ${keydown.stats.p95Ms.toFixed(2)}ms (n=${keydown.stats.samples})`
      )
    } else {
      // eslint-disable-next-line no-console
      console.log(`[perf-proof] keydown→frame-presented UNAVAILABLE: ${keydown.reason}`)
    }

    // Loose sanity asserts only — regression gating happens in bench:check.
    expect(latency, 'latency bench returned a result').not.toBeNull()
    expect(latency!.renderHalf.cpu.samples, 'CPU latency samples').toBeGreaterThan(0)
    expect(latency!.renderHalf.cpu.medianMs, 'CPU render-half median sane').toBeLessThan(250)
    expect(gpuCpu, 'gpu/cpu bench returned a result').not.toBeNull()
    expect(memory, 'memory bench returned a result').not.toBeNull()
    expect(memory!.bytesPerPane, 'per-pane wasm cost positive').toBeGreaterThan(0)
    expect(
      keydown.stats,
      `typed keydown→frame-presented measures should be collectable: ${keydown.reason ?? ''}`
    ).not.toBeNull()
  })
})
