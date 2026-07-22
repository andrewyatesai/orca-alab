import { atermSearchMatchRect } from './aterm-search-overlay'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermTerminal } from './aterm_wasm.js'
import type {
  AtermWorkerAsyncFacade,
  AtermWorkerSearchFindResult
} from './aterm-worker-query-channel'

/** The find/next/prev/clear/count/index/rect surface the controller exposes. */
export type AtermSearchApi = {
  findMatches: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  /** Awaitable find for the search UI's pending state: resolves the post-find
   *  `{count, activeIndex}`, or null when a newer find superseded this one (its
   *  result must be discarded — the newer request owns the pending state). Resolves
   *  synchronously in-process; a worker round-trip on the worker path. */
  findMatchesAsync: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean
  ) => Promise<AtermWorkerSearchFindResult | null>
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
  /** True while the worker's cost gate serves results older than the buffer content
   *  (streaming; see aterm-worker-search) — the UI's stale indicator. Always false
   *  in-process, where refresh is immediate. */
  searchResultsStale: () => boolean
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
  /** Shared mutable pane metrics — read at call time so match rects stay correct
   *  across DPI moves and live font/line-height changes. */
  metrics: AtermMetrics
  isDisposed: () => boolean
  getRows: () => number
  getSearchMatches: () => AtermSearchMatch[]
  getSearchActiveIndex: () => number
}

/** Build the controller's search method surface. Extracted so the renderer stays
 *  under the line budget; state (matches/active index/rows) is read via getters
 *  because it changes as the viewport scrolls and content updates. */
export function buildAtermSearchApi(deps: AtermSearchApiDeps): AtermSearchApi {
  const { searchController, term, metrics, isDisposed, getRows } = deps
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
    findMatchesAsync: (query, caseSensitive, isRegex) => {
      if (isDisposed()) {
        return Promise.resolve({ count: 0, activeIndex: 0 })
      }
      // Worker path: the channel correlates the result to THIS request and cancels a
      // superseded one instantly. Skips the (empty there) main-thread controller.
      if (facade.searchFindAsync) {
        return facade.searchFindAsync(query, caseSensitive, isRegex)
      }
      const count = searchController.find(query, caseSensitive, isRegex)
      return Promise.resolve({ count, activeIndex: searchController.activeIndex() })
    },
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
    searchResultsStale: () => workerSearch()?.stale ?? false,
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
        cellWidth: metrics.cellWidth,
        cellHeight: metrics.cellHeight,
        rows: getRows()
      })
    }
  }
}
