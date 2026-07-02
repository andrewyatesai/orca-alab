import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES the OPT-IN, default-OFF aterm render worker can run the GPU engine
// (aterm-gpu-web) INSIDE the worker on a transferred OffscreenCanvas, so GPU
// render+present happens OFF the renderer main thread — the universal off-main win.
// With both the worker-render flag AND the GPU flag forced on BEFORE the pane:
//   1. the pane's grid canvas is transferred to the worker (getContext throws on the
//      main side) — proof the OffscreenCanvas worker path was taken, and
//   2. the worker rasterized: it posts a STATE snapshot with a non-zero framebuffer
//      + grid and, after real output, an advanced cursor.
//
// TOLERANCE: headless WebGL is software (SwiftShader) and GPU-in-a-worker via
// OffscreenCanvas MAY or may not be available. So this asserts EITHER (a) the GPU
// worker rendered (state.engine === 'gpu'), OR (b) it cleanly fell back to the CPU
// worker and rendered (state.engine === 'cpu') — both mean the off-main render
// happened. The path that ran is logged. A headless-no-GPU-in-worker condition must
// NOT fail the suite. The flags are default-off, so production is unaffected.

test.describe('aterm off-main GPU render worker', () => {
  test('renders a pane via the GPU worker, or cleanly falls back to the CPU worker', async ({
    orcaPage
  }) => {
    // Surface renderer console/page errors so a worker boot/render failure (which
    // would otherwise look like a silent timeout) is visible in the report.
    orcaPage.on('console', (msg) => {
      const t = msg.text()
      if (/aterm|worker|offscreen|webgl|gpu|panic/i.test(t)) {
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

    // Force the off-main worker mirror AND the GPU path on BEFORE the pane so the
    // worker mirror picks the GPU worker engine (it falls back to the
    // CPU worker if WebGL can't be acquired inside the worker).
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermWorkerRender?: boolean }).__atermWorkerRender = true
      ;(window as unknown as { __atermGpuEnabled?: boolean }).__atermGpuEnabled = true
    })

    // New terminal tab → its pane is rendered by the worker mirror.
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })
    const ptyId = await waitForActivePanePtyId(orcaPage)

    // The grid canvas is transferred to the worker, so the main thread can no longer
    // acquire a context for it: getContext THROWS InvalidStateError. This uniquely
    // identifies the OffscreenCanvas worker — a non-worker CPU pane returns a live 2d
    // context and a non-worker GPU pane returns null WITHOUT throwing.
    const ownership = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      if (!c) {
        return { found: false, transferred: false, detail: 'no canvas' }
      }
      try {
        const ctx = c.getContext('2d')
        return { found: true, transferred: false, detail: ctx ? 'has 2d (CPU)' : 'null 2d (GPU)' }
      } catch (e) {
        return { found: true, transferred: true, detail: String(e) }
      }
    })
    // eslint-disable-next-line no-console
    console.log(`[aterm-gpu-worker] canvas ownership: ${JSON.stringify(ownership)}`)
    expect(ownership.found, 'the aterm grid canvas should exist').toBe(true)
    expect(
      ownership.transferred,
      `the grid canvas must be worker-owned (OffscreenCanvas); got: ${ownership.detail}`
    ).toBe(true)

    // Run real output so the worker engine processes the mirrored bytes + re-renders.
    await execInTerminal(orcaPage, ptyId, 'printf "GPU_WORKER_RENDER_OK\\n"')

    // The worker posts a STATE snapshot (with the engine tag) after each draw; assert
    // it rasterized a real, non-zero framebuffer + grid AND that output advanced the
    // cursor. The engine tag may be 'gpu' (GPU worker) OR 'cpu' (clean fallback) —
    // both prove the off-main render happened, so either is a PASS.
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const s = (
              window as unknown as {
                __atermWorkerRenderState?: {
                  engine: 'cpu' | 'gpu'
                  width: number
                  height: number
                  cols: number
                  rows: number
                  cursorX: number
                  cursorY: number
                  baseY: number
                }
              }
            ).__atermWorkerRenderState
            if (!s) {
              return false
            }
            const sized = s.width > 0 && s.height > 0 && s.cols > 0 && s.rows > 0
            const advanced = s.cursorY > 0 || s.baseY > 0 || s.cursorX > 0
            return sized && advanced
          }),
        {
          timeout: 20_000,
          message: 'the render worker (GPU or CPU fallback) should post a non-blank state'
        }
      )
      .toBe(true)

    const finalState = await orcaPage.evaluate(
      () =>
        (
          window as unknown as {
            __atermWorkerRenderState?: { engine: 'cpu' | 'gpu' }
          }
        ).__atermWorkerRenderState ?? null
    )
    const enginePath = finalState?.engine === 'gpu' ? 'GPU-in-worker' : 'CPU-worker fallback'
    // eslint-disable-next-line no-console
    console.log(
      `[aterm-gpu-worker] PASS via ${enginePath} — worker render state: ${JSON.stringify(finalState)}`
    )
    // Both off-main paths are valid; just assert the worker reported a known engine.
    expect(['cpu', 'gpu']).toContain(finalState?.engine)
  })
})
