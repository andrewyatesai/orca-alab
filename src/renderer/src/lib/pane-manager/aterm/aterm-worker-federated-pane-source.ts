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
    evictWarmIndex: handle.searchIndexRelease
  }
}
