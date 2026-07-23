// Production bridge from the feature-detected row_range_json export to the §4
// linear-scan row reader. Reuses the landed strict-parse row-range reader (the
// same one the 4B mirror consumes) so the batch export's fail-closed discipline
// covers the degradation path too. Absent on pre-export pins → null reader →
// the over-budget pane reports an honest empty batch (no behavior change today).
//
// REAL-ENGINE COORDINATE CONSTRAINT (verified against the committed wasm pin,
// not a mock): `row_range_json(first_row, count)` addresses DISPLAY rows and
// serves ONLY the live viewport — it returns `undefined` for any negative
// display row (scrollback ABOVE the viewport). Scrollback text is reachable only
// through the search INDEX (`search_summary`), which the §4 admission rule
// denied for this very pane. So the unindexed degradation can read exactly the
// current viewport window; retained history below it is un-indexable text here
// and the scan settles INCOMPLETE (never a false exhaustive-empty). A future
// engine `abs_row_text`-style wasm export would widen the readable window; until
// then this is the honest bound.
//
// Absolute↔display mapping is the codebase-canonical one (aterm-worker-search.ts,
// aterm-search-overlay.ts): displayRow = absRow − search_display_origin +
// display_offset. So the viewport top's absolute row is
// `search_display_origin − display_offset`, and any retained row below it is the
// un-indexable depth this reader flags via `hasUnreadableDepth`.

import {
  createAtermRowRangeReader,
  hasAtermRowRangeExport
} from './aterm-worker-row-range-export'
import type { LinearScanRowReader } from './aterm-federated-linear-scan'

/** The minimal engine surface the reader needs (row_range_json is feature-
 *  detected separately). `cols` is passed by the caller from the live viewport
 *  dimensions — WorkerEngine does not expose a `cols` getter. */
export type LinearScanEngine = {
  search_display_origin: number
  display_offset: number
}

/** Build a linear-scan row reader over a pane engine, or null when the engine
 *  lacks row_range_json (pre-export pin). `rows`/`cols` are the live viewport.
 *  The readable window is the viewport [base_y, base_y+rows); scrollback below
 *  it is flagged as unreadable depth so the scan settles honestly incomplete. */
export function createFederatedLinearScanReader(
  engine: LinearScanEngine,
  rows: number,
  cols: number
): LinearScanRowReader | null {
  if (!hasAtermRowRangeExport(engine)) {
    return null
  }
  const rowRange = createAtermRowRangeReader(engine)
  // Absolute row of display row 0 (the viewport top) under the canonical mapping.
  const viewportTopAbs = engine.search_display_origin - engine.display_offset
  const viewportRows = Math.max(0, rows)
  return {
    // Oldest READABLE absolute row is the viewport top (row_range_json display 0).
    oldestAbsRow: viewportTopAbs,
    rowCount: viewportRows,
    // A positive viewport-top absolute row means older rows were produced and
    // are unreadable by this path (evicted or in un-indexable scrollback) — a
    // safe over-approximation that keeps the scan's completeness honest.
    hasUnreadableDepth: viewportTopAbs > 0,
    read: (firstAbsRow, count) => {
      // Refuse a range that leaves the readable viewport rather than silently
      // returning fewer/mis-addressed rows — the scan treats null as a gap and
      // settles incomplete, which is the honest outcome for un-indexable rows.
      if (firstAbsRow < viewportTopAbs || firstAbsRow + count > viewportTopAbs + viewportRows) {
        return null
      }
      const records = rowRange.read(firstAbsRow - viewportTopAbs, count, cols)
      return records ? records.map((r) => r.text) : null
    }
  }
}
