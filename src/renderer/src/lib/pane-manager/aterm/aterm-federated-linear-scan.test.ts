import { describe, expect, it } from 'vitest'
import {
  runFederatedLinearScan,
  type FederatedLinearScanResult,
  type LinearScanRowReader
} from './aterm-federated-linear-scan'

/** A reader over an in-memory absolute-row buffer; `oldestAbsRow` offsets the
 *  coordinate space so the scan is exercised away from row 0. */
function bufferReader(
  lines: string[],
  oldestAbsRow = 0,
  opts?: { unavailableAt?: number }
): LinearScanRowReader & { reads: Array<[number, number]> } {
  const reads: Array<[number, number]> = []
  return {
    reads,
    oldestAbsRow,
    rowCount: lines.length,
    read: (firstAbsRow, count) => {
      reads.push([firstAbsRow, count])
      if (opts?.unavailableAt !== undefined && reads.length > opts.unavailableAt) {
        return null
      }
      const start = firstAbsRow - oldestAbsRow
      return lines.slice(start, start + count)
    }
  }
}

function runSync(
  opts: Omit<Parameters<typeof runFederatedLinearScan>[0], 'isCancelled' | 'yieldSlice' | 'onDone'> &
    Partial<Pick<Parameters<typeof runFederatedLinearScan>[0], 'isCancelled'>>
): FederatedLinearScanResult | null {
  let out: FederatedLinearScanResult | null = null
  let settled = false
  runFederatedLinearScan({
    ...opts,
    isCancelled: opts.isCancelled ?? (() => false),
    yieldSlice: (next) => next(), // drive slices synchronously in-test
    onDone: (result) => {
      out = result
      settled = true
    }
  })
  expect(settled).toBe(true)
  return out
}

describe('runFederatedLinearScan', () => {
  it('finds matches newest-row-first with raw-text snippets, fully scanned', () => {
    const result = runSync({
      reader: bufferReader(['alpha needle', 'nothing', 'beta needle', 'gamma']),
      query: 'needle',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(result?.incomplete).toBe(false)
    // Newest row first: row 2 ("beta needle") before row 0 ("alpha needle").
    expect(result?.matches.map((m) => m.absRow)).toEqual([2, 0])
    expect(result?.matches[0]).toEqual({ absRow: 2, col: 5, len: 6, snippet: 'beta needle' })
    expect(result?.total).toBe(2)
  })

  it('addresses rows in the buffer absolute-row space (non-zero oldestAbsRow)', () => {
    const result = runSync({
      reader: bufferReader(['x', 'hit here', 'y'], 1000),
      query: 'hit',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(result?.matches.map((m) => m.absRow)).toEqual([1001])
  })

  it('smart-case already resolved by the caller: caseSensitive=false matches any case', () => {
    const result = runSync({
      reader: bufferReader(['NEEDLE up', 'needle down']),
      query: 'needle',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(result?.total).toBe(2)
    const sensitive = runSync({
      reader: bufferReader(['NEEDLE up', 'needle down']),
      query: 'needle',
      caseSensitive: true,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(sensitive?.matches.map((m) => m.absRow)).toEqual([1])
  })

  it('finds every occurrence within a row, left-to-right', () => {
    const result = runSync({
      reader: bufferReader(['aa aa aa']),
      query: 'aa',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(result?.matches.map((m) => m.col)).toEqual([0, 3, 6])
    expect(result?.total).toBe(3)
  })

  it('supports regex; an invalid pattern degrades to zero matches (no throw)', () => {
    const ok = runSync({
      reader: bufferReader(['err 42', 'ok']),
      query: 'err \\d+',
      caseSensitive: false,
      isRegex: true,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(ok?.matches).toEqual([{ absRow: 0, col: 0, len: 6, snippet: 'err 42' }])
    const invalid = runSync({
      reader: bufferReader(['err 42']),
      query: '(unclosed',
      caseSensitive: false,
      isRegex: true,
      maxMatches: 50,
      maxRowsScanned: 1000
    })
    expect(invalid?.matches).toEqual([])
  })

  it('is BOUNDED: stops at maxRowsScanned with incomplete=true (never scans a huge buffer whole)', () => {
    const deep = Array.from({ length: 100_000 }, (_, i) => (i === 10 ? 'needle' : `line ${i}`))
    const reader = bufferReader(deep)
    const result = runSync({
      reader,
      query: 'needle',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 5000,
      sliceRows: 1000
    })
    expect(result?.incomplete).toBe(true)
    // Only the newest 5000 rows were read — the deep 'needle' at row 10 is NOT
    // reached, and the scan stopped without reading the whole buffer.
    const rowsRead = reader.reads.reduce((n, [, count]) => n + count, 0)
    expect(rowsRead).toBeLessThanOrEqual(5000 + 1000)
    expect(result?.matches).toEqual([])
  })

  it('stops at the match cap with incomplete=true and total as a floor', () => {
    const many = Array.from({ length: 200 }, () => 'hit')
    const result = runSync({
      reader: bufferReader(many),
      query: 'hit',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1_000_000,
      sliceRows: 10
    })
    expect(result?.matches).toHaveLength(50)
    expect(result?.incomplete).toBe(true)
    expect(result?.total).toBeGreaterThanOrEqual(50)
  })

  it('a reader gap (resize skew) settles the partial result flagged incomplete', () => {
    const result = runSync({
      reader: bufferReader(Array.from({ length: 30 }, (_, i) => `row ${i}`), 0, { unavailableAt: 1 }),
      query: 'row',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000,
      sliceRows: 5
    })
    expect(result?.incomplete).toBe(true)
  })

  it('cancellation between slices settles onDone(null)', () => {
    let calls = 0
    const result = runSync({
      reader: bufferReader(Array.from({ length: 100 }, () => 'hit')),
      query: 'zzz',
      caseSensitive: false,
      isRegex: false,
      maxMatches: 50,
      maxRowsScanned: 1000,
      sliceRows: 10,
      isCancelled: () => ++calls > 1 // cancel after the first slice
    })
    expect(result).toBeNull()
  })
})
