import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { RendererRecoveryCircuitBreaker } from './renderer-recovery-circuit-breaker'

// Differential parity certificate (E1 unit): this TS circuit breaker and the Rust
// `orca-crash-recovery::renderer_recovery` core replay the SAME operation trace and
// must agree on every step. Divergence fails one side. Paired with the ay proofs
// (rust/crates/orca-crash-recovery/proofs/ay/rr_*.smt2), this is the full E1 pair.
// The corpus header fixes the config at window=100, max=3.
describe('renderer-recovery circuit breaker shared parity corpus', () => {
  it('matches the Rust orca-crash-recovery trace step for step', () => {
    const corpusPath = fileURLToPath(
      new URL(
        '../../../rust/crates/orca-crash-recovery/renderer-recovery-parity-corpus.txt',
        import.meta.url
      )
    )
    const corpus = readFileSync(corpusPath, 'utf8')
    const breaker = new RendererRecoveryCircuitBreaker({ windowMs: 100, maxRecoveries: 3 })
    let checked = 0
    let lineNo = 0
    for (const raw of corpus.split('\n')) {
      lineNo++
      const line = raw.trim()
      if (line === '' || line.startsWith('#')) {
        continue
      }
      const [op, ...restTokens] = line.split(/\s+/)
      const rest = restTokens.join(' ')
      if (op === 'reset') {
        breaker.reset()
        checked++
        continue
      }
      const [lhs, want = ''] = rest.split('=>')
      const now = Number(lhs.trim())
      if (op === 'attempt') {
        const d = breaker.registerRecoveryAttempt(now)
        const got = `${d.allowed ? 1 : 0} ${d.recentRecoveryCount}`
        expect(got, `attempt ${now} (line ${lineNo})`).toBe(want.trim())
      } else if (op === 'count') {
        expect(String(breaker.recentRecoveryCount(now)), `count ${now} (line ${lineNo})`).toBe(
          want.trim()
        )
      } else {
        throw new Error(`line ${lineNo}: unknown op ${op}`)
      }
      checked++
    }
    expect(checked).toBeGreaterThanOrEqual(10)
  })
})
