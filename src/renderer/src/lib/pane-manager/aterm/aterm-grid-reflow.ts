import type { AtermTerminal } from './aterm_wasm'
import { computeGrid } from './aterm-grid-size'
import { attachAtermDprTracker } from './aterm-dpr-tracker'
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'

/** Mutable cell metrics shared with the input-handler deps (selection/scroll/link
 *  read .dpr/.cellWidth/.cellHeight live), updated in place on a DPR change so the
 *  handlers stay correct without rebinding. */
export type AtermMetrics = { dpr: number; cellWidth: number; cellHeight: number }

type GridReflowConfig = {
  term: Pick<AtermTerminal, 'set_px' | 'cell_width' | 'cell_height'>
  container: HTMLElement
  /** Shared metrics object the input handlers captured; mutated in place here. */
  metrics: AtermMetrics
  /** Read the current grid (cols/rows). */
  getGrid: () => { cols: number; rows: number }
  /** Commit a new grid: resize the strategy + report it to the PTY. */
  setGrid: (cols: number, rows: number) => void
  isDisposed: () => boolean
  /** Push the new metrics into the input-handler deps + event reporting. */
  syncDependents: () => void
  scheduleDraw: () => void
}

/** Own the grid's DPI/size reflow: a ResizeObserver on the container plus a DPR
 *  tracker, both re-rasterizing the engine and recomputing cols/rows. Extracted
 *  from the wiring to keep it focused. Returns a disposer for both observers. */
export function attachAtermGridReflow(config: GridReflowConfig): { dispose: () => void } {
  const { term, container, metrics, getGrid, setGrid, isDisposed, syncDependents, scheduleDraw } =
    config

  // Re-rasterize at a new density's cell font px so cell metrics rebuild; otherwise
  // the grid stays sized for the construction dpr (wrong columns) when the window
  // settles to a different dpr than it was born at.
  const applyDpr = (nextDpr: number): void => {
    metrics.dpr = nextDpr
    term.set_px(Math.round(ATERM_RENDERER_FONT_PX * nextDpr))
    metrics.cellWidth = term.cell_width
    metrics.cellHeight = term.cell_height
    syncDependents()
  }

  const reflowGrid = (): void => {
    if (isDisposed()) {
      return
    }
    // The matchMedia resolution listener can miss the window's initial dpr settle
    // (esp. headless); the ResizeObserver fires on layout changes, so reconcile here.
    const liveDpr = window.devicePixelRatio || 1
    if (liveDpr !== metrics.dpr) {
      applyDpr(liveDpr)
    }
    const next = computeGrid(container, metrics.dpr, metrics.cellWidth, metrics.cellHeight)
    const current = getGrid()
    if (next.cols === current.cols && next.rows === current.rows) {
      return
    }
    setGrid(next.cols, next.rows)
    scheduleDraw()
  }

  const resizeObserver = new ResizeObserver(reflowGrid)
  resizeObserver.observe(container)

  const dprTracker = attachAtermDprTracker({
    getDpr: () => metrics.dpr,
    isDisposed,
    onDprChange: (nextDpr) => {
      applyDpr(nextDpr)
      reflowGrid()
      scheduleDraw()
    }
  })

  return {
    dispose: () => {
      resizeObserver.disconnect()
      dprTracker.dispose()
    }
  }
}
