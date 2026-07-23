import { describe, expect, it } from 'vitest'
import {
  daemonDepthCutoffRow,
  mapDaemonDepthMatchesToLive,
  mapDaemonRowToLive,
  type DaemonToLiveRowMapping
} from './federated-daemon-row-space'
import type { FederatedMatch } from './federated-search-model'

function match(absRow: number): FederatedMatch {
  return { absRow, col: 0, len: 3, snippet: null }
}

// The daemon retains 1000 rows (0..999); the live engine's newest absolute row
// is 5200. So daemon row 999 ≡ live 5200, and the live window's oldest retained
// row is 5000 (only the newest 200 rows are in the renderer).
const mapping: DaemonToLiveRowMapping = { daemonRowCount: 1000, liveNewestAbsRow: 5200 }
const OLDEST_LIVE = 5000

describe('mapDaemonRowToLive', () => {
  it('aligns the daemon newest row to the live newest row and walks back', () => {
    expect(mapDaemonRowToLive(999, mapping)).toBe(5200) // newest ≡ newest
    expect(mapDaemonRowToLive(998, mapping)).toBe(5199)
    expect(mapDaemonRowToLive(0, mapping)).toBe(4201) // oldest daemon row
  })
})

describe('daemonDepthCutoffRow (off-by-N boundary)', () => {
  it('places the cutoff so daemonRow=cutoff maps EXACTLY to the live oldest row', () => {
    const cutoff = daemonDepthCutoffRow(mapping, OLDEST_LIVE)
    // cutoff = 5000 - 5200 + 999 = 799.
    expect(cutoff).toBe(799)
    // The boundary: cutoff maps to the live oldest (excluded); cutoff-1 to just
    // below it (included). This is the off-by-N contract.
    expect(mapDaemonRowToLive(cutoff, mapping)).toBe(OLDEST_LIVE)
    expect(mapDaemonRowToLive(cutoff - 1, mapping)).toBe(OLDEST_LIVE - 1)
  })
})

describe('mapDaemonDepthMatchesToLive', () => {
  it('remaps daemon rows to live space and drops rows at/above the live oldest (off-by-N)', () => {
    const cutoff = daemonDepthCutoffRow(mapping, OLDEST_LIVE) // 799
    const matches = [
      match(cutoff - 1), // 798 → live 4999 (below oldest: KEPT)
      match(cutoff), //     799 → live 5000 (== oldest: DROPPED, live-covered)
      match(cutoff + 5), // 804 → live 5005 (in the live window: DROPPED)
      match(0) //             0 → live 4201 (deep history: KEPT)
    ]
    const mapped = mapDaemonDepthMatchesToLive(matches, mapping, OLDEST_LIVE)
    expect(mapped.map((m) => m.absRow)).toEqual([4999, 4201])
  })

  it('preserves col/len/snippet, translating only the row coordinate', () => {
    const mapped = mapDaemonDepthMatchesToLive(
      [{ absRow: 998, col: 7, len: 4, snippet: 'x[abcd]y' }],
      mapping,
      6000 // high oldest so the row is kept
    )
    expect(mapped).toEqual([{ absRow: 5199, col: 7, len: 4, snippet: 'x[abcd]y' }])
  })

  it('handles a daemon with EXACTLY the live window (no deeper history): all rows map in-window and drop', () => {
    // daemonRowCount == live window depth, newest aligned → every daemon row is
    // at/above the live oldest, so nothing is a genuine depth extension.
    const flush: DaemonToLiveRowMapping = { daemonRowCount: 200, liveNewestAbsRow: 5199 }
    const mapped = mapDaemonDepthMatchesToLive([match(0), match(199)], flush, 5000)
    expect(mapped).toEqual([])
  })
})
