// Predictive-echo (mosh-style speculative typing) command types, mirrored across the
// worker boundary. The engine's predictor lives in the worker, so the host posts each
// keystroke seam as a command; the worker runs the SAME engine predictor (all safety
// gates — password no-echo, alt-screen/TUI refusal, ASCII-only, wrap decline — stay
// engine-side) and reflects the ghost cells + glitch deadline back in STATE. Reconcile is
// NOT a command: the worker runs it right after `process` applies a chunk, against the
// grid it reconciles. Split out to keep the wire-contract file under the line budget
// (mirrors aterm-worker-rain-protocol / aterm-worker-spill-protocol).

export type AtermWorkerPredictSetMode = {
  type: 'predictSetMode'
  mode: 'off' | 'adaptive' | 'always'
}
/** A printable char the host just wrote to the PTY → track a speculative ghost. */
export type AtermWorkerPredictChar = { type: 'predictChar'; ch: string }
/** Backspace → cancel our own trailing guess (real erases are the app's echo). */
export type AtermWorkerPredictBackspace = { type: 'predictBackspace' }
/** A plain Enter (submit) → end the confirmation epoch (password-prompt safety). */
export type AtermWorkerPredictSubmit = { type: 'predictSubmit' }
/** Coordinate space changed (pane swap) — drop guesses (resize does this in-engine). */
export type AtermWorkerPredictReset = { type: 'predictReset' }

/** Every predict* command, for the extracted worker-side dispatcher. */
export type AtermWorkerPredictCommand =
  | AtermWorkerPredictSetMode
  | AtermWorkerPredictChar
  | AtermWorkerPredictBackspace
  | AtermWorkerPredictSubmit
  | AtermWorkerPredictReset
