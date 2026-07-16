import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { planSessionHistoryGc } from './daemon-session-history-gc-plan'

// Differential parity certificate (E1 unit): this TS GC planner and the Rust
// `orca-session-gc::plan_session_history_gc` core run the SAME shared corpus and
// must agree on {expire, evictForSize, remainingBytes} for every scanned store +
// liveness/budget context. Divergence fails one side. Paired with the ay proofs
// (rust/crates/orca-session-gc/proofs/ay/*.smt2), this is the full E1 pair.
// Fixed thresholds match the corpus header.
const THRESHOLDS = { minDirAgeMs: 10, endedRetentionMs: 100, unrestoredRetentionMs: 1000 }

const parseNames = (tok: string): string[] => (tok === '-' ? [] : tok.split(','))

describe('daemon session-history GC planner shared parity corpus', () => {
  it('matches the Rust orca-session-gc plan for every case', () => {
    const corpusPath = fileURLToPath(
      new URL('../../../rust/crates/orca-session-gc/parity-corpus.txt', import.meta.url)
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
      const [input, output] = line.split('=>')
      const [config, dirsStr] = input.split('|')
      const [nowTok, maxTok, luTok, liveTok] = config.trim().split(/\s+/)
      const livenessUnknown = luTok === '1'
      const liveDirNames = livenessUnknown
        ? null
        : new Set(liveTok === '-' ? [] : liveTok.split(','))
      const dirs = dirsStr
        .trim()
        .split(';')
        .map((s) => s.trim())
        .filter((s) => s !== '')
        .map((spec) => {
          const [name, bytes, last, ended] = spec.split(':')
          return {
            name,
            totalBytes: Number(bytes),
            lastActivityMs: Number(last),
            isEnded: ended === '1'
          }
        })
      const [expireTok, evictTok, remainingTok] = output.trim().split(/\s+/)
      const plan = planSessionHistoryGc({
        dirs,
        now: Number(nowTok),
        maxTotalBytes: Number(maxTok),
        livenessUnknown,
        liveDirNames,
        thresholds: THRESHOLDS
      })
      expect(plan.expire, `expire (line ${lineNo})`).toEqual(parseNames(expireTok))
      expect(plan.evictForSize, `evict (line ${lineNo})`).toEqual(parseNames(evictTok))
      expect(plan.remainingBytes, `remaining (line ${lineNo})`).toBe(Number(remainingTok))
      checked++
    }
    expect(checked).toBeGreaterThanOrEqual(8)
  })
})
