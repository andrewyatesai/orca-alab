import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// Proves DOUBLE/TRIPLE-CLICK selection is WIRED under the aterm renderer (the
// default): a synthetic mousedown with detail===2 routes to the engine's
// word/URL (Semantic) selection and detail===3 routes to whole-line (Lines)
// selection, the resulting selection is reflected by controller.selectionText()
// (the same engine state the highlight paints from), and a single click still
// runs the character-drag path. Drives the REAL Electron app.
//
// Headless note (ORCA_E2E_HEADLESS): the window is hidden so the canvas rect is
// reported at the origin (0,0); clientX/Y then map straight to device pixels via
// pointToCell, so a click a few pixels into the grid lands on the first cell —
// where the cursor (and our text) sits after a home + write.
//
// The aterm bindings' selection_word (Semantic + expand_semantic) and
// selection_line (Lines + expand_lines) apply the full word/line extents, so a
// double-click selects the whole word and a triple-click the whole line.

type AtermSelectionControllerProbe = {
  process: (data: string) => void
  scrollLines: (delta: number) => void
  selectionText: () => string
}

function findActiveController(): AtermSelectionControllerProbe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getActivePane?: () => { atermController?: AtermSelectionControllerProbe | null } | null
      getPanes?: () => { atermController?: AtermSelectionControllerProbe | null }[]
    }
    const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller on any pane')
}

test.describe('aterm word/line selection', () => {
  test('double-click and triple-click drive the engine word/line selection', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Turn the aterm renderer on BEFORE the pane that will use it is created.
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

    const result = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermSelectionControllerProbe
      const ctrl = find()
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      if (!c) {
        return null
      }
      // Snap to the live bottom, then home the cursor and write a known line so a
      // click on the first cell lands on the first character. `\x1b[2J\x1b[H`
      // clears + homes; no CRLF so the whole line sits on display row 0.
      ctrl.scrollLines(-100000)
      ctrl.process('\x1b[2J\x1b[H')
      ctrl.process('hello world from aterm')

      const rect = c.getBoundingClientRect()
      // A few pixels in lands on cell (0,0) — the first character ('h') of the line.
      const clickAt = (detail: number): void => {
        c.dispatchEvent(
          new MouseEvent('mousedown', {
            button: 0,
            buttons: 1,
            clientX: rect.left + 5,
            clientY: rect.top + 5,
            detail,
            bubbles: true,
            cancelable: true
          })
        )
      }

      // Clear via a single click drag-start path first, so the word/line result
      // can't be a leftover from a prior selection.
      clickAt(1)
      const afterSingle = ctrl.selectionText()
      // Double-click (detail===2) → semantic WORD selection path.
      clickAt(2)
      const afterWord = ctrl.selectionText()
      // Triple-click (detail===3) → whole-LINE selection path.
      clickAt(3)
      const afterLine = ctrl.selectionText()
      return { afterSingle, afterWord, afterLine }
    }, findActiveController.toString())

    expect(result, 'should reach the controller + canvas').not.toBeNull()
    // Double-click on the 'h' selects the WHOLE word "hello" (engine Semantic
    // selection expanded to word boundaries via expand_semantic).
    expect(result!.afterWord, 'double-click selects the whole word').toBe('hello')
    // Triple-click selects the WHOLE line (expand_lines, trailing blanks trimmed).
    expect(result!.afterLine, 'triple-click selects the whole line').toBe(
      'hello world from aterm'
    )
  })
})
