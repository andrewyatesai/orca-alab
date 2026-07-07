import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES the OPT-IN, default-OFF aterm render mirror (plan §9, stage 2a) renders a
// pane OFF the renderer main thread. With the worker-render flag forced on BEFORE
// the pane is created, the pane's grid canvas is transferred to a render worker via
// transferControlToOffscreen(), so:
//   1. the main thread can no longer get a 2d/webgl2 context for it (getContext
//      throws/returns null) — proof the OffscreenCanvas path was taken, NOT the
//      in-thread CPU/GPU drawers, and
//   2. the worker actually rasterized: it posts a STATE snapshot with a non-zero
//      framebuffer + grid, and after real output the cursor advances past the
//      origin — the mirrored bytes were processed + re-rendered by the worker.
// The flag is default-off, so production keeps the proven main-thread CPU/GPU paths.

test.describe('aterm off-main render mirror', () => {
  test('renders a pane on a worker-owned OffscreenCanvas', async ({ orcaPage }) => {
    // Surface renderer console/page errors so a worker boot/render failure (which
    // would otherwise look like a silent timeout) is visible in the report.
    orcaPage.on('console', (msg) => {
      const t = msg.text()
      if (/aterm|worker|offscreen|panic/i.test(t)) {
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

    // Force the off-main worker mirror on BEFORE the pane.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermWorkerRender?: boolean }).__atermWorkerRender = true
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
    // identifies the OffscreenCanvas mirror — a CPU pane returns a live 2d context
    // and a GPU pane returns null (webgl2-owned) WITHOUT throwing, so neither is a
    // false positive.
    const ownership = await orcaPage.evaluate((ptyId) => {
      // Scope to the pane under test by ptyId: the FIRST canvas in the DOM is the
      // bootstrap pane (in-process, GPU-owned on capable hosts — getContext('2d')
      // returns null WITHOUT throwing), not this test's worker-owned pane.
      const managers = (
        window as unknown as {
          __paneManagers?: Map<
            string,
            {
              getPanes?: () => {
                container?: {
                  dataset?: { ptyId?: string }
                  querySelector: (s: string) => Element | null
                }
              }[]
            }
          >
        }
      ).__paneManagers
      let c: HTMLCanvasElement | null = null
      for (const mgr of managers?.values() ?? []) {
        for (const pane of mgr.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === ptyId) {
            c = pane.container.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
          }
        }
      }
      if (!c) {
        return { found: false, transferred: false, detail: 'no canvas' }
      }
      try {
        const ctx = c.getContext('2d')
        return { found: true, transferred: false, detail: ctx ? 'has 2d (CPU)' : 'null 2d (GPU)' }
      } catch (e) {
        return { found: true, transferred: true, detail: String(e) }
      }
    }, ptyId)
    // eslint-disable-next-line no-console
    console.log(`[aterm-worker] canvas ownership: ${JSON.stringify(ownership)}`)
    expect(ownership.found, 'the aterm grid canvas should exist').toBe(true)
    expect(
      ownership.transferred,
      `the grid canvas must be worker-owned (OffscreenCanvas); got: ${ownership.detail}`
    ).toBe(true)

    // Run real output so the worker engine processes the mirrored bytes + re-renders.
    await execInTerminal(orcaPage, ptyId, 'printf "WORKER_RENDER_OK\\n"')

    // The worker posts a STATE snapshot after each draw; assert it rasterized a real,
    // non-zero framebuffer + grid AND that output advanced the cursor past the origin
    // (so we know the off-main engine actually processed the mirrored bytes).
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const s = (
              window as unknown as {
                __atermWorkerRenderState?: {
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
          message: 'the render worker should post a non-blank, advanced state snapshot'
        }
      )
      .toBe(true)

    const finalState = await orcaPage.evaluate(
      () =>
        (window as unknown as { __atermWorkerRenderState?: unknown }).__atermWorkerRenderState ??
        null
    )
    // eslint-disable-next-line no-console
    console.log(`[aterm-worker] PASS — worker render state: ${JSON.stringify(finalState)}`)

    // The worker owns the grid canvas, so search highlights + the link underline paint
    // on a main-thread stacked overlay (Stage D.2). Assert it mounted (sized to the
    // worker framebuffer) + that the snapshot carries the overlay-driving fields.
    const overlay = await orcaPage.evaluate(() => {
      const o = document.querySelector(
        '[data-testid="aterm-worker-overlay"]'
      ) as HTMLCanvasElement | null
      const s = (
        window as unknown as {
          __atermWorkerRenderState?: { searchMatchRects?: unknown[]; hoverCursor?: string }
        }
      ).__atermWorkerRenderState
      return {
        mounted: !!o,
        sized: !!o && o.width > 0 && o.height > 0,
        hasSearchField: Array.isArray(s?.searchMatchRects),
        hasHoverField: typeof s?.hoverCursor === 'string'
      }
    })
    // eslint-disable-next-line no-console
    console.log(`[aterm-worker] overlay: ${JSON.stringify(overlay)}`)
    expect(overlay.mounted, 'the worker search/link overlay canvas should mount').toBe(true)
    expect(overlay.sized, 'the overlay should size to the worker framebuffer').toBe(true)
    expect(overlay.hasSearchField, 'snapshot should carry searchMatchRects').toBe(true)
    expect(overlay.hasHoverField, 'snapshot should carry hoverCursor').toBe(true)
  })
})
