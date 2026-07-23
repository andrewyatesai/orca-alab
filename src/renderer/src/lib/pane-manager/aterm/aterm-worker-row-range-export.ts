// E9 batch row-range export (pairs with the P7 grid-mirror redesign): ONE engine
// call per buildState replaces the per-visible-row row_text/row_is_wrapped/row_len
// (+ per-cell cell_is_wide) wasm-boundary walk that dominates mirror cost.
//
// CONTRACT (feature-detected; the 4A engine track implements the export — wire
// glue lands here first, so a pinned artifact WITHOUT the export keeps the
// per-row path):
//   row_range_json(first_row: number, count: number): string | undefined
// `first_row` is a DISPLAY row (display_offset-aware, same coords as row_text);
// the return is a JSON array of exactly `count` records, in row order:
//   { "text": string, "wrapped": boolean, "len": number, "widths"?: string }
// `widths` follows AtermWorkerGridRow's per-column digit convention ('0' spacer /
// '1' normal / '2' wide lead, length == cols) and is OMITTED for all-narrow rows
// so the host reuses its cached all-'1' string (the ASCII fast path, engine-side).
// `undefined` means the range is unavailable this frame (e.g. resize skew) — the
// caller falls back to the per-row path for that frame only. Any malformed payload
// permanently disables the batch path for the reader (glue/blob skew fails closed,
// like hasAtermSpillExports).

/** The optional batch export surface; absent on pre-E9 pinned artifacts. */
export type AtermRowRangeExportEngine = {
  row_range_json?: (first_row: number, count: number) => string | undefined
}

/** One validated batch row record (y is implicit: first_row + array index). */
export type AtermRowRangeRecord = {
  text: string
  wrapped: boolean
  len: number
  /** Omitted by the engine for all-narrow rows; callers substitute all-'1'. */
  widths?: string
}

/** True when the pinned engine artifact exports the batch row-range read. */
export function hasAtermRowRangeExport(term: object): boolean {
  return typeof (term as AtermRowRangeExportEngine).row_range_json === 'function'
}

export type AtermRowRangeReader = {
  /** All `count` visible rows in one wasm-boundary crossing, or null to use the
   *  per-row fallback (export absent, range unavailable, or disabled on skew). */
  read: (firstRow: number, count: number, cols: number) => AtermRowRangeRecord[] | null
}

export function createAtermRowRangeReader(term: object): AtermRowRangeReader {
  // Detected once per reader (per pane engine); a contract violation clears it.
  let exportFn = hasAtermRowRangeExport(term)
    ? (term as Required<AtermRowRangeExportEngine>).row_range_json.bind(term)
    : null
  return {
    read: (firstRow, count, cols) => {
      if (!exportFn) {
        return null
      }
      const json = exportFn(firstRow, count)
      if (json === undefined) {
        // Transient (contract): range unavailable this frame — retry next frame.
        return null
      }
      const rows = parseRowRangeJson(json, count, cols)
      if (rows === null) {
        // Malformed payload = glue/blob skew; fail closed for the reader's lifetime.
        exportFn = null
      }
      return rows
    }
  }
}

/** Strict parse: exactly `count` records, widths (when present) exactly `cols`
 *  digits. Null on any violation so skewed payloads can never reach the mirror. */
function parseRowRangeJson(
  json: string,
  count: number,
  cols: number
): AtermRowRangeRecord[] | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(json)
  } catch {
    return null
  }
  if (!Array.isArray(parsed) || parsed.length !== count) {
    return null
  }
  for (const entry of parsed) {
    if (typeof entry !== 'object' || entry === null) {
      return null
    }
    const row = entry as Partial<AtermRowRangeRecord>
    if (
      typeof row.text !== 'string' ||
      typeof row.wrapped !== 'boolean' ||
      typeof row.len !== 'number' ||
      (row.widths !== undefined && (typeof row.widths !== 'string' || row.widths.length !== cols))
    ) {
      return null
    }
  }
  return parsed as AtermRowRangeRecord[]
}
