import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// Proves in-terminal SEARCH works under the aterm renderer (the default): feed
// many lines incl. a unique token deep in scrollback, run the controller's
// search, and assert (1) a positive match count, (2) the viewport scrolled off
// the bottom to bring the match into view, and (3) the canvas pixels changed
// where the highlight overlay paints. Drives the REAL Electron app.
//
// Headless note (ORCA_E2E_HEADLESS): the window is hidden so DOM layout reports
// a 0x0 rect. Output is fed through the controller's process() (the exact path
// the PTY output mirror uses) and search is driven through the controller
// directly, so the assertions are deterministic without OS focus/geometry.

type AtermSearchControllerProbe = {
  process: (data: string) => void
  displayOffset: () => number
  scrollLines: (delta: number) => void
  findMatches: (query: string, caseSensitive: boolean) => number
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
  searchActiveMatchRect: () => { x: number; y: number; width: number; height: number } | null
}

function findActiveController(): AtermSearchControllerProbe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getActivePane?: () => { atermController?: AtermSearchControllerProbe | null } | null
      getPanes?: () => { atermController?: AtermSearchControllerProbe | null }[]
    }
    const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller on any pane')
}

test.describe('aterm in-terminal search', () => {
  test('match count, scroll-to-match, and highlight overlay', async ({ orcaPage }) => {
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

    // Feed a unique token deep into scrollback, then bury it under filler so the
    // match is NOT visible at the live bottom — a search must scroll to find it.
    const result = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermSearchControllerProbe
      const ctrl = find()
      ctrl.scrollLines(-100000) // snap to bottom
      ctrl.process('ZZUNIQUESEARCHTOKENZZ is here\r\n')
      let bulk = ''
      for (let i = 0; i < 300; i++) {
        bulk += `filler scrollback line ${i}\r\n`
      }
      ctrl.process(bulk)
      const offsetBeforeSearch = ctrl.displayOffset()
      const count = ctrl.findMatches('ZZUNIQUESEARCHTOKENZZ', true)
      return {
        offsetBeforeSearch,
        count,
        activeIndex: ctrl.searchActiveMatchIndex(),
        offsetAfterSearch: ctrl.displayOffset(),
        // A case-sensitive miss must yield zero and clear the highlight count.
        missCount: ctrl.findMatches('zzuniquesearchtokenzz', true)
      }
    }, findActiveController.toString())

    expect(result.offsetBeforeSearch, 'live output snaps the viewport to the bottom').toBe(0)
    expect(result.count, 'the unique token is found at least once').toBeGreaterThan(0)
    expect(result.activeIndex, 'an active match is selected (1-based)').toBeGreaterThan(0)
    expect(
      result.offsetAfterSearch,
      'searching a scrolled-off token scrolls the viewport to it'
    ).toBeGreaterThan(0)
    expect(result.missCount, 'a case-sensitive miss finds nothing').toBe(0)

    // Prove the highlight paints SPECIFICALLY on the active match cells (not just
    // "some pixels changed"): (1) capture the un-highlighted baseline, (2) find →
    // paint the overlay, (3) assert the changed pixels fall INSIDE the active
    // match's reported cell rect, and essentially none change outside it, then
    // (4) clear the search and assert the previously-changed pixels REVERT to the
    // baseline — i.e. the overlay reverts exactly the match region.
    const highlight = await orcaPage.evaluate(async (findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermSearchControllerProbe
      const ctrl = find()
      // The highlight is painted on the GPU path's stacked 2d overlay (the grid
      // canvas is webgl2-owned there); on the CPU path it's painted on the grid
      // canvas's own 2d context. Read whichever holds the highlight — both are 2d
      // (top-left origin), so the rect math below is unchanged. The overlay is
      // sized to match the grid canvas, so its device-pixel coords align 1:1.
      const overlay = document.querySelector(
        '[data-testid="aterm-search-overlay"]'
      ) as HTMLCanvasElement | null
      const grid = document.querySelector(
        '[data-testid="aterm-canvas"]'
      ) as HTMLCanvasElement | null
      const c = overlay ?? grid
      const ctx = c?.getContext('2d')
      if (!c || !ctx || !c.width || !c.height) {
        return null
      }
      const raf = (): Promise<void> =>
        new Promise((resolve) => requestAnimationFrame(() => resolve()))
      const snapshot = (): Uint8ClampedArray =>
        ctx.getImageData(0, 0, c.width, c.height).data.slice()
      const rgbDiffers = (
        a: Uint8ClampedArray,
        b: Uint8ClampedArray,
        i: number
      ): boolean => a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2]

      // Clear highlights and let a frame paint to get the un-highlighted baseline.
      ctrl.clearSearch()
      await raf()
      await raf()
      const before = snapshot()
      // Find again (paints the highlight overlay) and let a frame land.
      ctrl.findMatches('ZZUNIQUESEARCHTOKENZZ', true)
      await raf()
      await raf()
      const after = snapshot()
      // The reported device-pixel rect of the active match's highlight band.
      const rect = ctrl.searchActiveMatchRect()
      if (!rect) {
        return { rect: null }
      }
      // Categorize every changed pixel as inside-or-outside the reported rect, and
      // record which pixels changed so we can verify they revert on clear.
      let changedInside = 0
      let changedOutside = 0
      const changedIdx: number[] = []
      const inRect = (x: number, y: number): boolean =>
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
      for (let y = 0; y < c.height; y++) {
        for (let x = 0; x < c.width; x++) {
          const i = (y * c.width + x) * 4
          if (rgbDiffers(after, before, i)) {
            changedIdx.push(i)
            if (inRect(x, y)) {
              changedInside++
            } else {
              changedOutside++
            }
          }
        }
      }
      // Clear the highlight; the previously-changed match pixels must revert.
      ctrl.clearSearch()
      await raf()
      await raf()
      const cleared = snapshot()
      let reverted = 0
      for (const i of changedIdx) {
        if (!rgbDiffers(cleared, before, i)) {
          reverted++
        }
      }
      return {
        rect,
        changedInside,
        changedOutside,
        changedTotal: changedIdx.length,
        reverted
      }
    }, findActiveController.toString())

    expect(highlight, 'should read the canvas before/after the highlight paint').not.toBeNull()
    expect(highlight!.rect, 'the active match should report an on-screen cell rect').not.toBeNull()
    expect(
      highlight!.changedInside,
      'the highlight must change pixels INSIDE the active match cell rect'
    ).toBeGreaterThan(0)
    // Pixels changing OUTSIDE the reported match rect would mean the diff is not
    // the match-region highlight (e.g. a full redraw). The overlay is a single
    // translucent rect over the match cells, so out-of-rect change must be ~zero;
    // allow a tiny slack for sub-pixel edge bleed at the rect boundary.
    expect(
      highlight!.changedOutside,
      `changed pixels must be confined to the match rect (out=${highlight!.changedOutside}, in=${highlight!.changedInside})`
    ).toBeLessThanOrEqual(Math.ceil(highlight!.changedInside * 0.02))
    // Clearing the search must revert (essentially) all of the highlighted pixels
    // back to their pre-highlight values — proving the diff WAS the highlight.
    expect(
      highlight!.reverted,
      `clearing the search should revert the highlighted pixels (reverted=${highlight!.reverted}/${highlight!.changedTotal})`
    ).toBeGreaterThanOrEqual(Math.floor(highlight!.changedTotal * 0.98))
  })
})
