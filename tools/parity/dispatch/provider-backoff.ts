// TS dispatch for the provider-backoff parity module: drives the live
// `src/main/rate-limits/active-failure-backoff.ts` reference so the harness
// compares it against the Rust port (orca-provider-backoff).
//
// The Rust core owns base/ceiling as constants (ACTIVE_FAILURE_REFETCH_MS = 30s,
// MAX_ACTIVE_FAILURE_REFETCH_MS = 15min); the TS twin takes them as params. We pin
// the same two values here so the compared pair differs only in language, not in
// configuration — a real behavioural-parity check, not a re-tuned reimplementation.

import { activeFailureRefetchThrottleMs } from '../../../src/main/rate-limits/active-failure-backoff'

const ACTIVE_FAILURE_REFETCH_MS = 30 * 1000
const MAX_ACTIVE_FAILURE_REFETCH_MS = 15 * 60 * 1000

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'activeFailureRefetchThrottleMs': {
      const { streak } = input as { streak: number }
      // Clamp a negative/absent streak to 0, matching the Rust adapter's read of
      // an out-of-range value (both then collapse to the base wait).
      const safeStreak = Number.isFinite(streak) ? Math.max(0, Math.trunc(streak)) : 0
      return activeFailureRefetchThrottleMs(
        safeStreak,
        ACTIVE_FAILURE_REFETCH_MS,
        MAX_ACTIVE_FAILURE_REFETCH_MS
      )
    }
    default:
      return { __parity_error__: `unknown function ${fn}` }
  }
}
