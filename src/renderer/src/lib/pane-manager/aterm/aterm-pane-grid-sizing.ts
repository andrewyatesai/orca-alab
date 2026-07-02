// Owns a pane's grid dimensions: the initial container-derived size, the
// container/DPI reflow, and the explicit resize override (snapshot replay at
// source dims, mobile-fit hold) that the reflow honors until a fit clears it.
// Extracted from aterm-pane-wiring to keep it under the line cap.

import { computeGrid } from './aterm-grid-size'
import { attachAtermGridReflow, type AtermGridReflow, type AtermMetrics } from './aterm-grid-reflow'
import type { AtermTerminal } from './aterm_wasm'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermPaneResizeSink } from './aterm-pane-controller-types'

export type AtermPaneGridSizing = {
  /** Live grid (cols × rows) — the controller's gridSize and the PTY report. */
  grid: () => { cols: number; rows: number }
  /** Explicit grid resize (xterm resize semantics): pins an override the
   *  container ResizeObserver honors, so it can't undo a snapshot-replay or
   *  mobile-fit grid mid-hold. */
  resize: (cols: number, rows: number) => void
  /** Drop the explicit override and refit to the container (safeFit's aterm
   *  path after a snapshot replay / mobile release). */
  fitToContainer: () => void
  /** The attached container/DPI reflow (dispose / reconcile / force). */
  reflow: AtermGridReflow
}

type GridSizingDeps = {
  term: Pick<AtermTerminal, 'set_px' | 'set_line_height' | 'cell_width' | 'cell_height'>
  container: HTMLElement
  metrics: AtermMetrics
  strategy: AtermDrawStrategy
  getFontPx: () => number
  getLineHeight: () => number
  resizeSink: AtermPaneResizeSink
  syncDependents: () => void
  scheduleDraw: () => void
  isDisposed: () => boolean
}

export function createAtermPaneGridSizing(deps: GridSizingDeps): AtermPaneGridSizing {
  const { container, metrics, strategy, resizeSink, scheduleDraw, isDisposed } = deps
  let { cols, rows } = computeGrid(container, metrics.dpr, metrics.cellWidth, metrics.cellHeight)
  // An explicit facade resize pins the grid here until fitToContainer clears it.
  let explicitGrid: { cols: number; rows: number } | null = null

  // One committer for the observer reflow AND the explicit resize, so the
  // engine grid, the strategy surface, and the PTY report never diverge.
  const commit = (nextCols: number, nextRows: number): void => {
    cols = nextCols
    rows = nextRows
    strategy.resize(rows, cols)
    resizeSink(cols, rows)
  }

  // Size the real grid up front so the canvas matches from frame 1 (the wiring
  // reports it to the PTY once the input handlers are attached).
  strategy.resize(rows, cols)

  const reflow = attachAtermGridReflow({
    term: deps.term,
    container,
    metrics,
    getFontPx: deps.getFontPx,
    getLineHeight: deps.getLineHeight,
    getGrid: () => ({ cols, rows }),
    // Worker path (onMetricsChange present): cell metrics land a frame after set_px, so
    // defer the grid commit to the worker's metrics push instead of the stale snapshot.
    // In-process set_px is synchronous (no hook) -> commit immediately (unchanged).
    asyncMetrics: strategy.onMetricsChange !== undefined,
    getGridOverride: () => explicitGrid,
    setGrid: commit,
    isDisposed,
    syncDependents: deps.syncDependents,
    scheduleDraw
  })

  return {
    grid: () => ({ cols, rows }),
    resize: (nextCols, nextRows) => {
      if (isDisposed() || !Number.isFinite(nextCols) || !Number.isFinite(nextRows)) {
        return
      }
      const safeCols = Math.max(1, Math.floor(nextCols))
      const safeRows = Math.max(1, Math.floor(nextRows))
      explicitGrid = { cols: safeCols, rows: safeRows }
      if (safeCols !== cols || safeRows !== rows) {
        commit(safeCols, safeRows)
        scheduleDraw()
      }
    },
    fitToContainer: () => {
      if (isDisposed()) {
        return
      }
      explicitGrid = null
      reflow.reflow()
    },
    reflow
  }
}
