import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
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

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    // Wait for THIS pane's aterm controller (by ptyId; wasm/font/GPU load) so the
    // in-page probe below finds it — under parallel e2e load it can attach after the
    // PTY binds (and the backgrounded initial pane's controller can attach first).
    await waitForAtermControllerByPtyId(orcaPage, ptyId)

    const ready = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermSelectionControllerProbe
      const ctrl = find()
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      if (!c) {
        return false
      }
      // Snap to the live bottom, then home the cursor and write a known line so a
      // click on the first cell lands on the first character. `\x1b[2J\x1b[H`
      // clears + homes; no CRLF so the whole line sits on display row 0.
      ctrl.scrollLines(-100000)
      ctrl.process('\x1b[2J\x1b[H')
      ctrl.process('hello world from aterm')
      return true
    }, findActiveController.toString())
    expect(ready, 'should reach the controller + canvas').toBe(true)

    // A few pixels in lands on cell (0,0) — the first character ('h') of the line.
    const clickAt = (detail: number): Promise<void> =>
      orcaPage.evaluate((clickDetail: number) => {
        const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement
        const rect = c.getBoundingClientRect()
        c.dispatchEvent(
          new MouseEvent('mousedown', {
            button: 0,
            buttons: 1,
            clientX: rect.left + 5,
            clientY: rect.top + 5,
            detail: clickDetail,
            bubbles: true,
            cancelable: true
          })
        )
      }, detail)
    // On the worker render path (the production default) selection mutations and
    // selectionText() round-trip through the worker's per-frame snapshot, so POLL the
    // exact expected text instead of a same-tick sync read; the in-process path
    // resolves on the first poll.
    const pollSelection = (message: string): ReturnType<typeof expect.poll> =>
      expect.poll(
        async () =>
          orcaPage.evaluate((findSrc: string) => {
            // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
            const find = new Function(
              `return (${findSrc})()`
            ) as () => AtermSelectionControllerProbe
            return find().selectionText()
          }, findActiveController.toString()),
        { timeout: 15_000, message }
      )

    // Clear via a single click drag-start path first, so the word/line result
    // can't be a leftover from a prior selection.
    await clickAt(1)
    // Double-click (detail===2) → semantic WORD selection path. Double-click on the
    // 'h' selects the WHOLE word "hello" (engine Semantic selection expanded to word
    // boundaries via expand_semantic).
    await clickAt(2)
    await pollSelection('double-click selects the whole word').toBe('hello')
    // Triple-click (detail===3) → whole-LINE selection path (expand_lines, trailing
    // blanks trimmed).
    await clickAt(3)
    await pollSelection('triple-click selects the whole line').toBe('hello world from aterm')
  })
})
