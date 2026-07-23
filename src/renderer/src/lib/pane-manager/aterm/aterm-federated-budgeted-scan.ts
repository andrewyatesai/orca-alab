// One pane's federated scan through the E-6 budgeted resumable engine API: sliced,
// cancellable, restart-capped — shared by the worker's serial fan-out AND the
// main-thread in-process fallback (which must never run an unbudgeted engine call
// on the UI thread). Deliberately separate from the pane find-bar state machine
// (aterm-worker-search): a federated scan must never touch a pane's active find
// state — it drives the engine's budgeted cursor directly and publishes nothing.

import type { EngineBudgetedSearchStep } from './aterm-engine-budgeted-search'
import type { EngineSearchSummaryFn } from './aterm-engine-search-summary'
import type { AtermFederatedMatch } from './aterm-worker-federated-protocol'
import {
  SEARCH_FIND_MAX_RESTARTS,
  SEARCH_SLICE_BUDGET_MS,
  decodeMatches
} from './aterm-worker-search-sliced-find'

// Same adaptive-row bounds as the pane find bar's sliced runner (not exported
// there); one mis-measured slice must not collapse to per-row calls or balloon
// back into a blocking search.
const SLICE_INITIAL_ROWS = 4096
const SLICE_MIN_ROWS = 256
const SLICE_MAX_ROWS = 262144

/** The engine surface one federated pane scan drives. */
export type FederatedScanEngine = {
  search: (query: string, caseSensitive: boolean, isRegex: boolean) => Uint32Array
  searchBudgeted?: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    cursor: bigint | undefined,
    rowBudget: number
  ) => EngineBudgetedSearchStep
  searchBudgetedCancel?: () => void
  /** E-1 summary binding (feature-detected); preferred for snippets once the
   *  budgeted scan has warmed the index. */
  searchSummary?: EngineSearchSummaryFn
}

export type FederatedPaneScanResult = {
  /** Newest-first (highest absRow first), capped at maxMatches. */
  matches: AtermFederatedMatch[]
  total: number
  incomplete: boolean
}

export type FederatedPaneScanOptions = {
  engine: FederatedScanEngine
  query: string
  caseSensitive: boolean
  isRegex: boolean
  maxMatches: number
  /** Polled between slices; true stops the scan (onDone(null), engine state freed). */
  isCancelled: () => boolean
  /** Yield seam between slices: setTimeout(0) in the worker, idle callbacks on the
   *  main-thread fallback — the caller owns cadence so no path can stall its host. */
  yieldSlice: (next: () => void) => void
  /** Exactly one call: the result, or null when cancelled. */
  onDone: (result: FederatedPaneScanResult | null) => void
}

/** Order matches newest-first and apply the per-pane cap + snippet join. */
function finishScan(
  opts: FederatedPaneScanOptions,
  found: AtermFederatedMatch[],
  incomplete: boolean
): void {
  const total = found.length
  const sorted = [...found].sort((a, b) => b.absRow - a.absRow || b.col - a.col)
  const capped = sorted.slice(0, opts.maxMatches)
  // Snippets ride the E-1 summary binding when present: the budgeted scan above
  // has just warmed the index, so the summary read is O(matches), not a rebuild.
  const summary = opts.engine.searchSummary?.(
    opts.query,
    opts.caseSensitive,
    opts.isRegex,
    opts.maxMatches
  )
  if (summary) {
    const snippetByPos = new Map<string, string | null>()
    for (const m of summary.matches) {
      snippetByPos.set(`${m.absRow}:${m.col}`, m.snippet)
    }
    for (const m of capped) {
      m.snippet = snippetByPos.get(`${m.absRow}:${m.col}`) ?? null
    }
    opts.onDone({
      matches: capped,
      total: Math.max(total, summary.total),
      incomplete: incomplete || summary.incomplete
    })
    return
  }
  opts.onDone({ matches: capped, total, incomplete })
}

/** Run one pane's scan. Every engine call is row-budgeted (E-6); a pane on an
 *  artifact-skew pin without the budgeted API degrades to the same legacy
 *  one-shot the pane find bar uses — bounded by that pin's own behavior. */
export function runFederatedPaneScan(opts: FederatedPaneScanOptions): void {
  const { engine } = opts
  const searchBudgeted = engine.searchBudgeted
  if (!searchBudgeted) {
    if (opts.isCancelled()) {
      opts.onDone(null)
      return
    }
    // Artifact-skew fallback (mirrors the pane find bar): the legacy one-shot,
    // which drops the engine's incomplete signal — reported honestly as false.
    let flat: Uint32Array
    try {
      flat = engine.search(opts.query, opts.caseSensitive, opts.isRegex)
    } catch {
      // Engine freed mid-run (pane disposed) — settle as cancelled, never throw
      // into the shared worker (a crash there retires EVERY pane).
      opts.onDone(null)
      return
    }
    finishScan(
      opts,
      decodeMatches(flat).map((m) => ({
        absRow: m.line,
        col: m.startCol,
        len: m.length,
        snippet: null
      })),
      false
    )
    return
  }
  let cursor: bigint | undefined
  let found: AtermFederatedMatch[] = []
  let restarts = 0
  let sliceRows = SLICE_INITIAL_ROWS
  const slice = (): void => {
    if (opts.isCancelled()) {
      // Free the engine's partial index so a cancelled fan-out leaves nothing resident.
      engine.searchBudgetedCancel?.()
      opts.onDone(null)
      return
    }
    const t0 = performance.now()
    let step: EngineBudgetedSearchStep
    try {
      step = searchBudgeted(opts.query, opts.caseSensitive, opts.isRegex, cursor, sliceRows)
    } catch {
      // Engine freed between slices (pane disposed) — settle as cancelled rather
      // than throwing into the shared worker's loop (a crash retires every pane).
      opts.onDone(null)
      return
    }
    const dt = performance.now() - t0
    sliceRows = Math.min(
      SLICE_MAX_ROWS,
      Math.max(SLICE_MIN_ROWS, Math.round((sliceRows * SEARCH_SLICE_BUDGET_MS) / Math.max(dt, 0.5)))
    )
    const restarted = step.reset && cursor !== undefined
    if (step.reset) {
      found = []
    }
    for (const m of decodeMatches(step.matches)) {
      found.push({ absRow: m.line, col: m.startCol, len: m.length, snippet: null })
    }
    if (step.complete) {
      finishScan(opts, found, step.incompleteIndex)
      return
    }
    if (restarted) {
      restarts++
      if (restarts >= SEARCH_FIND_MAX_RESTARTS) {
        // Streaming-restart settle: publish the scanned prefix flagged incomplete
        // and free the partial index — no unbounded call, ever (E-6 invariant).
        engine.searchBudgetedCancel?.()
        finishScan(opts, found, true)
        return
      }
    }
    cursor = step.cursor
    opts.yieldSlice(slice)
  }
  slice()
}
