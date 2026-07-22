// Owns a pane's search state: the live match list / active index the painter and
// controller read, the coalesced re-index flag the process pump sets, and the
// search controller + controller-facing search API built over them. Extracted
// from aterm-pane-wiring to keep that file under the line cap.

import { createAtermSearchController, type AtermSearchMatch } from './aterm-search'
import { buildAtermSearchApi, type AtermSearchApi } from './aterm-search-api'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermTerminal } from './aterm_wasm'

export type AtermPaneSearchState = {
  searchController: ReturnType<typeof createAtermSearchController>
  searchApi: AtermSearchApi
  getSearchMatches: () => AtermSearchMatch[]
  getSearchActiveIndex: () => number
  /** Mark that content changed while a query is active (coalesced per frame). */
  markSearchRefresh: () => void
  /** Consume the pending re-index flag (the painter's once-per-frame read). */
  takeSearchRefresh: () => boolean
}

export function createAtermPaneSearchState(deps: {
  term: AtermTerminal
  metrics: AtermMetrics
  isDisposed: () => boolean
  getRows: () => number
  scheduleDraw: () => void
}): AtermPaneSearchState {
  const { term, metrics, isDisposed, getRows, scheduleDraw } = deps
  let searchMatches: AtermSearchMatch[] = []
  let searchActiveIndex = -1
  let searchRefreshPending = false
  // In-process change feed: every highlight update (find/next/prev/refresh/clear)
  // funnels through setSearchHighlights, so the scrollbar marker strip and the
  // search UI get one notify seam on this path (the worker path pushes via STATE).
  const searchStateListeners = new Set<() => void>()

  const searchController = createAtermSearchController(term, {
    setSearchHighlights: (next, activeIndex) => {
      searchMatches = next
      searchActiveIndex = activeIndex
      searchStateListeners.forEach((listener) => listener())
    },
    scrollToMatch: (match) => {
      if (!isDisposed()) {
        term.scroll_search_line_into_view(match.line)
      }
    },
    redraw: scheduleDraw
  })

  const searchApi = buildAtermSearchApi({
    searchController,
    term,
    metrics,
    isDisposed,
    getRows,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex,
    onControllerSearchStateChange: (handler) => {
      searchStateListeners.add(handler)
      return () => searchStateListeners.delete(handler)
    }
  })

  return {
    searchController,
    searchApi,
    getSearchMatches: () => searchMatches,
    getSearchActiveIndex: () => searchActiveIndex,
    markSearchRefresh: () => {
      searchRefreshPending = true
    },
    takeSearchRefresh: () => {
      const pending = searchRefreshPending
      searchRefreshPending = false
      return pending
    }
  }
}
