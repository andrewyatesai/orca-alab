// TS dispatch for the keep-tail parity module: drives the live
// src/main/daemon/daemon-stream-keep-tail-drop.ts reference against the Rust port
// (orca-flow-control::keep_tail). Both sides close over the budget/floor/ceiling
// constants, so each function takes only the droppable-session count — the
// compared pair differs only in language, not configuration.

import {
  backgroundSessionDropCapChars,
  backgroundSessionKeepTailChars
} from '../../../src/main/daemon/daemon-stream-keep-tail-drop'

export function dispatch(fn: string, input: unknown): unknown {
  const { droppableSessions } = input as { droppableSessions: number }
  // A session count is non-negative; clamp a negative/non-integer input to 0 so
  // it matches the Rust adapter's read (both then feed Math.max(1, n)).
  const n = Number.isFinite(droppableSessions) ? Math.max(0, Math.trunc(droppableSessions)) : 0
  switch (fn) {
    case 'backgroundSessionKeepTailChars':
      return backgroundSessionKeepTailChars(n)
    case 'backgroundSessionDropCapChars':
      return backgroundSessionDropCapChars(n)
    default:
      return { __parity_error__: `unknown function ${fn}` }
  }
}
