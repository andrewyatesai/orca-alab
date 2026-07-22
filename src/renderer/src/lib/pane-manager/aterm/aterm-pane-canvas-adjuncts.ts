import { createAtermSearchOverlayCanvas } from './aterm-search-overlay-canvas'
import { createAtermScrollbarOverlay } from './aterm-scrollbar-overlay'
import { createAtermA11yMirror } from './aterm-a11y-mirror'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import type { AtermSearchMarkerModel } from './aterm-search-marker-model'
import type { TerminalScrollIntentTarget } from '../terminal-scroll-intent-types'
import type { AtermTerminal } from './aterm_wasm.js'

type PaneCanvasAdjunctDeps = {
  canvas: HTMLCanvasElement
  /** Off-screen ARIA live region the a11y mirror writes visible rows into. */
  liveRegion: HTMLElement
  term: AtermTerminal
  metrics: AtermMetrics
  /** GPU path only: the grid canvas is webgl2-owned, so search highlights need
   *  the stacked 2d overlay; the CPU drawer paints them in-frame instead. */
  needsSearchOverlay: boolean
  getRows: () => number
  /** For the a11y mirror's rewrap detection (a cols change renumbers absolute lines). */
  getCols: () => number
  getHoveredLinkSpan: () => AtermHoveredLinkSpan | null
  getFgColor: () => number
  /** Predictive-echo ghost cells for the GPU-path stacked overlay (empty on the CPU
   *  path, which paints them in-frame, and when off / not predict-capable). */
  getPredictionCells: () => Uint32Array
  /** The pane's scroll-intent target (facade) for the scrollbar's thumb-drag path. */
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null
  /** Search marker model for the scrollbar strip (both paths via the search API). */
  getSearchMarkers: () => AtermSearchMarkerModel
  /** Search-state change feed driving the strip repaint (worker STATE push or the
   *  in-process controller notify); returns a disposer. */
  onSearchStateChange: (handler: () => void) => () => void
  scheduleDraw: () => void
  isDisposed: () => boolean
}

/** Mount the DOM the wiring stacks around the grid canvas: the (GPU-only)
 *  search-highlight overlay canvas, the overlay scrollbar, and the off-screen
 *  ARIA mirror screen readers get terminal output from (the canvas is opaque
 *  to them). Extracted from aterm-pane-wiring to keep it under the line budget. */
export function mountAtermPaneCanvasAdjuncts(deps: PaneCanvasAdjunctDeps): {
  searchOverlay: ReturnType<typeof createAtermSearchOverlayCanvas> | null
  scrollbarOverlay: ReturnType<typeof createAtermScrollbarOverlay>
  a11yMirror: ReturnType<typeof createAtermA11yMirror>
} {
  const { canvas, term, metrics, getRows, scheduleDraw, isDisposed } = deps

  const searchOverlay = deps.needsSearchOverlay
    ? createAtermSearchOverlayCanvas(canvas, {
        term,
        metrics,
        getRows,
        getHoveredLinkSpan: deps.getHoveredLinkSpan,
        getFgColor: deps.getFgColor,
        getPredictionCells: deps.getPredictionCells
      })
    : null

  // Overlay scrollbar: scrollback position feedback + thumb-drag navigation
  // (the canvas gives deep scrollback no visible scroll affordance of its own).
  const scrollbar = createAtermScrollbarOverlay(canvas, {
    term,
    getRows,
    redraw: scheduleDraw,
    isDisposed,
    getScrollIntentTarget: deps.getScrollIntentTarget,
    getSearchMarkers: deps.getSearchMarkers
  })
  // Marker strip repaints on search-state changes, not the thumb's rAF loop — it
  // must stay visible while the thumb is faded out.
  const unsubscribeSearchMarkers = deps.onSearchStateChange(scrollbar.refreshSearchMarkers)
  const scrollbarOverlay: typeof scrollbar = {
    refreshSearchMarkers: scrollbar.refreshSearchMarkers,
    dispose: () => {
      unsubscribeSearchMarkers()
      scrollbar.dispose()
    }
  }

  const a11yMirror = createAtermA11yMirror({
    liveRegion: deps.liveRegion,
    term,
    getRows,
    getCols: deps.getCols,
    isAltScreen: () => term.is_alt_screen,
    isDisposed
  })

  return { searchOverlay, scrollbarOverlay, a11yMirror }
}
