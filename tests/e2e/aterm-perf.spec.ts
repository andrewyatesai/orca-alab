import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// Honest in-wasm render perf for the aterm CPU rasterizer. Native cargo benches
// reported ~1119 fps @80x24, 236 @200x50 — but wasm is slower than native, so
// this measures the REAL engine inside the real Electron renderer via the
// controller's e2e-only benchmarkRender() seam (fills realistic SGR content,
// warms up, then times N pure term.render() calls). Tagged @aterm-perf so it
// only runs on demand; it characterizes whether the CPU/wasm path sustains 60fps.

type BenchResult = {
  cols: number
  rows: number
  frames: number
  totalMs: number
  msPerFrame: number
  fps: number
}

type AtermPerfControllerProbe = {
  benchmarkRender?: (cols: number, rows: number, frames: number) => BenchResult
}

test.describe('aterm renderer in-wasm perf @aterm-perf', () => {
  test('measures ms/frame + fps at 80x24, 120x40, 200x50', async ({ orcaPage }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)

    // Wait for the async controller (wasm + font load) to be attached.
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const managers = (window as unknown as { __paneManagers?: Map<string, unknown> })
              .__paneManagers
            if (!managers) {
              return false
            }
            for (const manager of managers.values()) {
              const m = manager as {
                getActivePane?: () => { atermController?: AtermPerfControllerProbe | null } | null
                getPanes?: () => { atermController?: AtermPerfControllerProbe | null }[]
              }
              const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
              if (typeof pane?.atermController?.benchmarkRender === 'function') {
                return true
              }
            }
            return false
          }),
        { timeout: 20_000, message: 'aterm controller benchmark seam should be ready' }
      )
      .toBe(true)

    const sizes: [number, number][] = [
      [80, 24],
      [120, 40],
      [200, 50]
    ]
    const FRAMES = 200

    const results = await orcaPage.evaluate(
      ({ sizes, frames }) => {
        const managers = (window as unknown as { __paneManagers?: Map<string, unknown> })
          .__paneManagers
        let controller: AtermPerfControllerProbe | null = null
        if (managers) {
          for (const manager of managers.values()) {
            const m = manager as {
              getActivePane?: () => { atermController?: AtermPerfControllerProbe | null } | null
              getPanes?: () => { atermController?: AtermPerfControllerProbe | null }[]
            }
            const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
            if (typeof pane?.atermController?.benchmarkRender === 'function') {
              controller = pane.atermController
              break
            }
          }
        }
        if (!controller?.benchmarkRender) {
          return null
        }
        return sizes.map(([cols, rows]) => controller!.benchmarkRender!(cols, rows, frames))
      },
      { sizes, frames: FRAMES }
    )

    expect(results, 'benchmark returned results').not.toBeNull()
    const rows = results as BenchResult[]

    // Emit a human-readable perf table the runner captures verbatim.
    const lines = rows.map(
      (r) =>
        `[aterm-perf] ${r.cols}x${r.rows}: ${r.msPerFrame.toFixed(3)} ms/frame  (${r.fps.toFixed(
          1
        )} fps) over ${r.frames} frames`
    )
    console.log(`\n${lines.join('\n')}\n`)
    // Surface in the Playwright report too.
    testInfo.annotations.push({ type: 'aterm-perf', description: lines.join(' | ') })

    // Sanity: every measurement produced a real, finite frame time.
    for (const r of rows) {
      expect(r.msPerFrame, `${r.cols}x${r.rows} produced a positive frame time`).toBeGreaterThan(0)
      expect(Number.isFinite(r.fps), `${r.cols}x${r.rows} fps is finite`).toBe(true)
    }
  })
})
