import { atermSearchMatchRect } from './aterm-search-overlay'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'

/** The find/next/prev/clear/count/index/rect surface the controller exposes. */
export type AtermSearchApi = {
  findMatches: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
  /** Subscribe to search-state changes that land asynchronously (the worker pushes
   *  count/active-index a frame after a posted find/next/prev). Returns a disposer;
   *  a no-op disposer in-process, where the count updates synchronously. */
  onSearchStateChange: (handler: () => void) => () => void
  searchActiveMatchRect: () => {
    x: number
    y: number
    width: number
    height: number
  } | null
}

export type AtermSearchApiDeps = {
  searchController: AtermSearchController
  term: AtermTerminal
  cellWidth: number
  cellHeight: number
  isDisposed: () => boolean
  getRows: () => number
  getSearchMatches: () => AtermSearchMatch[]
  getSearchActiveIndex: () => number
}

/** Build the controller's search method surface. Extracted so the renderer stays
 *  under the line budget; state (matches/active index/rows) is read via getters
 *  because it changes as the viewport scrolls and content updates. */
export function buildAtermSearchApi(deps: AtermSearchApiDeps): AtermSearchApi {
  const { searchController, term, cellWidth, cellHeight, isDisposed, getRows } = deps
  // Worker path: the worker owns the engine, so term.search() can't return matches over
  // the seam (the main-thread controller stays empty). It instead pushes count/active-
  // index/rect each snapshot, exposed as searchStateSnapshot. Absent in-process, where
  // the controller holds the real matches — so this is null there and we fall back.
  const facade = term as typeof term & Partial<AtermWorkerAsyncFacade>
  const workerSearch = (): ReturnType<
    NonNullable<AtermWorkerAsyncFacade['searchStateSnapshot']>
  > | null => facade.searchStateSnapshot?.() ?? null
  return {
    findMatches: (query, caseSensitive, isRegex) =>
      isDisposed() ? 0 : searchController.find(query, caseSensitive, isRegex),
    // Nav/clear run in the worker on that path (the main-thread controller is empty there);
    // in-process they fall back to the controller, which holds the real matches.
    findNextMatch: () => (facade.searchNext ? facade.searchNext() : searchController.next()),
    findPreviousMatch: () => (facade.searchPrev ? facade.searchPrev() : searchController.prev()),
    // Clear BOTH: the local controller (in-process state + overlay) and, on the worker path,
    // the worker (so it stops emitting highlight rects).
    clearSearch: () => {
      searchController.clear()
      facade.searchClear?.()
    },
    searchMatchCount: () => workerSearch()?.count ?? searchController.count(),
    searchActiveMatchIndex: () => workerSearch()?.activeIndex ?? searchController.activeIndex(),
    onSearchStateChange: (handler) => facade.onSearchStateChange?.(handler) ?? (() => undefined),
    searchActiveMatchRect: () => {
      const workerState = workerSearch()
      if (workerState) {
        return workerState.activeRect
      }
      const matches = deps.getSearchMatches()
      const activeIndex = deps.getSearchActiveIndex()
      if (activeIndex < 0 || activeIndex >= matches.length) {
        return null
      }
      // Delegate to the overlay's mapping so the rect matches the painted band.
      return atermSearchMatchRect(matches[activeIndex], {
        term,
        cellWidth,
        cellHeight,
        rows: getRows()
      })
    }
  }
}
