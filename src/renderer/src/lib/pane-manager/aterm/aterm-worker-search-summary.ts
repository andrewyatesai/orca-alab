// E-1 `search_summary` snippet consumption (feature-detected; the 4A engine track
// implements the export — glue lands here first, so a pinned artifact WITHOUT it
// keeps the count-only degradation shape: bare match triplets, snippet === null).
//
// CONTRACT (mirrors hasAtermRowRangeExport's fail-closed discipline):
//   search_summary(query, case_sensitive, is_regex, max_matches): string | undefined
// The return is a JSON object of AT MOST `max_matches` span-marked line summaries
// in engine ABSOLUTE-row coordinates (the same space federated matches carry):
//   { "matches": [{ "absRow": number, "col": number, "len": number,
//                   "snippet": string }], "total": number, "incomplete": boolean }
// `undefined` means the summary is unavailable this call (e.g. mid-reindex) — the
// caller keeps its budgeted-scan rows with null snippets for this run. Any
// malformed payload PERMANENTLY disables the reader (glue/blob skew fails closed,
// exactly like createAtermRowRangeReader).
//
// Why snippet-only enrichment (not a second search backend): the budgeted E-6
// cursor stays the bounded match finder (its incremental index build is the
// E-6 invariant). This reader is asked ONLY for the ≤K already-capped result
// rows' text, a bounded enumeration over the index the scan just built — never
// an unbudgeted from-scratch construction on the hot path.

/** The optional summary export surface; absent on pre-E-1 pinned artifacts. */
export type AtermSearchSummaryEngine = {
  search_summary?: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    maxMatches: number
  ) => string | undefined
}

/** One validated summary record (engine absolute-row coordinate). */
export type AtermSearchSummaryMatch = {
  absRow: number
  col: number
  len: number
  snippet: string
}

export type AtermSearchSummary = {
  matches: AtermSearchSummaryMatch[]
  total: number
  incomplete: boolean
}

/** True when the pinned engine artifact exports the E-1 summary read. */
export function hasAtermSearchSummary(term: object): boolean {
  return typeof (term as AtermSearchSummaryEngine).search_summary === 'function'
}

export type AtermSearchSummaryReader = {
  /** The span-marked summary for a query (≤ maxMatches records), or null to keep
   *  the budgeted-scan rows with null snippets (export absent, transiently
   *  unavailable, or disabled on skew). */
  read: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    maxMatches: number
  ) => AtermSearchSummary | null
  /** A row → snippet map for enriching already-found match rows. Only rows the
   *  summary reports carry text; others stay null (honest partial enrichment). */
  snippetsByRow: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    maxMatches: number
  ) => Map<number, string> | null
}

export function createAtermSearchSummaryReader(term: object): AtermSearchSummaryReader {
  // Detected once per reader (per pane engine); a contract violation clears it.
  let exportFn = hasAtermSearchSummary(term)
    ? (term as Required<AtermSearchSummaryEngine>).search_summary.bind(term)
    : null
  const read: AtermSearchSummaryReader['read'] = (query, caseSensitive, isRegex, maxMatches) => {
    if (!exportFn) {
      return null
    }
    const json = exportFn(query, caseSensitive, isRegex, maxMatches)
    if (json === undefined) {
      // Transient (contract): summary unavailable this call — retry next run.
      return null
    }
    const parsed = parseSearchSummary(json)
    if (parsed === null) {
      // Malformed payload = glue/blob skew; fail closed for the reader's lifetime.
      exportFn = null
    }
    return parsed
  }
  return {
    read,
    snippetsByRow: (query, caseSensitive, isRegex, maxMatches) => {
      const summary = read(query, caseSensitive, isRegex, maxMatches)
      if (!summary) {
        return null
      }
      const byRow = new Map<number, string>()
      for (const m of summary.matches) {
        // Keep the newest (highest absRow lookup is by exact row; a row can carry
        // at most one primary match snippet — first wins for a repeated row).
        if (!byRow.has(m.absRow)) {
          byRow.set(m.absRow, m.snippet)
        }
      }
      return byRow
    }
  }
}

/** Strict parse: an object with a `matches` array of well-formed records plus a
 *  numeric `total` and boolean `incomplete`. Null on any violation so a skewed
 *  payload can never reach a federated batch. */
function parseSearchSummary(json: string): AtermSearchSummary | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(json)
  } catch {
    return null
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return null
  }
  const obj = parsed as Partial<AtermSearchSummary>
  if (
    !Array.isArray(obj.matches) ||
    typeof obj.total !== 'number' ||
    typeof obj.incomplete !== 'boolean'
  ) {
    return null
  }
  for (const entry of obj.matches) {
    if (typeof entry !== 'object' || entry === null) {
      return null
    }
    const m = entry as Partial<AtermSearchSummaryMatch>
    if (
      typeof m.absRow !== 'number' ||
      typeof m.col !== 'number' ||
      typeof m.len !== 'number' ||
      typeof m.snippet !== 'string'
    ) {
      return null
    }
  }
  return obj as AtermSearchSummary
}
