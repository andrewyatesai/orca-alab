import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  backgroundSessionDropCapChars,
  backgroundSessionKeepTailChars
} from './daemon-stream-keep-tail-drop'

// The cross-language parity certificate (2nd E1 unit): this TS production sizing
// and the Rust `orca-flow-control::keep_tail` spec run the SAME shared corpus and
// must agree on keep-tail and drop-cap for every session count. Divergence fails
// one side. Paired with the ay proofs (proofs/ay/kt_*.smt2), this is the full E1
// pair — spec proved correct, implementations proved equivalent.
describe('background-session keep-tail shared parity corpus', () => {
  it('matches the Rust orca-flow-control::keep_tail corpus for every n', () => {
    const corpusPath = fileURLToPath(
      new URL('../../../rust/crates/orca-flow-control/keep-tail-parity-corpus.txt', import.meta.url)
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
      const n = Number(lhs.trim())
      const [keepTail, dropCap] = rhs.trim().split(/\s+/).map(Number)
      expect(backgroundSessionKeepTailChars(n), `keep-tail at n=${n} (line ${lineNo})`).toBe(
        keepTail
      )
      expect(backgroundSessionDropCapChars(n), `drop-cap at n=${n} (line ${lineNo})`).toBe(dropCap)
      checked++
    }
    // Guard against a silently-empty corpus.
    expect(checked).toBeGreaterThanOrEqual(10)
  })
})
