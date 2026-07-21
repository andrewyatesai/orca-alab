import type { WorkerEngine } from './aterm-worker-engine-build'
import type { AtermWorkerPredictCommand } from './aterm-worker-predict-protocol'

// No ghost cells: reused so an off/idle predictor allocates nothing per frame.
const EMPTY_PREDICT_OVERLAY = new Uint32Array(0)

/** The worker-side driver for the shared engine predictor (mosh-style predictive echo).
 *  The host posts each keystroke seam as a command → dispatchAtermWorkerPredictCommand
 *  calls these; buildState reflects overlay()/deadlineMs() back so the main thread paints
 *  the ghost on its stacked overlay + arms the ONE glitch timer. Every safety gate
 *  (ASCII-only, wrap decline, no-echo/alt-screen, epoch) lives engine-side, so calling the
 *  SAME engine here holds them identically to the in-process path. */
export type WorkerPredictor = {
  setMode: (mode: 'off' | 'adaptive' | 'always') => void
  char: (ch: string) => void
  backspace: () => void
  submit: () => void
  reset: () => void
  /** After `process` applies a chunk: retire confirmed ghosts / flush divergence/no-echo. */
  reconcile: () => void
  /** Ghost cells for the overlay (`[row, col, codepoint]` triples), empty while off/idle. */
  overlay: () => Uint32Array
  /** Ms to the oldest guess's glitch flush, or null. Read AFTER overlay()'s self-heal. */
  deadlineMs: () => number | null
}

/** An inert predictor: every seam is a no-op, no ghost, no deadline. Used when the
 *  engine lacks the predict_* exports so a mismatched/older wasm blob degrades to
 *  no-prediction rather than crashing the shared worker on the first keystroke —
 *  the same graceful-degradation the in-process controller's capability probe gives. */
function inertWorkerPredictor(): WorkerPredictor {
  return {
    setMode: () => {},
    char: () => {},
    backspace: () => {},
    submit: () => {},
    reset: () => {},
    reconcile: () => {},
    overlay: () => EMPTY_PREDICT_OVERLAY,
    deadlineMs: () => null
  }
}

export function createWorkerPredictor(e: WorkerEngine): WorkerPredictor {
  // Capability guard: the predict_* exports are typed as present (the pinned blob
  // has them), but a runtime blob/type mismatch must NOT crash the whole shared
  // worker — degrade to inert, matching the in-process probe (aterm-prediction-echo).
  const probe = e as unknown as {
    predict_char?: unknown
    predict_overlay?: unknown
    set_predictive_echo?: unknown
    predict_next_deadline_ms?: unknown
  }
  if (
    typeof probe.predict_char !== 'function' ||
    typeof probe.predict_overlay !== 'function' ||
    typeof probe.set_predictive_echo !== 'function' ||
    typeof probe.predict_next_deadline_ms !== 'function'
  ) {
    return inertWorkerPredictor()
  }
  // Enabled (mode != off) gates the per-frame overlay/deadline reflect + per-chunk
  // reconcile so an off predictor crosses the wasm boundary zero times.
  let enabled = false
  return {
    setMode: (mode) => {
      enabled = mode !== 'off'
      e.set_predictive_echo(mode)
    },
    char: (ch) => void e.predict_char(ch),
    backspace: () => void e.predict_backspace(),
    submit: () => e.predict_line_submit(),
    reset: () => e.predict_reset(),
    reconcile: () => {
      if (enabled) {
        e.predict_reconcile()
      }
    },
    // predict_overlay runs the expiry self-heal, then the deadline is read AFTER it — so a
    // stale ghost is flushed here and never pins the reflected deadline (the native
    // stranded-deadline invariant, honored across the seam).
    overlay: () => (enabled ? e.predict_overlay() : EMPTY_PREDICT_OVERLAY),
    deadlineMs: () => (enabled ? (e.predict_next_deadline_ms() ?? null) : null)
  }
}

/** Route a predict* command to the pane's predictor + schedule a repaint. The host follows
 *  each with a 'draw' (its interactive presentNow) that paints synchronously + posts a
 *  STATE reflecting the fresh ghost + deadline; scheduleDraw is the coalesced backstop so
 *  the ghost still lands if the host coalesces its eager draw away this frame. */
export function dispatchAtermWorkerPredictCommand(
  predictor: WorkerPredictor | undefined,
  scheduleDraw: () => void,
  msg: AtermWorkerPredictCommand
): void {
  // No engine yet (still building) → no-op, same as the other pane commands.
  if (!predictor) {
    return
  }
  switch (msg.type) {
    case 'predictSetMode':
      predictor.setMode(msg.mode)
      break
    case 'predictChar':
      predictor.char(msg.ch)
      break
    case 'predictBackspace':
      predictor.backspace()
      break
    case 'predictSubmit':
      predictor.submit()
      break
    case 'predictReset':
      predictor.reset()
      break
  }
  scheduleDraw()
}
