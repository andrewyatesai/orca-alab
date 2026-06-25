import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  readAtermRgbaByPtyId,
  atermCanvasContextInfoByPtyId,
  forceAtermContextLossByPtyId
} from './helpers/aterm-canvas-pixels'

// PROVES GAP-1 recovery: an aterm GPU pane survives a WebGL2 context loss by
// swapping to the CPU 2d draw path and keeps rendering. Drives the REAL Electron
// app with the aterm renderer + GPU path forced on, opens a pane on the webgl2
// canvas, forces a real `webglcontextlost` (via the WEBGL_lose_context
// extension), and asserts:
//   1. the controller's documented context-loss handler swaps the pane to CPU —
//      the live aterm canvas is now 2d-owned (a canvas cannot be both, so a
//      non-null 2d context proves the CPU path took over), and
//   2. the recovered CPU pane still RENDERS: new command output changes the
//      canvas pixels after the swap.
//
// Headless note (ORCA_E2E_HEADLESS): the window is hidden. We force the loss by
// calling loseContext() on the live canvas's webgl2 context — a genuine browser
// 'webglcontextlost' event, which is exactly what the GPU drawer listens for —
// so this drives the real handler, not a stubbed one.

function countChangedPixels(before: number[], after: number[]): number {
  if (after.length !== before.length) {
    return after.length
  }
  let changed = 0
  for (let i = 0; i < after.length; i += 4) {
    if (after[i] !== before[i] || after[i + 1] !== before[i + 1] || after[i + 2] !== before[i + 2]) {
      changed++
    }
  }
  return changed
}

test.describe('aterm GPU context-loss recovery', () => {
  test('a lost WebGL2 context swaps the pane to CPU and keeps rendering', async ({ orcaPage }) => {
    // Heaviest aterm spec — forces a GPU context loss, swaps to CPU, then waits on
    // rAF-throttled redraws; needs headroom beyond the default under parallel load.
    test.setTimeout(120_000)
    orcaPage.on('console', (msg) => {
      const t = msg.text()
      if (/aterm|gpu|webgl|wgpu|panic|context/i.test(t)) {
        // eslint-disable-next-line no-console
        console.log(`[renderer:${msg.type()}] ${t}`)
      }
    })

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the aterm renderer AND the experimental GPU path on BEFORE the pane.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
      ;(window as unknown as { __atermGpuEnabled?: boolean }).__atermGpuEnabled = true
    })

    // A webgl2 context must be creatable headless to prove the GPU→CPU swap.
    const hasWebgl2 = await orcaPage.evaluate(() => {
      const c = document.createElement('canvas')
      const gl = c.getContext('webgl2')
      gl?.getExtension('WEBGL_lose_context')?.loseContext()
      return Boolean(gl)
    })
    expect(hasWebgl2, 'a webgl2 context must be creatable headless to prove the GPU path').toBe(true)

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
    // Wait for the async aterm controller (wasm/font/GPU load) so the GPU path is
    // live before we force the context loss — under parallel e2e load it can attach
    // after the PTY binds.
    await waitForActiveAtermController(orcaPage)

    // The pane started on the GPU path: the live canvas is webgl2-owned (a webgl2
    // canvas returns null for getContext('2d'), so this is unambiguous). Scope to
    // the pane bound to THIS ptyId — a previously-opened terminal tab keeps its own
    // canvas mounted, so a DOM-first-match / getActivePane can read the wrong pane.
    const beforeLoss = (await atermCanvasContextInfoByPtyId(orcaPage, ptyId)) ?? {
      gl: false,
      twoD: true
    }
    // eslint-disable-next-line no-console
    console.log(`[aterm-context-loss] before-loss webgl2=${beforeLoss.gl} 2d=${beforeLoss.twoD}`)
    expect(beforeLoss.gl, 'the pane must start on the GPU (webgl2) path').toBe(true)
    expect(beforeLoss.twoD, 'a webgl2 canvas cannot also be 2d').toBe(false)

    // Render some output first so the GPU pane is genuinely drawing.
    await execInTerminal(orcaPage, ptyId, 'printf "before-loss line\\n"')

    // Force a REAL WebGL2 context loss on THIS pane's live canvas — the same
    // 'webglcontextlost' event the GPU drawer listens for. This drives the
    // controller's documented context-loss → CPU-swap handler (swapToCpu).
    const lost = await forceAtermContextLossByPtyId(orcaPage, ptyId)
    expect(lost, 'WEBGL_lose_context must be available to force the context loss').toBe(true)

    // The swap is async (it loads the CPU drawer): poll until the ACTIVE pane's
    // aterm canvas is 2d-owned. swapToCpu replaces the canvas element in place (a
    // webgl2-poisoned canvas can't be reused for 2d), so the new canvas keeps the
    // same testid and is the CPU path's surface.
    await expect
      .poll(async () => atermCanvasContextInfoByPtyId(orcaPage, ptyId), {
        timeout: 20_000,
        message: 'the pane should swap to the CPU 2d path after the WebGL2 context loss'
      })
      .toEqual({ gl: false, twoD: true })

    // eslint-disable-next-line no-console
    console.log('[aterm-context-loss] swapped to CPU 2d path after context loss')

    // The recovered CPU pane must still RENDER: snapshot the (now 2d) active-pane
    // canvas, run a command, and assert the pixels change — proving rendering
    // survived the swap, not just that a 2d canvas exists.
    const beforeOutput = await readAtermRgbaByPtyId(orcaPage, ptyId)
    expect(beforeOutput, 'should snapshot the recovered CPU canvas').not.toBeNull()

    await execInTerminal(orcaPage, ptyId, 'printf "after-recovery RECOV-XYZ\\n"')

    await expect
      .poll(
        async () => {
          const after = await readAtermRgbaByPtyId(orcaPage, ptyId)
          return after ? countChangedPixels(beforeOutput!.data, after.data) : 0
        },
        {
          // Generous: the post-swap redraw rides the rAF draw scheduler, which the
          // hidden e2e window throttles hard under heavy parallel load.
          timeout: 35_000,
          message: 'the recovered CPU pane must keep rendering — new output changes the canvas'
        }
      )
      .toBeGreaterThan(200)

    // eslint-disable-next-line no-console
    console.log('[aterm-context-loss] PASS — CPU pane rendered after recovery')
  })
})
