import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { clampToSafeSplitIndex, nextSafeSplitIndex } from './daemon-stream-data-split'

// Differential parity certificate (E1 unit): these TS surrogate-safe split
// primitives and the Rust `orca-stream-split` core run the SAME shared corpus and
// must agree on every clamp/next index. Divergence fails one side. Paired with the
// ay proofs (rust/crates/orca-stream-split/proofs/ay/{cs,ns}_*.smt2), this is the
// full E1 pair. Units are hex UTF-16 code units built into a string via
// fromCharCode, exactly matching the Rust &[u16].
const buildString = (unitsTok: string): string =>
  unitsTok === '_'
    ? ''
    : String.fromCharCode(...unitsTok.split(',').map((h) => Number.parseInt(h, 16)))

describe('surrogate-safe split-index shared parity corpus', () => {
  it('matches the Rust orca-stream-split clamp/next for every case', () => {
    const corpusPath = fileURLToPath(
      new URL('../../../rust/crates/orca-stream-split/parity-corpus.txt', import.meta.url)
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
      const tok = line.split(/\s+/)
      const op = tok[0]
      const value = buildString(tok[1])
      if (op === 'clamp') {
        const start = Number(tok[2])
        const end = Number(tok[3])
        // tok[4] === '=>'
        const want = Number(tok[5])
        expect(clampToSafeSplitIndex(value, start, end), `clamp (line ${lineNo})`).toBe(want)
      } else if (op === 'next') {
        const start = Number(tok[2])
        // tok[3] === '=>'
        const want = Number(tok[4])
        expect(nextSafeSplitIndex(value, start), `next (line ${lineNo})`).toBe(want)
      } else {
        throw new Error(`line ${lineNo}: unknown op ${op}`)
      }
      checked++
    }
    expect(checked).toBeGreaterThanOrEqual(8)
  })
})
