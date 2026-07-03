import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermTerminal } from './aterm_wasm.js'

/** Wire the single-engine worker strategy's PUSHED channels into the live pane:
 *  forward the engine's query replies (DA/DSR/CPR/colour) to the PTY, and re-reflow the
 *  grid when the worker re-rasterizes at a new cell size (set_px/line-height apply a
 *  frame after the snapshot). Both are no-ops for the in-process CPU/GPU strategies,
 *  which leave onReply/onMetricsChange unset (their replies pull-drain + metrics read
 *  synchronously). */
export function wireWorkerStrategyHooks(deps: {
  strategy: AtermDrawStrategy
  term: Pick<AtermTerminal, 'cell_width' | 'cell_height'>
  metrics: { cellWidth: number; cellHeight: number }
  inputSink: (data: string) => void
  forceReflow: () => void
  /** Re-read + re-emit the window title (OSC 0/2). On the worker path the title arrives
   *  in a STATE message after the posted process(), so the pump's per-chunk
   *  emitTitleIfChanged would lag a command; fire it on the side-channel push instead. */
  emitTitleIfChanged: () => void
  /** Re-derive the effects colour from the live OSC 12 cursor colour. Snapshot-backed
   *  on the worker path, so it fires on the same side-channel push as the title. */
  syncCursorColor: () => void
  isDisposed: () => boolean
}): void {
  const { strategy, term, metrics, inputSink, forceReflow, emitTitleIfChanged, isDisposed } = deps
  // Replies are PUSHED (not pull-drained): a CPR/DA query that produces no further
  // output would otherwise deadlock waiting for the next drain.
  strategy.onReply?.((data) => {
    if (!isDisposed()) {
      inputSink(data)
    }
  })
  strategy.onMetricsChange?.(() => {
    if (isDisposed()) {
      return
    }
    metrics.cellWidth = term.cell_width
    metrics.cellHeight = term.cell_height
    forceReflow()
  })
  // A title set on the final pre-idle chunk arrives in a later STATE message; re-emit
  // it the moment that push lands so the tab/window title isn't a command behind.
  strategy.onSideChannel?.(() => {
    if (!isDisposed()) {
      emitTitleIfChanged()
      deps.syncCursorColor()
    }
  })
}
