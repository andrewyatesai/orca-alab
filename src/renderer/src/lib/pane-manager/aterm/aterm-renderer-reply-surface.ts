import { e2eConfig } from '@/lib/e2e-config'
import { benchmarkAtermRender, type AtermRenderBenchmarkResult } from './aterm-render-benchmark'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermThemeColors } from './aterm-theme-colors'
import type { AtermTerminal } from './aterm_wasm.js'

/** The renderer-authoritative reply surface: pixel size (CSI 14t/16t), theme
 *  colors (OSC 10/11), and the e2e-only render benchmark. The daemon and the
 *  unopened xterm shim can't answer pixel/color queries, so the aterm canvas
 *  does. Extracted from the controller to keep that file under the line budget. */
export type AtermRendererReplySurface = {
  pixelSize: () => { width: number; height: number; cellWidth: number; cellHeight: number }
  themeColors: () => { fg: number; bg: number }
  benchmarkRender?: (cols: number, rows: number, frames: number) => AtermRenderBenchmarkResult
}

export function buildAtermRendererReplySurface(deps: {
  term: AtermTerminal
  /** Shared mutable pane metrics — read per query so CSI 14t/16t replies track
   *  DPI moves and live font/line-height changes instead of the attach-time cell. */
  metrics: AtermMetrics
  themeColors: AtermThemeColors
  getGrid: () => { cols: number; rows: number }
  scheduleDraw: () => void
}): AtermRendererReplySurface {
  const { term, metrics, themeColors, getGrid, scheduleDraw } = deps
  return {
    // term.width/height are the last-rendered framebuffer device px; before the
    // first render they're 0, so fall back to cell*grid for a startup-race query.
    // The frame includes the window-space effects chrome when on (worker facade
    // getters; in-process has none → 0) — CSI 14t must report the TEXT AREA, so
    // subtract it.
    pixelSize: () => {
      const { cols, rows } = getGrid()
      const chromeTerm = term as typeof term & { chrome_pad?: number; chrome_head?: number }
      const pad = chromeTerm.chrome_pad ?? 0
      const head = chromeTerm.chrome_head ?? 0
      return {
        width: term.width ? term.width - 2 * pad : metrics.cellWidth * cols,
        height: term.height ? term.height - 2 * pad - head : metrics.cellHeight * rows,
        cellWidth: metrics.cellWidth,
        cellHeight: metrics.cellHeight
      }
    },
    themeColors: () => ({ fg: themeColors.fg, bg: themeColors.bg }),
    // e2e-only perf seam (excluded from prod so it can't leak the engine handle).
    ...(e2eConfig.exposeStore
      ? {
          benchmarkRender: (benchCols: number, benchRows: number, frames: number) =>
            benchmarkAtermRender(term, getGrid(), benchCols, benchRows, frames, scheduleDraw)
        }
      : {})
  }
}
