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

    // Re-run the search, snapshot the canvas, then assert the highlight overlay
    // changed pixels AND that the change is CONCENTRATED in the match region (a
    // single highlighted row band), not scattered noise across the whole canvas.
    const highlight = await orcaPage.evaluate(async (findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermSearchControllerProbe
      const ctrl = find()
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      const ctx = c?.getContext('2d')
      if (!c || !ctx || !c.width || !c.height) {
        return null
      }
      const raf = (): Promise<void> =>
        new Promise((resolve) => requestAnimationFrame(() => resolve()))
      // Clear highlights and let a frame paint to get the un-highlighted baseline.
      ctrl.clearSearch()
      await raf()
      await raf()
      const before = ctx.getImageData(0, 0, c.width, c.height).data.slice()
      // Find again (paints the highlight overlay) and let a frame land.
      ctrl.findMatches('ZZUNIQUESEARCHTOKENZZ', true)
      await raf()
      await raf()
      const after = ctx.getImageData(0, 0, c.width, c.height).data
      // Track changed-pixel count + the vertical band the changes fall in.
      let changed = 0
      let minY = c.height
      let maxY = -1
      for (let y = 0; y < c.height; y++) {
        for (let x = 0; x < c.width; x++) {
          const i = (y * c.width + x) * 4
          if (
            after[i] !== before[i] ||
            after[i + 1] !== before[i + 1] ||
            after[i + 2] !== before[i + 2]
          ) {
            changed++
            if (y < minY) {
              minY = y
            }
            if (y > maxY) {
              maxY = y
            }
          }
        }
      }
      const bandHeight = maxY >= minY ? maxY - minY + 1 : 0
      return { changed, bandHeight, canvasHeight: c.height }
    }, findActiveController.toString())

    expect(highlight, 'should read the canvas before/after the highlight paint').not.toBeNull()
    expect(
      highlight!.changed,
      'painting the search highlight overlay must change canvas pixels'
    ).toBeGreaterThan(0)
    // The highlight is a single-row translucent rect over the match; clearing it
    // reverts exactly those pixels. So the changed pixels must form a thin band
    // (a few cell-rows tall at most), NOT span most of the canvas — proving the
    // diff is the match-region highlight, not an unrelated full-canvas redraw.
    expect(
      highlight!.bandHeight,
      `changed pixels should be a thin band near the match (got ${highlight!.bandHeight}px of ${highlight!.canvasHeight}px)`
    ).toBeLessThan(highlight!.canvasHeight * 0.4)
  })
})
