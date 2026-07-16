import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { GpuCrashFallbackTracker } from './gpu-crash-fallback-decision'

// Differential parity certificate (E1 unit): this TS tracker and the Rust
// `orca-crash-recovery::gpu_fallback` core replay the SAME crash trace and must
// agree on every step. Divergence fails one side. Paired with the ay proofs
// (rust/crates/orca-crash-recovery/proofs/ay/gf_*.smt2), this is the full E1 pair.
// The corpus header fixes the config at window=30, threshold=3.
describe('gpu software-fallback latch shared parity corpus', () => {
  it('matches the Rust orca-crash-recovery trace step for step', () => {
    const corpusPath = fileURLToPath(
      new URL(
        '../../../rust/crates/orca-crash-recovery/gpu-fallback-parity-corpus.txt',
        import.meta.url
      )
    )
    const corpus = readFileSync(corpusPath, 'utf8')
    const tracker = new GpuCrashFallbackTracker({ windowMs: 30, threshold: 3 })
    let checked = 0
    let lineNo = 0
    for (const raw of corpus.split('\n')) {
      lineNo++
      const line = raw.trim()
      if (line === '' || line.startsWith('#')) {
        continue
      }
      // Format: `crash <msSinceLaunch> => <shouldEngage> <crashesInWindow>`
      const rest = line.replace(/^crash\s+/, '')
      const [lhs, want = ''] = rest.split('=>')
      const ms = Number(lhs.trim())
      const d = tracker.recordGpuCrash(ms)
      const got = `${d.shouldEngageFallback ? 1 : 0} ${d.crashesInWindow}`
      expect(got, `crash ${ms} (line ${lineNo})`).toBe(want.trim())
      checked++
    }
    expect(checked).toBeGreaterThanOrEqual(6)
  })
})
