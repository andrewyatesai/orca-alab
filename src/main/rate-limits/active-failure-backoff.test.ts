import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { activeFailureRefetchThrottleMs } from './active-failure-backoff'

// Differential parity certificate (E1 unit): this TS production sizing and the
// Rust `orca-provider-backoff::active_failure_refetch_throttle_ms` spec run the
// SAME shared corpus and must agree on the throttle for every failure streak.
// Divergence fails one side. Paired with the ay proofs (that crate's
// proofs/ay/bo_*.smt2), this is the full E1 pair — spec proved correct,
// implementations proved equivalent. The corpus encodes BASE=30000, MAX=900000
// (mirroring MIN_POLL_MS / DEFAULT_POLL_MS), so the test pins those exact values.
const BASE_MS = 30_000
const MAX_MS = 900_000

describe('active-failure refetch backoff shared parity corpus', () => {
  it('matches the Rust orca-provider-backoff corpus for every streak', () => {
    const corpusPath = fileURLToPath(
      new URL('../../../rust/crates/orca-provider-backoff/parity-corpus.txt', import.meta.url)
    )
    const corpus = readFileSync(corpusPath, 'utf8')
    let checked = 0
    let lineNo = 0
    for (const raw of corpus.split('\n')) {
      lineNo++
      const line = raw.trim()
      if (line === '' || line.startsWith('#')) {
        continue
      }
      const [lhs, rhs = ''] = line.split('=>')
      const streak = Number(lhs.trim())
      const want = Number(rhs.trim())
      expect(
        activeFailureRefetchThrottleMs(streak, BASE_MS, MAX_MS),
        `throttle at streak=${streak} (line ${lineNo})`
      ).toBe(want)
      checked++
    }
    // Guard against a silently-empty corpus.
    expect(checked).toBeGreaterThanOrEqual(8)
  })

  it('doubles from the base and saturates at the ceiling', () => {
    expect(activeFailureRefetchThrottleMs(0, BASE_MS, MAX_MS)).toBe(30_000)
    expect(activeFailureRefetchThrottleMs(1, BASE_MS, MAX_MS)).toBe(30_000)
    expect(activeFailureRefetchThrottleMs(2, BASE_MS, MAX_MS)).toBe(60_000)
    expect(activeFailureRefetchThrottleMs(5, BASE_MS, MAX_MS)).toBe(480_000)
    // streak 6: 30000 * 32 = 960000 -> capped.
    expect(activeFailureRefetchThrottleMs(6, BASE_MS, MAX_MS)).toBe(900_000)
    expect(activeFailureRefetchThrottleMs(8, BASE_MS, MAX_MS)).toBe(900_000)
  })
})
