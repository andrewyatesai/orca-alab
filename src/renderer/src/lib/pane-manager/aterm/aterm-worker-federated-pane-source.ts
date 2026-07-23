// Adapts one PaneRuntime's engine handle into the federated fan-out's pane
// source (scan surface + admission reads + eviction hooks). Split from the
// worker entry to keep it under the line cap.

import type { PaneRuntime } from './aterm-worker-pane-dispatch'
import {
  createWorkerFederatedFind,
  type FederatedFindPaneSource,
  type WorkerFederatedFind
} from './aterm-worker-federated-find'
import type { AtermWorkerFederatedEvent } from './aterm-worker-federated-protocol'
import { createAtermSearchSummaryReader } from './aterm-worker-search-summary'
import { createFederatedLinearScanReader } from './aterm-federated-linear-scan-reader'

/** The worker entry's one-liner: a federated runner over its pane registry. */
export function createWorkerFederatedFindForPanes(
  panes: Map<number, PaneRuntime>,
  post: (event: AtermWorkerFederatedEvent) => void
): WorkerFederatedFind {
  return createWorkerFederatedFind({
    resolvePane: (paneId) => federatedPaneSourceFromRuntime(panes.get(paneId)),
    post
  })
}

export function federatedPaneSourceFromRuntime(
  pane: PaneRuntime | undefined
): FederatedFindPaneSource | null {
  const handle = pane?.engineHandle
  if (!pane || !handle) {
    return null
  }
  // E-1 consumption (feature-detected): a summary reader over the pane's own wasm
  // engine, used ONLY to enrich the ≤K already-found rows with span-marked text —
  // absent on pre-E-1 pins, in which case snippets stay null (count-only shape).
  const summaryReader = createAtermSearchSummaryReader(handle.engine)
  return {
    // E-6 (binding): only the budgeted cursor surface is exposed to the
    // federated scan — no one-shot, no unbudgeted summary.
    engine: {
      searchBudgeted: handle.searchBudgeted,
      searchBudgetedCancel: handle.searchBudgetedCancel
    },
    baseY: () => handle.engine.base_y,
    rows: () => pane.term?.dimensions().rows ?? 0,
    evictBudgetedState: handle.searchBudgetedCancel,
    evictWarmIndex: handle.searchIndexRelease,
    // §4 admission-denial fallback: a row-text reader off row_range_json, built
    // fresh per scan so it reflects the current viewport/retention (null on pins
    // without the export — over-budget then stays an honest empty batch).
    linearScanReader: () => {
      const dims = pane.term?.dimensions()
      return createFederatedLinearScanReader(handle.engine, dims?.rows ?? 0, dims?.cols ?? 0)
    },
    readSnippets: (query, caseSensitive, isRegex, maxMatches) =>
      summaryReader.snippetsByRow(query, caseSensitive, isRegex, maxMatches)
  }
}
