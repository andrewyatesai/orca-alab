import type { AtermTerminal } from './aterm_wasm.js'

/** The predictive-echo (mosh-style speculative typing) display mode. Default
 *  `'adaptive'`: show a guess only once the app has proven it line-echoes on the
 *  current line — invisible on fast-local + non-line-echoing apps (password
 *  prompts, TUIs), instant on high-latency SSH shells. */
export type AtermPredictionEchoMode = 'adaptive' | 'always' | 'off'

/** The engine's predictive-echo seam. The in-process CPU (`AtermTerminal`) and GPU
 *  (`AtermGpuTerminal`) engines export all of these; the worker-backed term facade
 *  does NOT (the engine lives off-thread), so the controller probes for them and
 *  degrades to inert no-ops there — predictions simply don't show, never crash. */
type AtermPredictEngine = Pick<
  AtermTerminal,
  | 'set_predictive_echo'
  | 'predict_char'
  | 'predict_backspace'
  | 'predict_line_submit'
  | 'predict_reconcile'
  | 'predict_overlay'
  | 'predict_next_deadline_ms'
  | 'predict_reset'
>

const EMPTY_OVERLAY = new Uint32Array(0)

export type AtermPredictionEcho = {
  /** A printable char the host just wrote to the PTY → track a speculative ghost. */
  noteChar: (ch: string) => void
  /** Backspace → cancel our own trailing guess (real erases are the app's echo). */
  noteBackspace: () => void
  /** A plain Enter (submit) → end the confirmation epoch (password-prompt safety). */
  noteSubmit: () => void
  /** Call right after the engine `process()`es a PTY chunk: reconcile guesses vs
   *  the real grid (confirmed ones retire, divergence/no-echo flushes). */
  reconcile: () => void
  /** The ghost cells to paint THIS frame (`[row, col, codepoint]` triples). Reads
   *  the engine overlay (which runs its expiry self-heal) then re-arms the deadline
   *  from the post-heal state, so a stale ghost can never pin the timer. */
  overlayCells: () => Uint32Array
  /** Apply the display mode (default `'adaptive'`). `'off'` disarms the deadline. */
  setMode: (mode: AtermPredictionEchoMode) => void
  /** Coordinate space changed (pane swap) — drop guesses AND disarm the deadline. */
  reset: () => void
  /** Tear down: MUST clear the pending deadline timer (no stranded 100%-CPU wake). */
  dispose: () => void
}

/** Drive the engine's predictive echo from the host input/process/paint seams and
 *  own the ONE glitch-expiry timer. The stranded-deadline 100%-CPU lesson is the
 *  hard invariant here: every disable path (`setMode('off')`, `reset`, `dispose`)
 *  clears the timer, and the timer is only ever re-armed AFTER the engine's expiry
 *  self-heal has run (in `overlayCells`), never off a permanently-past deadline. */
export function createAtermPredictionEcho(deps: {
  /** The pane's engine. Typed loosely so the worker facade (no predict methods at
   *  runtime) is a legal, inert input rather than a type error. */
  term: Partial<AtermPredictEngine>
  /** Present the ghost immediately (the interactive fast path) — the whole point
   *  is to paint the echo ~1 RTT before the PTY round-trips it back. */
  requestPaint: () => void
  isDisposed: () => boolean
}): AtermPredictionEcho {
  const { term, requestPaint, isDisposed } = deps
  // Runtime capability probe: absent on the worker path (engine is off-thread).
  const engine: AtermPredictEngine | null =
    typeof term.set_predictive_echo === 'function' &&
    typeof term.predict_char === 'function' &&
    typeof term.predict_overlay === 'function' &&
    typeof term.predict_next_deadline_ms === 'function'
      ? (term as AtermPredictEngine)
      : null

  let mode: AtermPredictionEchoMode = 'off'
  let deadlineTimer: ReturnType<typeof setTimeout> | null = null

  const clearDeadline = (): void => {
    if (deadlineTimer !== null) {
      clearTimeout(deadlineTimer)
      deadlineTimer = null
    }
  }

  // Arm the ONE glitch-expiry timer from the engine's CURRENT deadline. Always
  // clears the prior timer first, so only one is ever live. Callers must invoke
  // this only after any expiry self-heal has run (input registers a fresh future
  // guess; overlayCells re-arms post-`predict_overlay`), so the read is never a
  // permanently-past 0 that would busy-loop.
  const armDeadline = (): void => {
    clearDeadline()
    if (!engine || isDisposed() || mode === 'off') {
      return
    }
    const ms = engine.predict_next_deadline_ms()
    if (ms === undefined) {
      return
    }
    deadlineTimer = setTimeout(
      () => {
        deadlineTimer = null
        if (isDisposed()) {
          return
        }
        // Repaint: the paint reads overlayCells() → predict_overlay() runs the
        // expiry flush (erasing the stale ghost) AND re-arms for any later guess.
        requestPaint()
      },
      Math.max(0, ms)
    )
  }

  const enabled = (): boolean => engine !== null && mode !== 'off'

  return {
    noteChar: (ch) => {
      if (!engine || !enabled()) {
        return
      }
      engine.predict_char(ch)
      armDeadline()
      requestPaint()
    },
    noteBackspace: () => {
      if (!engine || !enabled()) {
        return
      }
      engine.predict_backspace()
      armDeadline()
      requestPaint()
    },
    noteSubmit: () => {
      if (!engine || !enabled()) {
        return
      }
      engine.predict_line_submit()
      // Submit flushes pending guesses, so this disarms (next_deadline → none).
      armDeadline()
      requestPaint()
    },
    reconcile: () => {
      if (!engine || !enabled()) {
        return
      }
      engine.predict_reconcile()
    },
    overlayCells: () => {
      if (!engine || mode === 'off') {
        return EMPTY_OVERLAY
      }
      // predict_overlay runs the expiry self-heal first; re-arm from the fresh
      // (post-heal) deadline so an expired ghost can never keep the timer pinned.
      const cells = engine.predict_overlay()
      armDeadline()
      return cells
    },
    setMode: (next) => {
      mode = next
      if (!engine) {
        return
      }
      engine.set_predictive_echo(next)
      // set_predictive_echo('off') fully resets the engine predictor; the host
      // timer MUST follow it down or it outlives the disable (the 100%-CPU trap).
      if (next === 'off') {
        clearDeadline()
      } else {
        armDeadline()
      }
    },
    reset: () => {
      clearDeadline()
      engine?.predict_reset()
    },
    dispose: () => {
      clearDeadline()
    }
  }
}
