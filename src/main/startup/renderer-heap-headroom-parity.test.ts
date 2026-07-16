import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { computeRendererHeapCeilingMb } from './renderer-heap-headroom'

// Differential parity certificate (E1 unit): this TS sizing and the Rust
// `orca-renderer-heap::renderer_heap_ceiling_mb` core run the SAME shared corpus and
// must agree on the ceiling (or null) for every RAM total + override. The corpus
// override token maps to the env string the REAL production function parses, so
// this exercises the whole TS path (parser + RAM tiers), not a shortcut. Paired
// with the ay proofs (that crate's proofs/ay/rh_*.smt2) this is the full E1 pair;
// it also pins that JS Number and Rust f64 agree bit-for-bit through the division /
// *0.4 / floor / clamp.
describe('renderer heap-ceiling shared parity corpus', () => {
  it('matches the Rust orca-renderer-heap corpus for every RAM tier + override', () => {
    const corpusPath = fileURLToPath(
      new URL('../../../rust/crates/orca-renderer-heap/parity-corpus.txt', import.meta.url)
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
      const [bytesTok, overrideTok] = lhs.trim().split(/\s+/)
      const bytes = Number(bytesTok)
      // Map the corpus override token to the env string the real parser resolves:
      // none -> no override; disable -> an explicit opt-out; N -> that number.
      const envOverride =
        overrideTok === 'none' ? undefined : overrideTok === 'disable' ? 'off' : overrideTok
      const want = rhs.trim()
      const got = computeRendererHeapCeilingMb(bytes, envOverride)
      const gotStr = got === null ? 'null' : String(got)
      expect(gotStr, `${bytesTok} ${overrideTok} (line ${lineNo})`).toBe(want)
      checked++
    }
    expect(checked).toBeGreaterThanOrEqual(12)
  })
})
