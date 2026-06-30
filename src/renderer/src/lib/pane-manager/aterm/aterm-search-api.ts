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
  const workerSearch = (): ReturnType<
    NonNullable<AtermWorkerAsyncFacade['searchStateSnapshot']>
  > | null =>
    (term as typeof term & Partial<AtermWorkerAsyncFacade>).searchStateSnapshot?.() ?? null
  return {
    findMatches: (query, caseSensitive, isRegex) =>
      isDisposed() ? 0 : searchController.find(query, caseSensitive, isRegex),
    findNextMatch: () => searchController.next(),
    findPreviousMatch: () => searchController.prev(),
    clearSearch: () => searchController.clear(),
    searchMatchCount: () => workerSearch()?.count ?? searchController.count(),
    searchActiveMatchIndex: () => workerSearch()?.activeIndex ?? searchController.activeIndex(),
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
