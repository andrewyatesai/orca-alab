import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// HONEST GPU-vs-CPU frame-time benchmark for the aterm renderer, run inside the
// REAL Electron renderer (headless WebGL2 = ANGLE-over-Metal on this machine).
// The whole justification for the WebGL2 GPU draw path is that it should be
// FASTER than the CPU putImageData path at large grids; this measures whether
// that holds. For each grid it builds a fresh CPU engine (aterm-wasm) and a fresh
// GPU engine (aterm-gpu-web) at the same content + theme + font px and times N
// per-frame draws:
//   - CPU: term.render() (wasm rasterize) + putImageData blit — the real CPU
//     per-frame cost the live pane pays.
//   - GPU: gpuTerm.render() — the full WebGL2 present (atlas upload + instanced
//     draw + blit into the swapchain).
// Each frame toggles a cell so neither engine no-ops an unchanged grid. The GPU
// glyph-atlas warm-up (first frame) and the one-time init/wasm-load are reported
// SEPARATELY since they're real one-time costs, not steady-state. Tagged
// @aterm-gpu-perf so it runs on demand. If GPU init fails headless the test says
// so loudly rather than fabricating a win.

type PathBench = {
  path: 'cpu' | 'gpu'
  mutation: 'sparse' | 'full'
  cols: number
  rows: number
  frames: number
  width: number
  height: number
  totalMs: number
  msPerFrame: number
  fps: number
  firstFrameMs: number
  initMs: number
  submitMsPerFrame: number
}

type ModeRow = {
  cols: number
  rows: number
  mutation: 'sparse' | 'full'
  cpu: PathBench
  gpu: PathBench | null
}

type BenchRow = {
  cols: number
  rows: number
  sparse: ModeRow
  full: ModeRow
}

type BenchResult = {
  available: boolean
  reason?: string
  rows: BenchRow[]
  adapterInfo: string | null
  glRenderer: string | null
  glVendor: string | null
  cpuWasmLoadMs: number
  gpuWasmLoadMs: number
}

type BenchProbe = {
  __atermGpuCpuBench?: (sizes: [number, number][], frames: number) => Promise<BenchResult>
}

