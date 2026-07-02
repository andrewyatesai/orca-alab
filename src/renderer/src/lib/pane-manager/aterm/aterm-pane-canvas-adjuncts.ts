import { createAtermSearchOverlayCanvas } from './aterm-search-overlay-canvas'
import { createAtermScrollbarOverlay } from './aterm-scrollbar-overlay'
import { createAtermA11yMirror } from './aterm-a11y-mirror'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
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
  getHoveredLinkSpan: () => AtermHoveredLinkSpan | null
  getFgColor: () => number
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
        getFgColor: deps.getFgColor
      })
    : null

  // Overlay scrollbar: scrollback position feedback + thumb-drag navigation
  // (the canvas gives deep scrollback no visible scroll affordance of its own).
  const scrollbarOverlay = createAtermScrollbarOverlay(canvas, {
    term,
    getRows,
    redraw: scheduleDraw,
    isDisposed
  })

  const a11yMirror = createAtermA11yMirror({
    liveRegion: deps.liveRegion,
    term,
    getRows,
    isAltScreen: () => term.is_alt_screen,
    isDisposed
  })

  return { searchOverlay, scrollbarOverlay, a11yMirror }
}
