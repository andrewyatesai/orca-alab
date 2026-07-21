import type { AtermWorkerPaneCommand } from './aterm-render-worker-protocol'

// The worker paints the predictive-echo ghosts on the main-thread stacked overlay from
// the STATE's predictOverlay cells, so the controller's own overlay read is inert here.
const EMPTY_PREDICT_CELLS = new Uint32Array(0)

/** The worker-backed term's predictive-echo surface. The engine predictor is off-thread,
 *  so each seam posts a command; predict_overlay is inert (the worker paints the ghost on
 *  the stacked overlay) and predict_next_deadline_ms returns the STATE-reflected value the
 *  controller arms its glitch timer from. Shaped to AtermTerminal's predict_* so the
 *  controller's capability probe passes and it drives this exactly like the in-process
 *  engine — the whole reason the worker path is no longer inert. */
export function createWorkerPredictFacade(
  post: (cmd: AtermWorkerPaneCommand) => void,
  getDeadlineMs: () => number | undefined
): {
  set_predictive_echo: (mode: string) => void
  predict_char: (ch: string) => boolean
  predict_backspace: () => boolean
  predict_line_submit: () => void
  predict_reconcile: () => void
  predict_overlay: () => Uint32Array
  predict_next_deadline_ms: () => number | undefined
  predict_reset: () => void
} {
  return {
    set_predictive_echo: (mode) =>
      post({
        type: 'predictSetMode',
        mode: mode as 'off' | 'adaptive' | 'always'
      }),
    predict_char: (ch) => {
      post({ type: 'predictChar', ch })
      return true
    },
    predict_backspace: () => {
      post({ type: 'predictBackspace' })
      return true
    },
    predict_line_submit: () => post({ type: 'predictSubmit' }),
    // The worker reconciles inside its own 'process' handler (where the freshly-applied
    // grid is) — a host-pump post-process reconcile would race the async parse, so this
    // seam is a no-op on the worker path.
    predict_reconcile: () => undefined,
    predict_overlay: () => EMPTY_PREDICT_CELLS,
    predict_next_deadline_ms: () => getDeadlineMs(),
    predict_reset: () => post({ type: 'predictReset' })
  }
}