test.describe('aterm GPU-vs-CPU frame-time perf @aterm-gpu-perf', () => {
  test('measures CPU vs WebGL2 GPU ms/frame at 80x24, 120x40, 200x50, 400x100', async ({
    orcaPage
  }, testInfo) => {
    // Surface renderer-process console so a GPU init/panic is visible in the log.
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

    // Force the aterm renderer + GPU opt-in BEFORE the pane (the bench hook builds
    // its own GPU engines, but the GPU path must be loadable for the import).
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
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

    // Wait for the bench hook to be installed (set once the async controller — wasm
    // + font load — attaches).
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            () => typeof (window as unknown as BenchProbe).__atermGpuCpuBench === 'function'
          ),
        { timeout: 30_000, message: 'aterm GPU/CPU bench hook should be ready' }
      )
      .toBe(true)

    const sizes: [number, number][] = [
      [80, 24],
      [120, 40],
      [200, 50],
      [400, 100]
    ]
    const FRAMES = 120

    const result = (await orcaPage.evaluate(
      ({ sizes, frames }) => {
        const fn = (window as unknown as BenchProbe).__atermGpuCpuBench
        return fn ? fn(sizes, frames) : null
      },
      { sizes, frames: FRAMES }
    )) as BenchResult | null

    expect(result, 'bench returned a result').not.toBeNull()
    const r = result as BenchResult

    // Header: which GL backend produced these numbers (interpretability) + the
    // one-time wasm load costs.
    const header = [
      `[aterm-gpu-perf] GL renderer=${r.glRenderer ?? '<none>'} vendor=${r.glVendor ?? '<none>'}`,
      `[aterm-gpu-perf] wgpu adapter=${r.adapterInfo ?? '<none>'}`,
      `[aterm-gpu-perf] wasm load (one-time): cpu=${r.cpuWasmLoadMs.toFixed(1)}ms gpu=${r.gpuWasmLoadMs.toFixed(1)}ms`
    ]

    // Render one mutation mode as a table. `sparse` = 1 cell changes/frame (typical
    // terminal output; the GPU re-encodes only the dirty row, CPU still rasterizes
    // the whole grid). `full` = every cell changes/frame (worst case; both paths do
    // full per-frame work). Reporting both keeps the comparison honest.
    const renderTable = (mode: 'sparse' | 'full'): string[] => {
      const label = mode === 'sparse' ? 'SPARSE (1 cell/frame)' : 'FULL (whole grid/frame)'
      const out: string[] = [
        `[aterm-gpu-perf] -- ${label} --`,
        '[aterm-gpu-perf] size      | CPU ms/frame (fps)   | GPU ms/frame (fps)   | speedup | GPU submit-only | GPU warm-up'
      ]
      for (const row of r.rows) {
        const m = row[mode]
        const c = m.cpu
        const g = m.gpu
        const cpuCell = `${c.msPerFrame.toFixed(3)} (${c.fps.toFixed(0)})`.padEnd(20)
        if (!g) {
          out.push(
            `[aterm-gpu-perf] ${`${row.cols}x${row.rows}`.padEnd(9)} | ${cpuCell} | GPU FAILED          |    -    | -               | -`
          )
          continue
        }
        const gpuCell = `${g.msPerFrame.toFixed(3)} (${g.fps.toFixed(0)})`.padEnd(20)
        const speedup = g.msPerFrame > 0 ? c.msPerFrame / g.msPerFrame : 0
        // submit-only = render() with NO gl.finish() (command submission cost). The
        // gap up to ms/frame is the GPU completion the finish() forces — shown so
        // the synced number is verifiably a real frame, not a queue-and-return.
        const submit = `${g.submitMsPerFrame.toFixed(3)} ms`.padEnd(15)
        const warmup = `init ${g.initMs.toFixed(1)}ms + atlas ${g.firstFrameMs.toFixed(1)}ms`
        out.push(
          `[aterm-gpu-perf] ${`${row.cols}x${row.rows}`.padEnd(9)} | ${cpuCell} | ${gpuCell} | ${`${speedup.toFixed(2)}x`.padEnd(7)} | ${submit} | ${warmup}`
        )
      }
      return out
    }

    const lines = [...header, ...renderTable('sparse'), ...renderTable('full')]
    // eslint-disable-next-line no-console
    console.log(`\n${lines.join('\n')}\n`)
    testInfo.annotations.push({ type: 'aterm-gpu-perf', description: lines.join(' | ') })

    // If the GPU path could not run at all, FAIL LOUDLY with the reason — do not
    // pass silently (and do not pretend there was a win).
    expect(
      r.available,
      `the WebGL2 GPU bench must run headless to compare paths; reason=${r.reason ?? 'unknown'}`
    ).toBe(true)

    // Sanity: every measurement is a real, finite, positive frame time, and the
    // CPU/GPU pair rasterized the SAME framebuffer extent (apples-to-apples work).
    const checkMode = (m: ModeRow): void => {
      const tag = `${m.cols}x${m.rows} (${m.mutation})`
      expect(m.cpu.msPerFrame, `CPU ${tag} positive frame time`).toBeGreaterThan(0)
      expect(Number.isFinite(m.cpu.fps), `CPU ${tag} fps finite`).toBe(true)
      if (m.gpu) {
        expect(m.gpu.msPerFrame, `GPU ${tag} positive frame time`).toBeGreaterThan(0)
        expect(Number.isFinite(m.gpu.fps), `GPU ${tag} fps finite`).toBe(true)
        expect(m.gpu.width, `GPU/CPU same frame width at ${tag}`).toBe(m.cpu.width)
        expect(m.gpu.height, `GPU/CPU same frame height at ${tag}`).toBe(m.cpu.height)
      }
    }
    for (const row of r.rows) {
      checkMode(row.sparse)
      checkMode(row.full)
    }
  })
})
