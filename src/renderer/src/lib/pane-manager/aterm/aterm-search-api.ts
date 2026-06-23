import { atermSearchMatchRect } from './aterm-search-overlay'
import type { AtermSearchController, AtermSearchMatch } from './aterm-search'
import type { AtermTerminal } from './aterm_wasm.js'

/** The find/next/prev/clear/count/index/rect surface the controller exposes. */
export type AtermSearchApi = {
  findMatches: (query: string, caseSensitive: boolean) => number
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
  return {
    findMatches: (query, caseSensitive) =>
      isDisposed() ? 0 : searchController.find(query, caseSensitive),
    findNextMatch: () => searchController.next(),
    findPreviousMatch: () => searchController.prev(),
    clearSearch: () => searchController.clear(),
    searchMatchCount: () => searchController.count(),
    searchActiveMatchIndex: () => searchController.activeIndex(),
    searchActiveMatchRect: () => {
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
