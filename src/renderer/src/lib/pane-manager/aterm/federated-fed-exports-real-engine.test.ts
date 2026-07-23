// REAL committed-wasm coverage for the fed E-1 exports (pin 1cb05f33): the
// consumers below run against a live `AtermTerminal`, NOT a mock. This is the
// exact gap the Wave-6 closure gate failed on — the earlier suites exercised
// stub engines, so a pin missing the export (or shaped differently) passed green.
// If a future pin drops or reshapes search_summary / row_range_json /
// search_index_release, these fail loudly instead of degrading silently.

import { readFileSync } from 'node:fs'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { initSync, AtermTerminal } from './aterm_wasm.js'
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'
import { createAtermSearchSummaryReader } from './aterm-worker-search-summary'
import { createAtermRowRangeReader } from './aterm-worker-row-range-export'
import { detectEngineSearchIndexRelease } from './aterm-engine-search-index-release'
import { createFederatedLinearScanReader } from './aterm-federated-linear-scan-reader'
import { runFederatedLinearScan, type FederatedLinearScanResult } from './aterm-federated-linear-scan'

const ATERM_DIR = new URL('./', import.meta.url)
const FONT_URL = new URL('../../../assets/fonts/jetbrains-mono.ttf', import.meta.url)
let fontBytes: Uint8Array

beforeAll(() => {
  // Real engine, headless: initSync + on-disk bytes (the node analogue of
  // load-aterm.ts's browser fetch), the same pattern the other real-wasm suites use.
  initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
  fontBytes = new Uint8Array(readFileSync(FONT_URL))
})

const openTerms: AtermTerminal[] = []
afterEach(() => {
  for (const t of openTerms.splice(0)) t.free()
})

function open(rows: number, cols: number): AtermTerminal {
  const t = new AtermTerminal(rows, cols, fontBytes, ATERM_RENDERER_FONT_PX, 0xffffff, 0x000000, 0xffffff, 0x334455)
  openTerms.push(t)
  return t
}

/** Feed `count` lines; every third carries a "-HIT" suffix after the needle. */
function feedNeedles(t: AtermTerminal, count: number): void {
  for (let i = 0; i < count; i += 1) {
    t.process_str(`alpha line ${i} needle${i % 3 === 0 ? '-HIT' : ''}\r\n`)
  }
}

function runScanSync(
  opts: Parameters<typeof runFederatedLinearScan>[0]
): FederatedLinearScanResult | null {
  let out: FederatedLinearScanResult | null = null
  let settled = false
  runFederatedLinearScan({
    ...opts,
    yieldSlice: (next) => next(),
    onDone: (r) => {
      out = r
      settled = true
    }
  })
  expect(settled).toBe(true)
  return out
}

describe('fed E-1 search_summary consumer — real engine', () => {
  it('returns REAL span-marked snippets + honest total/incomplete', () => {
    const t = open(10, 40)
    feedNeedles(t, 50)
    const reader = createAtermSearchSummaryReader(t)
    const summary = reader.read('needle', false, false, 5)
    expect(summary).not.toBeNull()
    // Capped to max_matches, but total counts EVERY match (uncapped honesty).
    expect(summary!.matches.length).toBe(5)
    expect(summary!.total).toBe(50)
    expect(summary!.incomplete).toBe(false)
    for (const m of summary!.matches) {
      // Real line text, not a synthetic stub.
      expect(m.snippet).toContain('needle')
      expect(m.snippet).toContain(`line ${m.absRow}`)
      // The span points at the needle within the real snippet.
      expect(m.snippet.slice(m.col, m.col + m.len)).toBe('needle')
    }
  })

  it('snippetsByRow maps engine absolute rows to their REAL scrollback text', () => {
    const t = open(10, 40)
    feedNeedles(t, 30)
    const byRow = createAtermSearchSummaryReader(t).snippetsByRow('needle', false, false, 50)
    expect(byRow).not.toBeNull()
    // Scrollback rows (absRow 0..2) are reachable via the index the summary reuses.
    expect(byRow!.get(0)).toBe('alpha line 0 needle-HIT')
    expect(byRow!.get(1)).toBe('alpha line 1 needle')
  })

  it('empty query is a definitive zero-match summary (mirrors search silence)', () => {
    const t = open(10, 40)
    feedNeedles(t, 20)
    const summary = createAtermSearchSummaryReader(t).read('', false, false, 5)
    expect(summary).toEqual({ matches: [], total: 0, incomplete: false })
  })
})

