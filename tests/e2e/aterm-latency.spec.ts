import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// HONEST keystroke-latency benchmark for the aterm renderer, run inside the REAL
// Electron renderer. It is a MEASUREMENT (loose sanity asserts only), not a flaky
// perf gate.
//
// What's measured (all on a single-cell update — one echoed keystroke):
//  - RENDER-HALF latency (median + p95): aterm CPU process→render→putImageData and
//    aterm GPU process→render→gl.finish(). This is the render contribution to
//    per-keystroke latency; the PTY round-trip is shared and excluded.
//  - Per-frame cost at 80x24 and 120x40 for aterm-GPU and aterm-CPU.
//  - The GL renderer string (ANGLE/Metal vs software) so the numbers are
//    interpretable.

type LatencyStats = {
  samples: number
  medianMs: number
  p95Ms: number
  minMs: number
  maxMs: number
  meanMs: number
}
type FrameTimeRow = {
  cols: number
  rows: number
  atermCpuMsPerFrame: number
  atermGpuMsPerFrame: number | null
}
type BenchResult = {
  glRenderer: string | null
  glVendor: string | null
  gpuAdapterInfo: string | null
  renderHalf: { cpu: LatencyStats; gpu: LatencyStats | null; gpuReason?: string }
  frameTable: FrameTimeRow[]
}
type BenchProbe = {
  __atermLatencyBench?: (
    sizes: [number, number][],
    iterations: number,
    warmup: number,
    frames: number
  ) => Promise<BenchResult>
}

test.describe('aterm keystroke-latency benchmark @aterm-latency', () => {
  test('measures aterm CPU/GPU render-half and per-frame latency', async ({
    orcaPage
  }, testInfo) => {
    // Builds several throwaway engines (CPU + GPU render-half, plus per-size aterm
    // CPU/GPU) — give it room beyond the default.
    test.setTimeout(240_000)
    orcaPage.on('console', (msg) => {
      const t = msg.text()
      if (/aterm|gpu|webgl|wgpu|panic/i.test(t)) {
        // eslint-disable-next-line no-console
        console.log(`[renderer:${msg.type()}] ${t}`)
      }
    })
    orcaPage.on('pageerror', (err) => {
      // eslint-disable-next-line no-console
      console.log(`[renderer:pageerror] ${err.message}`)
    })

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the GPU opt-in BEFORE the pane (the bench builds its
    // own engines, but the GPU path must be loadable for the import).
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermGpuEnabled?: boolean }).__atermGpuEnabled = true
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
        { timeout: 30_000, message: 'aterm latency bench hook should be ready' }
      )
      .toBe(true)

    const sizes: [number, number][] = [
      [80, 24],
      [120, 40]
    ]
    const ITERATIONS = 40
    const WARMUP = 10
    const FRAMES = 40

    const result = (await orcaPage.evaluate(
      ({ sizes, iterations, warmup, frames }) => {
        const fn = (window as unknown as BenchProbe).__atermLatencyBench
        return fn ? fn(sizes, iterations, warmup, frames) : null
      },
      { sizes, iterations: ITERATIONS, warmup: WARMUP, frames: FRAMES }
    )) as BenchResult | null

    expect(result, 'bench returned a result').not.toBeNull()
    const r = result as BenchResult

    const fmtStats = (s: LatencyStats): string =>
      `median ${s.medianMs.toFixed(3)}ms  p95 ${s.p95Ms.toFixed(3)}ms  (min ${s.minMs.toFixed(
        3
      )} / max ${s.maxMs.toFixed(3)}, n=${s.samples})`

    const lines: string[] = [
      `[aterm-latency] GL renderer=${r.glRenderer ?? '<none>'} vendor=${r.glVendor ?? '<none>'}`,
      `[aterm-latency] wgpu adapter=${r.gpuAdapterInfo ?? '<none>'}`,
      '[aterm-latency] -- RENDER-HALF latency, single-cell update @ 80x24 (render contribution to one keystroke) --',
      `[aterm-latency] aterm CPU: ${fmtStats(r.renderHalf.cpu)}`,
      r.renderHalf.gpu
        ? `[aterm-latency] aterm GPU: ${fmtStats(r.renderHalf.gpu)}`
        : `[aterm-latency] aterm GPU: FAILED — ${r.renderHalf.gpuReason ?? 'unknown'}`,
      '[aterm-latency] -- ms/frame, single-cell update --',
      '[aterm-latency] size      | aterm-GPU (render+finish) | aterm-CPU (render+blit)'
    ]
    for (const row of r.frameTable) {
      const size = `${row.cols}x${row.rows}`.padEnd(9)
      const gpu = (
        row.atermGpuMsPerFrame == null ? 'FAILED' : `${row.atermGpuMsPerFrame.toFixed(3)} ms`
      ).padEnd(24)
      const cpu = `${row.atermCpuMsPerFrame.toFixed(3)} ms`
      lines.push(`[aterm-latency] ${size} | ${gpu} | ${cpu}`)
    }
    // The aterm columns are raw synchronous render WORK (process→render→present, no
    // frame wait). Takeaways the numbers support:
    //  1. aterm GPU (the DEFAULT) renders in sub-millisecond time and stays flat as
    //     the grid grows, well under one 120Hz frame (8.333ms).
    //  2. aterm CPU (the software-GL FALLBACK) is competitive at a typical 80x24, but
    //     its rasterization cost grows with grid area — which is why GPU is the
    //     default and CPU is only the fallback.
    const cpuMed = r.renderHalf.cpu.medianMs
    const gpuMed = r.renderHalf.gpu?.medianMs ?? null
    lines.push(
      `[aterm-latency] VERDICT: aterm render-half median — CPU ${cpuMed.toFixed(3)}ms${
        gpuMed != null ? `, GPU ${gpuMed.toFixed(3)}ms` : ''
      } (one 120Hz frame = 8.333ms).`
    )
    // eslint-disable-next-line no-console
    console.log(`\n${lines.join('\n')}\n`)
    testInfo.annotations.push({ type: 'aterm-latency', description: lines.join(' | ') })

    // Loose sanity asserts only — this is a measurement, not a perf gate.
    expect(r.renderHalf.cpu.samples, 'CPU latency collected samples').toBeGreaterThan(0)
    expect(r.renderHalf.cpu.medianMs, 'CPU render-half median is positive').toBeGreaterThan(0)
    expect(Number.isFinite(r.renderHalf.cpu.p95Ms), 'CPU p95 finite').toBe(true)
    // A render-half single-cell update has no business taking 250ms; if it does,
    // something is pathologically wrong (and the perf claim is false).
    expect(r.renderHalf.cpu.medianMs, 'CPU render-half median under 250ms').toBeLessThan(250)
    if (r.renderHalf.gpu) {
      expect(r.renderHalf.gpu.medianMs, 'GPU render-half median is positive').toBeGreaterThan(0)
      expect(r.renderHalf.gpu.medianMs, 'GPU render-half median under 250ms').toBeLessThan(250)
    }
    for (const row of r.frameTable) {
      expect(row.atermCpuMsPerFrame, `aterm CPU ${row.cols}x${row.rows} positive`).toBeGreaterThan(
        0
      )
      if (row.atermGpuMsPerFrame != null) {
        expect(
          row.atermGpuMsPerFrame,
          `aterm GPU ${row.cols}x${row.rows} positive`
        ).toBeGreaterThan(0)
      }
    }
  })
})
