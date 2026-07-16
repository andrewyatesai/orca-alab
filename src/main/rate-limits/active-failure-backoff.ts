// Pure capped-exponential-backoff for the active-window refetch throttle. Lifted
// out of the RateLimitsService so the doubling+clamp decision is unit-testable and
// machine-checkable in isolation — it is the TS half of the `orca-provider-backoff`
// E1 pair (proven equivalent to the Rust core by
// `rust/crates/orca-provider-backoff/parity-corpus.txt`, proven correct by that
// crate's `proofs/ay/bo_*.smt2`). Base/ceiling stay owned by the caller (they mirror
// MIN_POLL_MS / DEFAULT_POLL_MS) and are passed in, so the tie can't silently drift.

/**
 * Refetch throttle (ms) for a provider with `streak` consecutive failures:
 * `min(baseMs * 2 ** max(0, streak - 1), maxMs)`. Streak 0 and 1 both wait `baseMs`;
 * each further failure doubles the wait until it saturates at `maxMs`.
 */
export function activeFailureRefetchThrottleMs(
  streak: number,
  baseMs: number,
  maxMs: number
): number {
  return Math.min(baseMs * 2 ** Math.max(0, streak - 1), maxMs)
}
