// The per-pane seam the federated-search live adapter reads off a pane
// controller: which engine host answers a federated scan for this pane. Worker
// panes ride the ONE worker-scoped fan-out (their workerPaneId names them in the
// federatedFind pane list); in-process fallback panes expose their local engine
// for a main-thread scan under the SAME E-6 slice budget (§2.1: an unbudgeted
// call there stalls the UI thread itself). Types-only.

import type { FederatedScanEngine } from './aterm-federated-budgeted-scan'

export type AtermFederatedSearchTarget =
  | { kind: 'worker'; workerPaneId: number }
  | {
      kind: 'in-process'
      engine: FederatedScanEngine
      /** Retained depth + viewport rows for the §4 admission estimate. */
      baseY: () => number
      rows: () => number
    }
