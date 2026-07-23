// Worker-side federated fan-out (FEDERATED-SEARCH-DESIGN §2.1 + §4): walks the
// supplied panes SERIALLY in priority order, scanning each through the budgeted
// E-6 cursor, streaming one batch per pane, and enforcing the §4 memory-admission
// rule — the wasm32 4GB ceiling in the ONE shared worker is the hazard: a worker
// OOM retires rendering for every pane, so never let a federated query make every
// pane's index resident at once.
//
// Never touches any pane's find-bar state (aterm-worker-search): count/activeIndex/
// rects/markers are unperturbed by a federated run — it drives the engine's
// budgeted cursor directly and publishes only worker-scoped batches.

import { runFederatedPaneScan, type FederatedScanEngine } from './aterm-federated-budgeted-scan'
import {
  runFederatedLinearScan,
  type LinearScanRowReader
} from './aterm-federated-linear-scan'
import type {
  AtermWorkerFederatedCommand,
  AtermWorkerFederatedEvent,
  AtermWorkerFederatedFind,
  AtermWorkerFederatedFindPane
} from './aterm-worker-federated-protocol'

// §4 admission constants: a CONSERVATIVE upper-bound on the standing index cost
// and the hard worker-resident budget past which a pane is skipped rather than
// indexed. 1283 is the unreproducible external-audit figure (see
// CODEX-ROADMAP-REVIEW.md — corpus/allocator/query-mode dependent, not in-tree
// reproducible); the honest post-sortedvec measured cost is ~522/339/394 B/line
// (rust/aterm/docs/measured/search-posting-containers.md), with the ≤250 target
// still OPEN. The over-estimate is deliberate: admission skips sooner, never later.
export const FEDERATED_INDEX_BYTES_PER_LINE = 1283
export const FEDERATED_WORKER_INDEX_BUDGET_BYTES = 256 * 1024 * 1024
// Hard row bound on the admission-denial linear scan: the newest rows are
// scanned first and the scan settles incomplete past this — an over-budget pane
// is very deep, so the unindexed fallback must never itself run long.
export const FEDERATED_LINEAR_SCAN_MAX_ROWS = 50_000

/** What the runner needs from one pane: the scan surface + retained-depth reads
 *  for the admission estimate, plus the eviction hooks. */
export type FederatedFindPaneSource = {
  engine: FederatedScanEngine
  /** Retained scrollback depth (absolute base row) + viewport rows — the
   *  admission estimate is (baseY + rows) × 1283 B. */
  baseY: () => number
  rows: () => number
  /** Free budgeted-cursor state (always present on budgeted-capable pins). */
  evictBudgetedState?: () => void
  /** Release the warm completed index (W4A binding; feature-detected). */
  evictWarmIndex?: () => void
  /** §4 admission-denial fallback (feature-detected via row_range_json): a
   *  newest-first row-text reader for the UNINDEXED linear scan. Absent on
   *  pre-export pins — an over-budget pane then reports an honest empty batch
   *  rather than a bounded partial one. */
  linearScanReader?: () => LinearScanRowReader | null
  /** E-1 `search_summary` snippet enrichment (feature-detected): a row → span-
   *  marked text map for the query, bounded to the same match cap. Absent on
   *  pre-E-1 pins — matches then keep null snippets (count-only degradation). */
  readSnippets?: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    maxMatches: number
  ) => Map<number, string> | null
}

/** Attach snippet text to the capped result rows when the pane exposes the E-1
 *  summary export; a no-op (snippets stay null) on pins without it. Enrichment
 *  is over the ≤K already-found rows only — a bounded read, never a re-search. */
function enrichSnippets(
  pane: FederatedFindPaneSource,
  cmd: AtermWorkerFederatedFind,
  matches: { absRow: number; col: number; len: number; snippet: string | null }[]
): void {
  if (!pane.readSnippets || matches.length === 0) {
    return
  }
  const byRow = pane.readSnippets(cmd.query, cmd.caseSensitive, cmd.isRegex, cmd.maxPerPane)
  if (!byRow) {
    return
  }
  for (const match of matches) {
    const snippet = byRow.get(match.absRow)
    if (snippet !== undefined) {
      match.snippet = snippet
    }
  }
}

export type WorkerFederatedFind = {
  dispatch: (cmd: AtermWorkerFederatedCommand) => void
}

