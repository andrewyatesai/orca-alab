import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { atermCanvasReady } from './helpers/aterm-canvas-pixels'
import { writeFileSync } from 'node:fs'

// PIXEL proof that theme.selectionForeground actually PAINTS selected-text glyphs.
// The aterm engine paints a selected cell's glyph in `self.selection_fg` when the
// host set one (set_selection_fg); it only WCAG-floors the cell's OWN fg when the
// theme set none (aterm-render lib.rs:2868). This drives the REAL theme path: set a
// distinctive selectionForeground (+ a distinct selectionBackground) through
// terminalColorOverrides via updateSettings → applyTerminalAppearance re-themes the
// OPEN pane in place (updateTheme → set_selection_fg). It then writes known WHITE
// text, drag-selects it, forces the selection paint to flush, and reads the selected
// glyph's pixels. A meaningful fraction must match the configured selectionForeground
// (resolved INDEPENDENTLY via the real pipeline hook) within a tiny tolerance.
//
// Why this FAILS if the feature breaks: the text is WHITE and the selection bg is
// bright GREEN. If set_selection_fg were NOT applied, the engine would fall back to
// the WCAG-floored white-on-green default — never the bright-MAGENTA configured
// selectionForeground. So magenta glyph pixels can ONLY come from the explicit
// selectionForeground reaching paint, not from the floored default.
//
// Headless geometry note (see aterm-renderer-phase1.spec.ts): the hidden window
// reports a 0x0 canvas rect, so synthetic drag client coords map straight to device
// pixels via the controller's pointToCell; a drag from the origin selects the first
// row where our text sits.

const SELECTION_FG_HEX = '#ff20e0' // bright magenta — distinct in all 3 channels
// Bright green selection bg: distinct from theme bg AND far from the magenta fg, so
// the floored white-on-green default is unmistakably NOT the configured magenta.
const SELECTION_BG_HEX = '#008800'
const TOLERANCE = 8

type Probe = {
  process: (data: string) => void
  scrollLines: (delta: number) => void
  selectionText: () => string
  cellSizeCss: () => { width: number; height: number }
}

function findActiveController(): Probe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getActivePane?: () => { atermController?: Probe | null } | null
      getPanes?: () => { atermController?: Probe | null }[]
    }
    const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller on any pane')
}

