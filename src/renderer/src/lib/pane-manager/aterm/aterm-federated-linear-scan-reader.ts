// Production bridge from the feature-detected row_range_json export to the §4
// linear-scan row reader. Reuses the landed strict-parse row-range reader (the
// same one the 4B mirror consumes) so the batch export's fail-closed discipline
// covers the degradation path too. Absent on pre-export pins → null reader →
// the over-budget pane reports an honest empty batch (no behavior change today).
//
// Coordinate convention mirrors aterm-search-api.ts's marker math (the in-repo
// source of truth for these engine coordinates): the oldest retained absolute
// row is `search_display_origin - base_y` and the newest+1 is `base_y + rows`;
// a display row is `absRow - base_y` (viewport-relative, negative into
// scrollback — the same coords row_text/row_range_json address). Validated
// against the real engine when the row_range_json pin lands.

import {
  createAtermRowRangeReader,
  hasAtermRowRangeExport
} from './aterm-worker-row-range-export'
import type { LinearScanRowReader } from './aterm-federated-linear-scan'

/** The minimal engine surface the reader needs (row_range_json is feature-
 *  detected separately). `cols` is passed by the caller from the live viewport
 *  dimensions — WorkerEngine does not expose a `cols` getter. */
export type LinearScanEngine = {
  base_y: number
  search_display_origin: number
}

/** Build a linear-scan row reader over a pane engine, or null when the engine
 *  lacks row_range_json (pre-export pin). `rows`/`cols` are the live viewport. */
export function createFederatedLinearScanReader(
  engine: LinearScanEngine,
  rows: number,
  cols: number
): LinearScanRowReader | null {
  if (!hasAtermRowRangeExport(engine)) {
    return null
  }
  const rowRange = createAtermRowRangeReader(engine)
  const oldestAbsRow = engine.search_display_origin - engine.base_y
  const rowCount = Math.max(0, engine.base_y + rows - oldestAbsRow)
  return {
    oldestAbsRow,
    rowCount,
    read: (firstAbsRow, count) => {
      const records = rowRange.read(firstAbsRow - engine.base_y, count, cols)
      return records ? records.map((r) => r.text) : null
    }
  }
}
