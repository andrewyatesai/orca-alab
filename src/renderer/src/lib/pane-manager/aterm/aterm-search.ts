import type { AtermTerminal } from './aterm_wasm.js'

/** A buffer search match in ABSOLUTE-row coordinates (the wasm index's native
 *  space). The renderer converts `line` to a display row at paint time via the
 *  engine's `search_display_origin` + `display_offset`, so a match stays correct
 *  as the viewport scrolls. */
export type AtermSearchMatch = {
  /** Absolute row of the match (0 = oldest retained line). */
  line: number
  /** Start column of the match (0-indexed). */
  startCol: number
  /** Match length in columns. */
  length: number
}

/** Primitives the search controller drives on the renderer: store the highlight
 *  set (active index in a stronger tone), bring a match into view, and redraw. */
export type AtermSearchRendererHooks = {
  setSearchHighlights: (matches: AtermSearchMatch[], activeIndex: number) => void
  scrollToMatch: (match: AtermSearchMatch) => void
  redraw: () => void
}

/** The find/next/prev/clear state machine plus the match count + active index,
 *  surfaced for the search UI. Owns no DOM/canvas — it calls renderer hooks. */
export type AtermSearchController = {
  /** Run a search; highlights all matches, selects + scrolls to the nearest one,
   *  and returns the result count. Empty query clears highlights → count 0.
   *  `isRegex` compiles the query as a regex (invalid pattern → 0 matches). */
  find: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  /** Advance to the next match (wraps); scrolls + restyles. No-op with 0 matches. */
  next: () => void
  /** Step to the previous match (wraps); scrolls + restyles. No-op with 0 matches. */
  prev: () => void
  /** Drop all highlights (search closed / query emptied). */
  clear: () => void
  /** Re-run the active query against the (now-changed) buffer so highlights track
   *  current content; no-op when no query is active. Preserves the active match
   *  position where possible. Called when process() feeds new output. */
  refresh: () => void
  /** Whether a non-empty query is currently active (highlights are live). */
  hasActiveQuery: () => boolean
  /** Total matches for the current query. */
  count: () => number
  /** 1-based index of the active match, or 0 when there are none. */
  activeIndex: () => number
}

// Decode the wasm `search` result — a flat [line, startCol, len, …] Uint32Array —
// into structured matches. The engine emits matches oldest-to-newest, which is
// the natural top-to-bottom order for next/prev navigation.
function decodeMatches(flat: Uint32Array): AtermSearchMatch[] {
  const matches: AtermSearchMatch[] = []
  // Each match is a [line, startCol, len] triplet. The bound must admit the
  // FINAL complete triplet: `i + 2 < length` drops it when it ends at the last
  // index, so require the whole triplet to fit (`i + 3 <= length`).
  for (let i = 0; i + 3 <= flat.length; i += 3) {
    matches.push({ line: flat[i], startCol: flat[i + 1], length: flat[i + 2] })
  }
  return matches
}

/** Build the aterm search controller over a terminal + renderer hooks. The
 *  controller is the single owner of search state so the renderer stays a dumb
 *  painter and the UI/keyboard layer talks to one find/next/prev/clear surface. */
export function createAtermSearchController(
  term: AtermTerminal,
  hooks: AtermSearchRendererHooks
): AtermSearchController {
  let matches: AtermSearchMatch[] = []
  let active = -1
  // Last query + case flag so a content change (process()) can re-run the search
  // and keep highlights pinned to the right cells. Empty query == no active search.
  let query = ''
  let caseSensitiveQuery = false
  let isRegexQuery = false

  const apply = (): void => {
    hooks.setSearchHighlights(matches, active)
    if (active >= 0 && active < matches.length) {
      hooks.scrollToMatch(matches[active])
    }
    hooks.redraw()
  }

  const find = (nextQuery: string, caseSensitive: boolean, isRegex: boolean): number => {
    if (!nextQuery) {
      clear()
      return 0
    }
    query = nextQuery
    caseSensitiveQuery = caseSensitive
    isRegexQuery = isRegex
    matches = decodeMatches(term.search(nextQuery, caseSensitive, isRegex))
    // Select the LAST match (newest / closest to the live bottom) so the first
    // find jumps near where output is, matching xterm's findNext-from-bottom feel.
    active = matches.length > 0 ? matches.length - 1 : -1
    apply()
    return matches.length
  }

  const step = (delta: number): void => {
    if (matches.length === 0) {
      return
    }
    active = (active + delta + matches.length) % matches.length
    apply()
  }

  const clear = (): void => {
    matches = []
    active = -1
    query = ''
    caseSensitiveQuery = false
    isRegexQuery = false
    hooks.setSearchHighlights([], -1)
    hooks.redraw()
  }

  // Re-run the active query against the changed buffer so highlights stay on the
  // correct cells after new output. Re-highlight only (no scroll) to avoid
  // yanking the viewport on every output frame; preserve the active position by
  // index, clamped to the new match count.
  const refresh = (): void => {
    if (!query) {
      return
    }
    matches = decodeMatches(term.search(query, caseSensitiveQuery, isRegexQuery))
    active = matches.length === 0 ? -1 : Math.min(Math.max(active, 0), matches.length - 1)
    hooks.setSearchHighlights(matches, active)
    hooks.redraw()
  }

  return {
    find,
    next: () => step(1),
    prev: () => step(-1),
    clear,
    refresh,
    hasActiveQuery: () => query.length > 0,
    count: () => matches.length,
    activeIndex: () => (active >= 0 ? active + 1 : 0)
  }
}