test.describe('aterm selection foreground', () => {
  test('theme.selectionForeground paints selected-text glyph pixels', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.evaluate(() => {
      // CPU path for a clean, deterministic headless pixel read; the engine's
      // selection_fg paint is shared with the GPU path (parity proven in Rust).
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)
    await expect
      .poll(async () => atermCanvasReady(orcaPage), {
        timeout: 20_000,
        message: 'aterm canvas should be ready to read'
      })
      .toBe(true)

    // Apply the distinctive selection theme through the REAL settings path. The
    // settings change re-runs applyTerminalAppearance, which calls updateTheme on
    // the open aterm pane → set_selection_fg with the configured selectionForeground.
    await orcaPage.evaluate(
      ({ fg, bg }) =>
        window.__store?.getState().updateSettings({
          terminalColorOverrides: { selectionForeground: fg, selectionBackground: bg }
        }),
      { fg: SELECTION_FG_HEX, bg: SELECTION_BG_HEX }
    )

    // Resolve the EXPECTED selectionForeground INDEPENDENTLY via the real pipeline
    // (resolveAtermThemeColors → atermThemeColorsFromITheme), not a value the
    // renderer echoed. Poll: the settings re-theme is async (React effect → IPC).
    const expectedFg = await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const fn = (
              window as unknown as {
                __resolveAtermThemeSelectionFg?: () => [number, number, number] | null
              }
            ).__resolveAtermThemeSelectionFg
            return fn ? fn() : null
          }),
        { timeout: 20_000, message: 'the theme should resolve a selectionForeground' }
      )
      .not.toBeNull()
    const want = (await orcaPage.evaluate(() => {
      const fn = (
        window as unknown as {
          __resolveAtermThemeSelectionFg?: () => [number, number, number] | null
        }
      ).__resolveAtermThemeSelectionFg
      return fn ? fn() : null
    })) as [number, number, number]
    void expectedFg
    // Sanity: the resolved value is our configured magenta (proves the real pipeline
    // carried the override through to the value that seeds set_selection_fg).
    expect(Math.abs(want[0] - 0xff), 'resolved fg R ~ ff').toBeLessThanOrEqual(TOLERANCE)
    expect(Math.abs(want[1] - 0x20), 'resolved fg G ~ 20').toBeLessThanOrEqual(TOLERANCE)
    expect(Math.abs(want[2] - 0xe0), 'resolved fg B ~ e0').toBeLessThanOrEqual(TOLERANCE)

    // Write known WHITE text, drag-select it, force the selection paint to flush,
    // and sample the selected glyph band. A closure does the whole write→select→
    // flush→sample so the poll re-runs it until the async re-theme + the rAF-
    // coalesced selection draw both land.
    const sampleBand = async (): Promise<{
      selText: string
      total: number
      matched: number
      fraction: number
    } | null> =>
      orcaPage.evaluate(
        async ({ findSrc, wantRgb, tol }) => {
          // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
          const find = new Function(`return (${findSrc})()`) as () => Probe
          const ctrl = find()
          ctrl.scrollLines(-100000) // snap to bottom
          // Clear + home, then a solid run of WHITE 'H' glyphs (dense ink per cell).
          // No CRLF: the run sits on display row 0.
          ctrl.process('\x1b[2J\x1b[H')
          ctrl.process(`\x1b[38;2;221;221;221m${'H'.repeat(20)}\x1b[0m`)

          const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement
          const dpr = window.devicePixelRatio || 1
          const mk = (type: string, deviceX: number, deviceY: number): MouseEvent =>
            new MouseEvent(type, {
              button: 0,
              buttons: 1,
              clientX: deviceX / dpr,
              clientY: deviceY / dpr,
              bubbles: true,
              cancelable: true
            })
          const cell = ctrl.cellSizeCss()
          const cellWpx = Math.max(1, Math.round(cell.width * dpr))
          const cellHpx = Math.max(1, Math.round(cell.height * dpr))
          // Drag across the first ~12 cells of row 0 to select the white run.
          c.dispatchEvent(mk('mousedown', 2, 2))
          c.dispatchEvent(mk('mousemove', cellWpx * 12, Math.floor(cellHpx / 2)))
          window.dispatchEvent(mk('mouseup', cellWpx * 12, Math.floor(cellHpx / 2)))
          const selText = ctrl.selectionText()
          // Nudge a redraw and wait two frames so the selection band + glyph repaint
          // flush to the canvas before we read pixels.
          ctrl.scrollLines(0)
          await new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(res)))

          const ctx = c.getContext('2d')
          if (!ctx) {
            return null
          }
          // Scan the selected glyph band: the first 10 cells × one cell height.
          const x1 = Math.min(c.width, cellWpx * 10)
          const y1 = Math.min(c.height, cellHpx)
          const img = ctx.getImageData(0, 0, x1, y1).data
          let total = 0
          let matched = 0
          for (let i = 0; i < img.length; i += 4) {
            total++
            if (
              Math.abs(img[i] - wantRgb[0]) <= tol &&
              Math.abs(img[i + 1] - wantRgb[1]) <= tol &&
              Math.abs(img[i + 2] - wantRgb[2]) <= tol
            ) {
              matched++
            }
          }
          return { selText, total, matched, fraction: total ? matched / total : 0 }
        },
        { findSrc: findActiveController.toString(), wantRgb: want, tol: TOLERANCE }
      )

    // Poll the fraction until the re-theme + selection paint land: a meaningful
    // fraction of the band must be the configured selectionForeground. 20 glyphs of
    // dense 'H' ink → the foreground ink is a sizable minority of the band; >1% is
    // well above noise yet only reachable when set_selection_fg painted the glyphs
    // (a broken path → the floored white-on-green default → ~0 magenta).
    await expect
      .poll(async () => (await sampleBand())?.fraction ?? 0, {
        timeout: 20_000,
        message: 'selected glyph pixels should carry the theme selectionForeground'
      })
      .toBeGreaterThan(0.01)

    // Final, non-polled assertions on a fresh sample for diagnostics.
    const result = await sampleBand()
    expect(result, 'should sample the selected glyph band').not.toBeNull()
    expect(result!.selText, 'the drag selected the H run').toContain('HHH')
    expect(result!.total, 'sampled a non-trivial glyph band').toBeGreaterThan(50)
    expect(
      result!.fraction,
      `selected glyph pixels should carry the theme selectionForeground (${want})`
    ).toBeGreaterThan(0.01)

    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    if (dataUrl.startsWith('data:image/png;base64,')) {
      writeFileSync('/tmp/aterm-selection-fg.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
    }
  })
})
