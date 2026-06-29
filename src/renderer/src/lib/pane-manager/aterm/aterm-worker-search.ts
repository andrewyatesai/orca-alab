// The worker-side search state machine: the single engine lives in the worker, so the
// find/next/prev/clear loop + match index run here (the main-thread search API posts
// commands and reads count/active/rect from the snapshot). Extracted from the worker
// terminal to keep that file under the line cap.

import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermWorkerState } from './aterm-render-worker-protocol'

/** A match in absolute-row coords (the engine's native index space). */
type WorkerMatch = { line: number; startCol: number; length: number }

function decodeMatches(flat: Uint32Array): WorkerMatch[] {
  const matches: WorkerMatch[] = []
  for (let i = 0; i + 3 <= flat.length; i += 3) {
    matches.push({ line: flat[i], startCol: flat[i + 1], length: flat[i + 2] })
  }
  return matches
}

export type WorkerSearch = {
  find: (query: string, caseSensitive: boolean, isRegex: boolean) => void
  next: () => void
  prev: () => void
  clear: () => void
  /** Re-run the active query against changed output, preserving the active index. */
  refresh: () => void
  count: () => number
  /** 1-based active match index, or 0 when there are none. */
  activeIndex: () => number
  /** Device-pixel rect of the active match if it's on screen, else null. */
  activeRect: () => AtermWorkerState['searchActiveRect']
  /** Device-pixel rects of ALL on-screen matches (for the main-thread overlay), each
   *  flagged active so the overlay can paint the active one stronger. */
  visibleRects: () => AtermWorkerState['searchMatchRects']
}

export function createWorkerSearch(handle: EngineHandle, getRows: () => number): WorkerSearch {
  const e = handle.engine
  let matches: WorkerMatch[] = []
  let active = -1
  let query = ''
  let caseSensitive = false
  let isRegex = false

  const run = (): void => {
    matches = query ? decodeMatches(handle.search(query, caseSensitive, isRegex)) : []
  }

  return {
    find: (q, cs, regex) => {
      query = q
      caseSensitive = cs
      isRegex = regex
      if (!q) {
        matches = []
        active = -1
        return
      }
      run()
      // Select the LAST match (closest to the live bottom), matching the main path.
      active = matches.length > 0 ? matches.length - 1 : -1
      if (active >= 0) {
        e.scroll_search_line_into_view(matches[active].line)
      }
    },
    next: () => {
      if (matches.length > 0) {
        active = (active + 1 + matches.length) % matches.length
        e.scroll_search_line_into_view(matches[active].line)
      }
    },
    prev: () => {
      if (matches.length > 0) {
        active = (active - 1 + matches.length) % matches.length
        e.scroll_search_line_into_view(matches[active].line)
      }
    },
    clear: () => {
      matches = []
      active = -1
      query = ''
      caseSensitive = false
      isRegex = false
    },
    refresh: () => {
      if (!query) {
        return
      }
      run()
      active = matches.length === 0 ? -1 : Math.min(Math.max(active, 0), matches.length - 1)
    },
    count: () => matches.length,
    activeIndex: () => (active >= 0 ? active + 1 : 0),
    activeRect: () => (active >= 0 ? rectFor(matches[active]) : null),
    visibleRects: () => {
      const rects: NonNullable<AtermWorkerState['searchMatchRects']> = []
      for (let i = 0; i < matches.length; i++) {
        const rect = rectFor(matches[i])
        if (rect) {
          rects.push({ ...rect, active: i === active })
        }
      }
      return rects
    }
  }

  // Absolute match line → on-screen device-pixel rect, or null when scrolled off
  // (the SAME mapping paintAtermSearchHighlights uses: search_display_origin +
  // display_offset).
  function rectFor(m: WorkerMatch): { x: number; y: number; width: number; height: number } | null {
    const displayRow = m.line - e.search_display_origin + e.display_offset
    if (displayRow < 0 || displayRow >= getRows()) {
      return null
    }
    return {
      x: m.startCol * e.cell_width,
      y: displayRow * e.cell_height,
      width: m.length * e.cell_width,
      height: e.cell_height
    }
  }
}
