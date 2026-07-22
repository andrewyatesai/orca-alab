import type { WorkerEngine } from './aterm-worker-engine-build'

/** The worker's sub-row pixel-scroll ingress (positive = older, the scroll_lines
 *  sign convention). Prefers the engine's scroll_px — it banks the fractional
 *  residual and presents it as a pixel band shift; a pinned engine artifact
 *  WITHOUT the export must not crash the SHARED worker (predictor precedent),
 *  so the fallback banks the sub-row rest here and flips whole lines instead. */
export function createWorkerScrollPx(
  e: Pick<WorkerEngine, 'cell_height' | 'scroll_lines' | 'scroll_px'>
): (deltaPx: number) => void {
  // Sub-row rows banked ONLY by the fallback (the engine banks its own residual).
  let fallbackRows = 0
  return (deltaPx) => {
    if (typeof e.scroll_px === 'function') {
      e.scroll_px(deltaPx)
      return
    }
    const cellH = e.cell_height
    if (!(cellH > 0)) {
      return
    }
    const total = fallbackRows + deltaPx / cellH
    const whole = Math.trunc(total)
    fallbackRows = total - whole
    if (whole !== 0) {
      e.scroll_lines(whole)
    }
  }
}