describe('4B mirror row_range_json consumer — real engine', () => {
  it('reads the REAL viewport text in one crossing; all-narrow rows omit widths', () => {
    const t = open(4, 40)
    for (let i = 0; i < 20; i += 1) t.process_str(`row-${i}-narrow\r\n`)
    const rows = createAtermRowRangeReader(t).read(0, 4, 40)
    expect(rows).not.toBeNull()
    expect(rows!.length).toBe(4)
    // Viewport bottom shows the freshest lines; text is the real grid content.
    expect(rows!.some((r) => r.text.startsWith('row-'))).toBe(true)
    // ASCII fast path: engine omits `widths` so the host reuses its cached all-'1'.
    for (const r of rows!) {
      expect(r.wrapped).toBe(false)
      expect(r.widths).toBeUndefined()
    }
  })

  it('emits a per-column widths string with a wide-lead digit for CJK rows', () => {
    const t = open(4, 40)
    // A wide (double-column) CJK glyph forces the engine off the all-narrow path.
    t.process_str('CJK: 中文 wide\r\n')
    const rows = createAtermRowRangeReader(t).read(0, 4, 40)
    expect(rows).not.toBeNull()
    const cjkRow = rows!.find((r) => r.text.includes('中'))
    expect(cjkRow).toBeDefined()
    expect(cjkRow!.widths).toBeDefined()
    expect(cjkRow!.widths!.length).toBe(40)
    expect(cjkRow!.widths).toContain('2') // the wide-lead column
  })

  it('returns null for a range outside the live viewport (scrollback is unavailable here)', () => {
    const t = open(4, 40)
    for (let i = 0; i < 20; i += 1) t.process_str(`row-${i}\r\n`)
    // Negative display row (into scrollback) → engine yields undefined → null.
    expect(createAtermRowRangeReader(t).read(-5, 3, 40)).toBeNull()
  })
})

describe('fed E-1 search_index_release — real engine idle eviction', () => {
  it('releases the warm index yet the next search rebuilds byte-identical results', () => {
    const t = open(10, 40)
    feedNeedles(t, 40)
    const reader = createAtermSearchSummaryReader(t)
    const before = reader.read('needle', false, false, 8)
    expect(before).not.toBeNull()

    const release = detectEngineSearchIndexRelease(t)
    expect(typeof release).toBe('function')
    release!() // drops the cached full-content index (real heap eviction)

    // One rebuild is paid; results are byte-identical (the release contract).
    const after = reader.read('needle', false, false, 8)
    expect(after).toEqual(before)
  })
})

describe('§4 unindexed linear-scan degradation — real engine', () => {
  it('an over-budget pane returns REAL viewport matches, flagged incomplete (never silent empty)', () => {
    const t = open(6, 40)
    feedNeedles(t, 60) // 60 lines → deep scrollback, small viewport
    const reader = createFederatedLinearScanReader(t, 6, 40)
    expect(reader).not.toBeNull()
    const result = runScanSync({
      reader: reader!,
      query: 'needle',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 100_000,
      isCancelled: () => false
    } as Parameters<typeof runFederatedLinearScan>[0])
    expect(result).not.toBeNull()
    // Real matches came out of the real viewport rows — not an empty batch.
    expect(result!.matches.length).toBeGreaterThan(0)
    for (const m of result!.matches) {
      expect(m.snippet).toContain('needle')
      expect(m.snippet.slice(m.col, m.col + m.len)).toBe('needle')
    }
    // Deep scrollback is un-indexable on this path → honestly incomplete.
    expect(result!.incomplete).toBe(true)
  })

  it('a match that lives only in un-readable scrollback yields incomplete, not a false exhaustive-empty', () => {
    const t = open(6, 40)
    // Unique token only on an early (scrolled-off) line; viewport has none.
    t.process_str('UNIQTOKEN only here\r\n')
    for (let i = 0; i < 60; i += 1) t.process_str(`filler line ${i}\r\n`)
    const reader = createFederatedLinearScanReader(t, 6, 40)!
    const result = runScanSync({
      reader,
      query: 'UNIQTOKEN',
      caseSensitive: true,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 100_000,
      isCancelled: () => false
    } as Parameters<typeof runFederatedLinearScan>[0])
    expect(result!.matches.length).toBe(0)
    // The critical guarantee: empty-but-INCOMPLETE, never a false "no matches".
    expect(result!.incomplete).toBe(true)
  })
})
