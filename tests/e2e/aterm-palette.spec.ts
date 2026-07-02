import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES the ANSI 16-colour palette is honoured: the aterm renderer seeds the
// engine's palette (indices 0–15) from the user's theme, so an SGR-indexed cell
// renders in the THEME colour, not the engine's built-in VGA default. Renders each
// palette colour as a solid background segment (SGR 48;5;<index>) on the CPU path
// (clean getImageData; GPU output is pixel-equivalent, proven elsewhere) and
// asserts every theme palette colour actually appears on the canvas — resolved
// through the same pipeline the renderer seeds from (__resolveAtermThemePalette),
// so a renderer using the engine default for an index fails.

type Probe = { process: (d: string) => void }
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

type PaletteEntry = { index: number; rgb: [number, number, number] }

test.describe('aterm ANSI palette', () => {
  test('SGR-indexed colours render in the theme palette, not the engine default', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    // Force the CPU draw path for a clean headless getImageData read; the palette
    // lives on the shared grid so CPU + GPU resolve it identically.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
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

    // The theme palette the renderer seeded (index → [r,g,b]), resolved through the
    // SAME pipeline — an independent source of truth, not what the renderer echoed.
    const palette = (await orcaPage.evaluate(
      () =>
        (
          window as unknown as { __resolveAtermThemePalette?: () => PaletteEntry[] }
        ).__resolveAtermThemePalette?.() ?? []
    )) as PaletteEntry[]
    expect(palette.length, 'the theme should define ANSI palette colours to seed').toBeGreaterThan(
      7
    )

    // Paint each palette colour as a solid background segment; 4 cols each keeps all
    // 16 within ~64 columns so they stay on the first wrapped rows (no scroll-off).
    const payload = [
      '\x1b[2J\x1b[H',
      ...palette.map((p) => `\x1b[48;5;${p.index}m    \x1b[0m`)
    ].join('')
    await orcaPage.evaluate(
      (args: { findSrc: string; payload: string }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${args.findSrc})()`) as () => {
          process: (d: string) => void
        }
        find().process(args.payload)
      },
      { findSrc: findController.toString(), payload }
    )

    // For each seeded palette colour, assert it actually appears on the canvas. A
    // renderer that ignored the seed would paint the engine's VGA default for that
    // index instead, and the theme colour would be absent.
    const missing = await expect
      .poll(
        async () =>
          orcaPage.evaluate((expected: PaletteEntry[]) => {
            const c = document.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
            const ctx = c?.getContext('2d')
            if (!c || !ctx || !c.width || !c.height) {
              return null
            }
            const data = ctx.getImageData(0, 0, c.width, c.height).data
            const present = (want: [number, number, number]): boolean => {
              for (let y = 1; y < c.height; y += 2) {
                for (let x = 1; x < c.width; x += 4) {
                  const i = (y * c.width + x) * 4
                  if (
                    Math.abs(data[i] - want[0]) <= 6 &&
                    Math.abs(data[i + 1] - want[1]) <= 6 &&
                    Math.abs(data[i + 2] - want[2]) <= 6
                  ) {
                    return true
                  }
                }
              }
              return false
            }
            return expected.filter((p) => !present(p.rgb)).map((p) => p.index)
          }, palette),
        { timeout: 20_000, message: 'theme palette colours should appear on the canvas' }
      )
      .toEqual([])
    void missing
  })
})
