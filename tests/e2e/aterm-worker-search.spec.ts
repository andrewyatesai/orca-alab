import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES in-terminal SEARCH works on the DEFAULT off-main worker path — the path the
// rest of the e2e suite opts OUT of (fixture forces __atermWorkerRender=false). On the
// worker the engine (and its match set) live off-main, so term.search() can't return
// matches synchronously: the count/active-index/clear must round-trip through the worker
// and surface via the per-frame STATE snapshot. This regresses the connectedness gap
// where the count stuck at "0" and next/prev/clear were dead (their worker handlers
// existed but nothing posted them). Asserts, against the REAL controller on the worker
// path: (1) the match COUNT surfaces from the snapshot, (2) next/prev ADVANCE the active
// match, (3) clear RESETS the count to 0.
//
// All probes resolve the pane BY PTY ID: the backgrounded initial pane also attaches a
// controller, and a search fed to that HIDDEN pane never surfaces a count — its worker
// frame scheduler is suspended, so the STATE snapshot carrying it is never posted.

type AtermWorkerSearchProbe = {
  process: (data: string) => void
  findMatches: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
}

function findControllerByPtyId(ptyId: string): AtermWorkerSearchProbe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getPanes?: () => { container?: HTMLElement; atermController?: AtermWorkerSearchProbe }[]
    }
    for (const pane of m.getPanes?.() ?? []) {
      if (pane?.container?.dataset?.ptyId === ptyId && pane.atermController) {
        return pane.atermController
      }
    }
  }
  throw new Error(`no aterm controller for pty ${ptyId}`)
}

const TOKEN = 'ZZWORKERSEARCHZZ'
const OCCURRENCES = 5

test.describe('aterm worker-path in-terminal search', () => {
  test('count, next/prev, and clear over the worker seam', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the off-main worker path on BEFORE the pane is created.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermWorkerRender?: boolean }).__atermWorkerRender = true
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForAtermControllerByPtyId(orcaPage, ptyId)

    // Confirm we are genuinely on the worker path: the grid canvas is transferred to the
    // worker, so a main-thread getContext('2d') THROWS (a CPU pane returns a context, a
    // GPU pane returns null without throwing — neither is a false positive).
    const transferred = await orcaPage.evaluate((id) => {
      const managers = (window as unknown as { __paneManagers?: Map<string, unknown> })
        .__paneManagers
      let c: HTMLCanvasElement | null = null
      for (const manager of managers?.values() ?? []) {
        const m = manager as { getPanes?: () => { container?: HTMLElement }[] }
        for (const pane of m.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === id) {
            c = pane.container.querySelector('[data-testid="aterm-canvas"]')
          }
        }
      }
      if (!c) {
        return false
      }
      try {
        c.getContext('2d')
        return false
      } catch {
        return true
      }
    }, ptyId)
    expect(transferred, 'the grid canvas must be worker-owned (OffscreenCanvas)').toBe(true)

    // Feed the token several times so next/prev have somewhere to navigate, then run the
    // search through the controller (term.search posts searchFind to the worker).
    await orcaPage.evaluate(
      ({ findSrc, ptyId, token, occurrences }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (id: string) => AtermWorkerSearchProbe
        const ctrl = find(ptyId)
        let out = ''
        for (let i = 0; i < occurrences; i++) {
          out += `${token} on line ${i}\r\n`
        }
        ctrl.process(out)
        ctrl.findMatches(token, true, false)
      },
      { findSrc: findControllerByPtyId.toString(), ptyId, token: TOKEN, occurrences: OCCURRENCES }
    )

    const readSearch = (): Promise<{ count: number; activeIndex: number }> =>
      orcaPage.evaluate(
        ({ findSrc, ptyId }) => {
          // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
          const find = new Function(`return (${findSrc})`)() as (
            id: string
          ) => AtermWorkerSearchProbe
          const ctrl = find(ptyId)
          return { count: ctrl.searchMatchCount(), activeIndex: ctrl.searchActiveMatchIndex() }
        },
        { findSrc: findControllerByPtyId.toString(), ptyId }
      )

    // (1) The match COUNT surfaces from the worker snapshot (the original bug: stuck at 0).
    await expect
      .poll(async () => (await readSearch()).count, {
        timeout: 15_000,
        message: 'the worker-path search count must surface from the snapshot (was stuck at 0)'
      })
      .toBe(OCCURRENCES)

    const afterFind = await readSearch()
    expect(afterFind.activeIndex, 'an active match is selected (1-based)').toBeGreaterThan(0)

    // (2) NEXT advances the active match (the worker advances its index + pushes it back).
    await orcaPage.evaluate(
      ({ findSrc, ptyId }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (id: string) => AtermWorkerSearchProbe
        find(ptyId).findNextMatch()
      },
      { findSrc: findControllerByPtyId.toString(), ptyId }
    )
    await expect
      .poll(async () => (await readSearch()).activeIndex, {
        timeout: 15_000,
        message: 'findNextMatch must advance the active index over the worker seam'
      })
      .not.toBe(afterFind.activeIndex)

    const afterNext = await readSearch()
    expect(afterNext.count, 'the count is unchanged by navigation').toBe(OCCURRENCES)

    // (3) CLEAR resets the count to 0 (the worker drops its match set + stops highlighting).
    await orcaPage.evaluate(
      ({ findSrc, ptyId }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (id: string) => AtermWorkerSearchProbe
        find(ptyId).clearSearch()
      },
      { findSrc: findControllerByPtyId.toString(), ptyId }
    )
    await expect
      .poll(async () => (await readSearch()).count, {
        timeout: 15_000,
        message: 'clearSearch must reset the worker-path match count to 0'
      })
      .toBe(0)
  })
})
