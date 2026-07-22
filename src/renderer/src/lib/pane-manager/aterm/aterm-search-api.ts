import { atermSearchMatchRect } from './aterm-search-overlay'
import {
  createSearchMarkerModelCache,
  type AtermSearchMarkerModel
} from './aterm-search-marker-model'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermMetrics } from './aterm-grid-reflow'
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
  /** True while posted find results haven't landed yet (worker path only — the
   *  in-process search is synchronous). The label shows "~N, searching…" then. */
  searchIsPending: () => boolean
  /** Scrollbar match-marker model: bounded track fractions over the retained
   *  buffer, from the FULL sorted match list on either path. */
  searchMarkerModel: () => AtermSearchMarkerModel
  /** Subscribe to search-state changes that land after the call returns: the worker
   *  pushes count/active-index a frame after a posted find/next/prev; in-process the
   *  controller notifies on every highlight update (find/next/prev/refresh/clear).
   *  Returns a disposer. */
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
  /** In-process change feed (fires on every controller highlight update) — the
   *  onSearchStateChange fallback where no worker facade exists. */
  onControllerSearchStateChange: (handler: () => void) => () => void
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
  // In-process marker derivation (the worker path ships its model in the snapshot).
  const markerCache = createSearchMarkerModelCache()
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
    // In-process find is synchronous, so results are never pending there.
    searchIsPending: () => workerSearch()?.pending ?? false,
    searchMarkerModel: () => {
      const workerState = workerSearch()
      if (workerState) {
        return workerState.markers
      }
      return markerCache(
        deps.getSearchMatches(),
        deps.getSearchActiveIndex(),
        // Oldest retained absolute row: ring eviction keeps absolute rows growing.
        term.search_display_origin - term.base_y,
        term.base_y + getRows()
      )
    },
    onSearchStateChange: (handler) =>
      facade.onSearchStateChange?.(handler) ?? deps.onControllerSearchStateChange(handler),
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
