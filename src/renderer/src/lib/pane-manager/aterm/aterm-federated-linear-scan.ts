// §4 admission-denial degradation: the UNINDEXED bounded linear scan.
//
// When a pane's estimated index bytes (lines × 1283 B) would breach the
// worker-resident budget, the §4 rule forbids building its posting index —
// but the design mandates the pane still degrade to a bounded linear scan,
// NEVER a silent no-results. This scanner reads already-stripped row TEXT in
// bounded slices (via the feature-detected row_range_json seam — no engine
// index, no 1283 B/line posting cost, each slice's text GC'd before the next)
// and matches newest-row-first, capped in both rows scanned and matches
// returned so the scan itself can never breach time or memory.
//
// It is NOT a TS ANSI strip (policy): the engine already parsed the rows to
// plain text; this only tests a query against clean text, on the explicit
// degradation path §4 defines. A pin WITHOUT row_range_json exposes no reader,
// so the over-budget pane stays an honest empty-but-incomplete batch (flagged,
// surfaced in the palette) rather than a false capability.

import type { AtermFederatedMatch } from './aterm-worker-federated-protocol'

/** Newest-first row-text reader over the retained buffer (row_range_json under
 *  the hood). Absolute-row addressed so the scan and the index path share one
 *  coordinate space. */
export type LinearScanRowReader = {
  /** Oldest retained absolute row (inclusive). */
  oldestAbsRow: number
  /** Retained row count: valid rows are [oldestAbsRow, oldestAbsRow+rowCount). */
  rowCount: number
  /** Read `count` rows starting at `firstAbsRow` (in-range). Null = unavailable
   *  this call (resize skew); the scan settles what it has, flagged incomplete. */
  read: (firstAbsRow: number, count: number) => string[] | null
}

export type FederatedLinearScanResult = {
  /** Newest-first, capped at maxMatches. Snippets are the raw row text (the UI
   *  marks the span via col/len) — richer than the index path's null snippet. */
  matches: AtermFederatedMatch[]
  total: number
  /** True whenever the scan stopped before the oldest row (row/match cap) or a
   *  reader gap truncated it — the honest "not fully searched" signal. */
  incomplete: boolean
}

export type FederatedLinearScanOptions = {
  reader: LinearScanRowReader
  query: string
  caseSensitive: boolean
  isRegex: boolean
  /** Per-pane match cap (the same K the index path honors). */
  maxMatches: number
  /** Hard bound on rows read so an over-budget (very deep) pane's degradation
   *  can never itself run long; newest rows are scanned first. */
  maxRowsScanned: number
  /** Rows per slice (bounded so peak text residency stays tiny). */
  sliceRows?: number
  isCancelled: () => boolean
  yieldSlice: (next: () => void) => void
  /** Exactly one call: the result, or null when cancelled. */
  onDone: (result: FederatedLinearScanResult | null) => void
}

const DEFAULT_SLICE_ROWS = 4096

/** Compile the query into a per-row matcher. Invalid regex → matches nothing
 *  (mirrors the engine: `aterm-wasm` treats an invalid pattern as zero matches),
 *  so a bad pattern degrades quietly rather than throwing into the worker loop. */
function buildMatcher(
  query: string,
  caseSensitive: boolean,
  isRegex: boolean
): ((row: string) => { col: number; len: number }[]) | null {
  if (query === '') {
    return null
  }
  if (isRegex) {
    let regex: RegExp
    try {
      regex = new RegExp(query, caseSensitive ? 'g' : 'gi')
    } catch {
      return null // invalid pattern → zero matches, no throw
    }
    return (row) => {
      const spans: { col: number; len: number }[] = []
      regex.lastIndex = 0
      let m: RegExpExecArray | null
      while ((m = regex.exec(row)) !== null) {
        // A zero-width match must still advance or exec() spins forever.
        const len = m[0].length
        spans.push({ col: m.index, len })
        if (len === 0) {
          regex.lastIndex++
        }
      }
      return spans
    }
  }
  const needle = caseSensitive ? query : query.toLowerCase()
  return (row) => {
    const hay = caseSensitive ? row : row.toLowerCase()
    const spans: { col: number; len: number }[] = []
    let from = 0
    for (;;) {
      const idx = hay.indexOf(needle, from)
      if (idx < 0) {
        break
      }
      spans.push({ col: idx, len: query.length })
      from = idx + query.length
    }
    return spans
  }
}

/** Run one over-budget pane's unindexed linear scan, newest row first. */
export function runFederatedLinearScan(opts: FederatedLinearScanOptions): void {
  const matcher = buildMatcher(opts.query, opts.caseSensitive, opts.isRegex)
  const { reader } = opts
  const sliceRows = Math.max(1, opts.sliceRows ?? DEFAULT_SLICE_ROWS)
  const found: AtermFederatedMatch[] = []
  let total = 0
  let rowsScanned = 0
  // Highest absolute row not yet scanned (exclusive upper edge walks downward).
  let nextTopExclusive = reader.oldestAbsRow + reader.rowCount
  const oldest = reader.oldestAbsRow

  const settle = (incomplete: boolean): void => {
    // Cap the returned matches (total keeps the honest uncapped count found).
    opts.onDone({ matches: found.slice(0, opts.maxMatches), total, incomplete })
  }

  const slice = (): void => {
    if (opts.isCancelled()) {
      opts.onDone(null)
      return
    }
    if (!matcher || nextTopExclusive <= oldest) {
      settle(false) // reached the oldest retained row: fully scanned
      return
    }
    if (rowsScanned >= opts.maxRowsScanned || found.length >= opts.maxMatches) {
      settle(true) // hit a bound with rows still unscanned
      return
    }
    const count = Math.min(sliceRows, nextTopExclusive - oldest)
    const firstAbsRow = nextTopExclusive - count
    const rows = reader.read(firstAbsRow, count)
    if (rows === null) {
      settle(true) // reader gap (resize skew): honest partial result
      return
    }
    // Walk this slice newest-first (bottom row of the slice down to the top).
    for (let i = rows.length - 1; i >= 0; i--) {
      const absRow = firstAbsRow + i
      for (const span of matcher(rows[i])) {
        total++
        if (found.length < opts.maxMatches) {
          found.push({ absRow, col: span.col, len: span.len, snippet: rows[i] })
        }
      }
    }
    rowsScanned += rows.length
    nextTopExclusive = firstAbsRow
    opts.yieldSlice(slice)
  }
  slice()
}