export function createWorkerFederatedFind(deps: {
  resolvePane: (paneId: number) => FederatedFindPaneSource | null
  post: (event: AtermWorkerFederatedEvent) => void
}): WorkerFederatedFind {
  type Run = { gen: number; cancelled: boolean }
  let activeRun: Run | null = null

  const startRun = (cmd: AtermWorkerFederatedFind): void => {
    const run: Run = { gen: cmd.gen, cancelled: false }
    activeRun = run
    // Estimated bytes of indexes this run has left resident (visible panes keep
    // theirs warm, find-bar TTL semantics); the hard budget gates admission.
    let residentEstimateBytes = 0
    let index = 0
    const finish = (cancelled: boolean): void => {
      if (activeRun === run) {
        activeRun = null
      }
      deps.post({ type: 'federatedDone', gen: run.gen, cancelled })
    }
    const nextPane = (): void => {
      if (run.cancelled) {
        finish(true)
        return
      }
      if (index >= cmd.panes.length) {
        finish(false)
        return
      }
      const paneRef = cmd.panes[index++]
      // Yield between panes (a fast pane can complete in one synchronous slice):
      // keystroke echo and a just-arrived cancel run before the next pane's scan.
      scanPane(paneRef, () => void setTimeout(nextPane, 0))
    }
    const scanPane = (paneRef: AtermWorkerFederatedFindPane, done: () => void): void => {
      const pane = deps.resolvePane(paneRef.paneId)
      if (!pane) {
        // Pane disposed between the main-side snapshot and the walk — skip silently
        // (the controller's stale-paneRef handling owns the UX for the gap).
        done()
        return
      }
      const estimateBytes = (pane.baseY() + pane.rows()) * FEDERATED_INDEX_BYTES_PER_LINE
      if (residentEstimateBytes + estimateBytes > FEDERATED_WORKER_INDEX_BUDGET_BYTES) {
        // §4 hard budget: refuse to index this pane. Degrade to the bounded
        // UNINDEXED linear scan (never a silent no-results) when the pin ships
        // the row-text reader; otherwise report an honest empty-but-incomplete
        // batch. Never counts against the resident index budget (no index built).
        const reader = pane.linearScanReader?.() ?? null
        if (!reader) {
          deps.post({
            type: 'federatedBatch',
            gen: run.gen,
            paneId: paneRef.paneId,
            matches: [],
            total: 0,
            incomplete: true,
            degraded: 'over-budget'
          })
          done()
          return
        }
        runFederatedLinearScan({
          reader,
          query: cmd.query,
          caseSensitive: cmd.caseSensitive,
          isRegex: cmd.isRegex,
          maxMatches: cmd.maxPerPane,
          maxRowsScanned: FEDERATED_LINEAR_SCAN_MAX_ROWS,
          isCancelled: () => run.cancelled,
          yieldSlice: (next) => void setTimeout(next, 0),
          onDone: (result) => {
            if (result && !run.cancelled) {
              deps.post({
                type: 'federatedBatch',
                gen: run.gen,
                paneId: paneRef.paneId,
                matches: result.matches,
                total: result.total,
                incomplete: result.incomplete,
                degraded: 'linear-scan'
              })
            }
            done()
          }
        })
        return
      }
      runFederatedPaneScan({
        engine: pane.engine,
        query: cmd.query,
        caseSensitive: cmd.caseSensitive,
        isRegex: cmd.isRegex,
        maxMatches: cmd.maxPerPane,
        isCancelled: () => run.cancelled,
        // setTimeout(0) yields to the worker message loop between slices so
        // keystroke echo / newer finds interleave (same cadence as the pane
        // find bar's sliced runner).
        yieldSlice: (next) => void setTimeout(next, 0),
        onDone: (result) => {
          if (paneRef.visible) {
            // Visible panes keep the warm index (their find bar benefits);
            // it still counts against this run's resident budget.
            residentEstimateBytes += estimateBytes
          } else {
            // §4: immediate eviction after each non-visible pane's scan.
            pane.evictBudgetedState?.()
            pane.evictWarmIndex?.()
          }
          if (result && !run.cancelled) {
            // E-1 consumption: enrich the capped rows with span-marked text when
            // the pin ships search_summary; null-snippet triplets otherwise.
            enrichSnippets(pane, cmd, result.matches)
            deps.post({
              type: 'federatedBatch',
              gen: run.gen,
              paneId: paneRef.paneId,
              matches: result.matches,
              total: result.total,
              incomplete: result.incomplete,
              degraded: 'none'
            })
          }
          done()
        }
      })
    }
    // Start async so the dispatch handler returns promptly (and a cancel posted
    // in the same message burst is observed before the first scan).
    setTimeout(nextPane, 0)
  }

  return {
    dispatch: (cmd) => {
      if (cmd.type === 'federatedCancel') {
        if (activeRun && activeRun.gen === cmd.gen) {
          activeRun.cancelled = true
        }
        return
      }
      // A new find always supersedes the in-flight run (its next slice observes
      // the cancel and settles with a cancelled done event).
      if (activeRun) {
        activeRun.cancelled = true
      }
      startRun(cmd)
    }
  }
}
