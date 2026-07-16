import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES live re-theming: a theme change re-colours an OPEN aterm pane in place
// (controller.updateTheme → engine set_theme + re-seed + redraw) on the SAME canvas
// element — no pane rebuild, so scrollback is preserved. Drives the CPU path for a
// clean headless pixel read; the engine theme is shared with the GPU path.

type Probe = {
  process: (d: string) => void
  updateTheme: (c: {
    fg: number
    bg: number
    cursor: number
    selection: number
    palette: { index: number; rgb: number }[]
  }) => void
}
function findController(): Probe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  for (const m of managers?.values() ?? []) {
    const mgr = m as {
      getActivePane?: () => { atermController?: Probe | null } | null
      getPanes?: () => { atermController?: Probe | null }[]
    }
    const pane = mgr.getActivePane?.() ?? mgr.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller')
}

const NEW_BG = 0x123456 // a distinctive bg unlikely to be any default theme bg

function topLeftRgb(): [number, number, number] | null {
  const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
  const ctx = c?.getContext('2d')
  if (!c || !ctx || !c.width) {
    return null
  }
  // Sample a few px in to dodge any 1px edge/cursor artifact.
  const d = ctx.getImageData(4, 4, 1, 1).data
  return [d[0], d[1], d[2]]
}

test.describe('aterm live re-theme', () => {
  test('updateTheme recolours an open pane in place (no rebuild)', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })
    // Cursor glow (default-on) grants window-space chrome that pads the frame around
    // the grid; this spec samples a fixed near-origin GRID pixel, so pin glow off.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalEffectsCursorGlow: false })
    })
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas).toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)

    // Clear to fill the screen with the CURRENT theme bg, capture it + the canvas
    // identity (so we can prove the SAME element is reused, not a rebuilt one).
    const before = await orcaPage.evaluate(
      (args: { findSrc: string; topSrc: string }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${args.findSrc})()`) as () => {
          process: (d: string) => void
        }
        find().process('\x1b[2J\x1b[H')
        const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement
        ;(c as unknown as { __id?: number }).__id = 0xabcdef
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const top = new Function(`return (${args.topSrc})()`) as () =>
          | [number, number, number]
          | null
        return top()
      },
      { findSrc: findController.toString(), topSrc: topLeftRgb.toString() }
    )
    expect(before, 'should read the pre-retheme bg').not.toBeNull()

    // Re-theme to a distinctive bg and assert the OPEN canvas repaints to it on the
    // SAME element (proving an in-place re-theme, not a pane rebuild).
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            (args: { findSrc: string; topSrc: string; newBg: number }) => {
              // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
              const find = new Function(`return (${args.findSrc})()`) as () => {
                updateTheme: (c: {
                  fg: number
                  bg: number
                  cursor: number
                  selection: number
                  palette: { index: number; rgb: number }[]
                }) => void
              }
              find().updateTheme({
                fg: 0xffffff,
                bg: args.newBg,
                cursor: 0x50fa7b,
                selection: 0x264f78,
                palette: []
              })
              const c = document.querySelector(
                '[data-testid="aterm-canvas"]'
              ) as HTMLCanvasElement | null
              const sameCanvas = (c as unknown as { __id?: number })?.__id === 0xabcdef
              // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
              const top = new Function(`return (${args.topSrc})()`) as () =>
                | [number, number, number]
                | null
              const rgb = top()
              const want = [(args.newBg >> 16) & 0xff, (args.newBg >> 8) & 0xff, args.newBg & 0xff]
              const matched =
                !!rgb &&
                Math.abs(rgb[0] - want[0]) <= 6 &&
                Math.abs(rgb[1] - want[1]) <= 6 &&
                Math.abs(rgb[2] - want[2]) <= 6
              return sameCanvas && matched
            },
            { findSrc: findController.toString(), topSrc: topLeftRgb.toString(), newBg: NEW_BG }
          ),
        { timeout: 20_000, message: 'the open pane should repaint to the new theme bg in place' }
      )
      .toBe(true)
  })
})
