// Fork-owned perf-marks family (ws-proof evidence lane): User Timing marks and
// measures that make the terminal's interactive latency observable in DevTools
// and drivable by the local perf gauntlet (tools/benchmarks/perf-proof-run.mjs).
// Marks are local-only instrumentation — nothing here is emitted as remote
// telemetry, so no payload/privacy schema is involved.

export const TERMINAL_PERF_MARKS = {
  /** Stamped when a terminal keydown is captured. Production emit site is the
   *  wave-2 one-liner in the key-encoding path (coordinated file); until then
   *  the perf lane stamps it from the harness on real synthetic keystrokes. */
  keydown: 'orca:terminal:keydown',
  /** presentNow entered: a just-fed interactive echo asked for an eager paint. */
  interactivePresentStart: 'orca:terminal:interactive-present:start',
  /** presentNow finished: the frame holding the echo was submitted. */
  framePresented: 'orca:terminal:frame-presented'
} as const

export const TERMINAL_PERF_MEASURES = {
  /** Render half of one keystroke: echo fed → frame presented. */
  interactivePresent: 'orca:terminal:interactive-present',
  /** Felt keystroke latency: keydown → frame presented (needs a keydown mark). */
  keydownToFramePresented: 'orca:terminal:keydown-to-frame-presented'
} as const

/** A keydown mark older than this cannot have caused the presented frame
 *  (echo round-trips are milliseconds; past this it's a stale/unconsumed mark). */
export const TERMINAL_KEYDOWN_ATTRIBUTION_WINDOW_MS = 1_000

/** Stamp the start of the interactive presentNow fast path. Re-marks in place
 *  (clear-then-mark) so the User Timing buffer holds one live entry per name
 *  instead of growing for the lifetime of a long session. */
export function markTerminalInteractivePresentStart(): void {
  const perf = globalThis.performance
  perf.clearMarks(TERMINAL_PERF_MARKS.interactivePresentStart)
  perf.mark(TERMINAL_PERF_MARKS.interactivePresentStart)
}

/** Stamp the presented frame and emit the family's measures: always the
 *  echo-fed→presented render half, plus keydown→frame-presented when a fresh
 *  keydown mark exists. Each keydown attributes to at most one frame. */
export function measureTerminalFramePresented(
  attributionWindowMs: number = TERMINAL_KEYDOWN_ATTRIBUTION_WINDOW_MS
): void {
  const perf = globalThis.performance
  perf.clearMarks(TERMINAL_PERF_MARKS.framePresented)
  perf.mark(TERMINAL_PERF_MARKS.framePresented)
  perf.clearMeasures(TERMINAL_PERF_MEASURES.interactivePresent)
  try {
    perf.measure(
      TERMINAL_PERF_MEASURES.interactivePresent,
      TERMINAL_PERF_MARKS.interactivePresentStart,
      TERMINAL_PERF_MARKS.framePresented
    )
  } catch {
    // No start mark yet (present fired before any marked fast-path entry).
  }
  const keydown = perf.getEntriesByName(TERMINAL_PERF_MARKS.keydown, 'mark').at(-1)
  if (!keydown) {
    return
  }
  const presented = perf.getEntriesByName(TERMINAL_PERF_MARKS.framePresented, 'mark').at(-1)
  if (!presented) {
    return
  }
  const elapsed = presented.startTime - keydown.startTime
  if (elapsed < 0 || elapsed > attributionWindowMs) {
    return
  }
  perf.clearMeasures(TERMINAL_PERF_MEASURES.keydownToFramePresented)
  perf.measure(
    TERMINAL_PERF_MEASURES.keydownToFramePresented,
    TERMINAL_PERF_MARKS.keydown,
    TERMINAL_PERF_MARKS.framePresented
  )
  // Consumed: the next presented frame must not re-attribute this keystroke.
  perf.clearMarks(TERMINAL_PERF_MARKS.keydown)
}
