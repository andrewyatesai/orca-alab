import type { AtermTerminal } from './aterm_wasm'
import { computeGrid } from './aterm-grid-size'
import { attachAtermDprTracker } from './aterm-dpr-tracker'

/** Mutable cell metrics shared with the input-handler deps (selection/scroll/link
 *  read .dpr/.cellWidth/.cellHeight live), updated in place on a DPR change so the
 *  handlers stay correct without rebinding. */
export type AtermMetrics = { dpr: number; cellWidth: number; cellHeight: number }

type GridReflowConfig = {
  term: Pick<AtermTerminal, 'set_px' | 'set_line_height' | 'cell_width' | 'cell_height'>
  container: HTMLElement
  /** Shared metrics object the input handlers captured; mutated in place here. */
  metrics: AtermMetrics
  /** Base cell font size in CSS px (the user's terminalFontSize). Read live so a
   *  size change re-rasterizes without a pane rebuild; defaults are handled upstream. */
  getFontPx: () => number
  /** Cell line-height multiplier (the user's terminalLineHeight, ~1–3). Read live so
   *  a change re-derives the cell-box height (not the glyph px) without a rebuild. */
  getLineHeight: () => number
  /** Read the current grid (cols/rows). */
  getGrid: () => { cols: number; rows: number }
  /** Commit a new grid: resize the strategy + report it to the PTY. */
  setGrid: (cols: number, rows: number) => void
  isDisposed: () => boolean
  /** Push the new metrics into the input-handler deps + event reporting. */
  syncDependents: () => void
  scheduleDraw: () => void
}

export type AtermGridReflow = {
  dispose: () => void
  /** Cheap per-frame guard for the draw loop: a number compare + a settings read,
   *  no layout. Re-rasterizes only when the live dpr or font size diverged from
   *  what the engine was last built at — closing the dpr-settle gap the
   *  matchMedia/ResizeObserver paths can miss (a pure dpr change leaves the CSS
   *  box unchanged, so the observer never fires). Returns true iff it actually
   *  reconciled, so the caller can avoid presenting in the same turn it
   *  reconfigured the GPU swapchain. */
  reconcileIfNeeded: () => boolean
}

/** Own the grid's DPI/size reflow: a ResizeObserver on the container plus a DPR
 *  tracker, both re-rasterizing the engine and recomputing cols/rows. Extracted
 *  from the wiring to keep it focused. Returns a disposer for both observers. */
export function attachAtermGridReflow(config: GridReflowConfig): AtermGridReflow {
  const { term, container, metrics, getFontPx, getLineHeight, getGrid, setGrid, isDisposed } =
    config
  const { syncDependents, scheduleDraw } = config

  // The engine px the glyph atlas was last rasterized at (= round(fontPx * dpr)) and
  // the line-height the cell box was last derived at. Tracked so the draw-loop guard
  // can detect a dpr-, font-size-, or line-height-driven mismatch with a cheap
  // compare (no layout read).
  let appliedPx = Math.round(getFontPx() * metrics.dpr)
  let appliedLineHeight = getLineHeight()

  // Re-rasterize at the live density + font size + line-height so cell metrics
  // rebuild; otherwise the grid (and glyph atlas) stays sized for the construction
  // values — wrong columns / a blurry upscale / a stale row height — when the window
  // settles to a different dpr, or the user changes the font size or line-height.
  const reapplyMetrics = (nextDpr: number): void => {
    metrics.dpr = nextDpr
    appliedPx = Math.round(getFontPx() * nextDpr)
    term.set_px(appliedPx)
    appliedLineHeight = getLineHeight()
    term.set_line_height(appliedLineHeight)
    metrics.cellWidth = term.cell_width
    metrics.cellHeight = term.cell_height
    syncDependents()
  }

  const reflowGrid = (): void => {
    if (isDisposed()) {
      return
    }
    const liveDpr = window.devicePixelRatio || 1
    const desiredPx = Math.round(getFontPx() * liveDpr)
    const metricsChanged =
      liveDpr !== metrics.dpr || desiredPx !== appliedPx || getLineHeight() !== appliedLineHeight
    if (metricsChanged) {
      reapplyMetrics(liveDpr)
    }
    const next = computeGrid(container, metrics.dpr, metrics.cellWidth, metrics.cellHeight)
    const current = getGrid()
    // On a metrics change, commit even when cols/rows are unchanged: the engine's
    // framebuffer / GPU swapchain must be resized to the new cell size (set_px alone
    // does not reconfigure the WebGL2 surface), else the GPU path keeps the old
    // backing-store dimensions.
    if (!metricsChanged && next.cols === current.cols && next.rows === current.rows) {
      return
    }
    setGrid(next.cols, next.rows)
    scheduleDraw()
  }

  const reconcileIfNeeded = (): boolean => {
    if (isDisposed()) {
      return false
    }
    const liveDpr = window.devicePixelRatio || 1
    if (
      liveDpr === metrics.dpr &&
      Math.round(getFontPx() * liveDpr) === appliedPx &&
      getLineHeight() === appliedLineHeight
    ) {
      return false
    }
    reflowGrid()
    return true
  }

  const resizeObserver = new ResizeObserver(reflowGrid)
  resizeObserver.observe(container)

  const dprTracker = attachAtermDprTracker({
    getDpr: () => metrics.dpr,
    isDisposed,
    // reflowGrid reads the live devicePixelRatio itself (already updated by the time
    // matchMedia fires) and re-rasterizes + resizes when it diverges.
    onDprChange: () => {
      reflowGrid()
      scheduleDraw()
    }
  })

  return {
    dispose: () => {
      resizeObserver.disconnect()
      dprTracker.dispose()
    },
    reconcileIfNeeded
  }
}
