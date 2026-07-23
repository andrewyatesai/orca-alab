// Builds a pane controller's federatedSearchTarget member: a worker pane is
// named by its shared-worker slot id in the ONE fan-out message; an in-process
// pane exposes its local engine for a main-thread scan under the same E-6 slice
// budget (never an unbudgeted call). Split from the wiring for the line cap.

import { copyBudgetedStep } from './aterm-engine-budgeted-search'
import type { AtermFederatedSearchTarget } from './aterm-federated-search-target'
import type { AtermTerminal } from './aterm_wasm.js'

export function buildFederatedSearchTarget(
  term: AtermTerminal,
  pending: { federatedWorkerPaneId?: number },
  gridSizing: { grid: () => { rows: number } },
  isDisposed: () => boolean
): () => AtermFederatedSearchTarget | null {
  return () => {
    if (isDisposed()) {
      return null
    }
    if (pending.federatedWorkerPaneId !== undefined) {
      return { kind: 'worker', workerPaneId: pending.federatedWorkerPaneId }
    }
    return {
      kind: 'in-process',
      // E-6 (binding): only the budgeted cursor surface — a skew pin without it
      // is refused by the scan runner, never scanned unbudgeted on this thread.
      engine: {
        // Guarded per call: artifact skew can leave a pin without the budgeted API.
        searchBudgeted:
          typeof term.search_budgeted === 'function'
            ? (q, cs, regex, cursor, rows) =>
                copyBudgetedStep(term.search_budgeted(q, cs, regex, cursor, rows))
            : undefined,
        searchBudgetedCancel:
          typeof term.search_budgeted_cancel === 'function'
            ? () => term.search_budgeted_cancel()
            : undefined
      },
      baseY: () => term.base_y,
      rows: () => gridSizing.grid().rows
    }
  }
}
